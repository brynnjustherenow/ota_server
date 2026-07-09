use std::time::{Duration, Instant};

use axum::http::StatusCode;
use chrono::Local;
use mqtt5::{Message, MqttClient, PublishOptions, callback};
use postcard::{Error, to_vec};
use serde::{Deserialize, Serialize};
use tokio::time::interval;
use tracing::{info, warn};

use crate::{AppState, service::OtaError};
impl AppState {
    pub async fn sent_command(&self, topic: &str, msg: Vec<u8>) -> Result<(), OtaError> {
        self.ota_client.publish(topic, msg).await?;
        Ok(())
    }

    pub async fn subcribe<F>(
        &self,
        client: &MqttClient,
        topic: String,
        callback: F,
    ) -> Result<(), String>
    where
        F: Fn(Message) + Send + Sync + 'static,
    {
        self.ota_client
            .subscribe(topic, callback)
            .await
            .map_err(|_| "subscribe failed...".to_string())?;
        Ok(())
    }
    pub async fn send_version(&self, version: String) -> Result<(), OtaError> {
        self.send_fleet_update(&version, None).await
    }

    /// 发布 retained 消息：同 topic 仅保留最后一条，故版本与配置合并为 fleet_update。
    pub async fn publish_retained(&self, topic: &str, msg: Vec<u8>) -> Result<(), OtaError> {
        let mut opts = PublishOptions::default();
        opts.retain = true;
        self.ota_client.publish_with_options(topic, msg, opts).await?;
        Ok(())
    }

    /// 合并下发版本号 + 可选配置（设备收到后触发 OTA 比对 + config merge）。
    pub async fn send_fleet_update(
        &self,
        version: &str,
        config: Option<&serde_json::Value>,
    ) -> Result<(), OtaError> {
        let topic = &self.config.mqtt_conf.cmd_topic;
        let payload = match config {
            Some(c) => serde_json::json!({ "cmd": "fleet_update", "version": version, "config": c }),
            None => serde_json::json!({ "cmd": "fleet_update", "version": version }),
        };
        let payload = serde_json::to_vec(&payload)
            .map_err(|e| OtaError::InvalidInput(format!("序列化 fleet_update 失败: {e}")))?;
        self.publish_retained(topic, payload).await
    }

    pub async fn send_video_event(&self, device_id: &str, filename: &str) -> Result<(), OtaError> {
        let topic = &self.config.mqtt_conf.status_topic;
        let event = VideoEvent {
            event: "video_uploaded".to_string(),
            device_id: device_id.to_string(),
            filename: filename.to_string(),
            ts: Local::now().timestamp_millis() as u64,
        };
        let payload = serde_json::to_vec(&event)
            .map_err(|e| OtaError::InvalidInput(format!("序列化 video event 失败: {e}")))?;
        self.sent_command(topic, payload).await
    }
}
///下发的指令，时间戳、最新版本、文件列表
#[derive(Debug, Deserialize, Serialize)]
#[repr(C)]
struct OtaMessage {
    ts: u64,
    version: String,
}
impl TryInto<Vec<u8>> for OtaMessage {
    type Error = String;
    fn try_into(self) -> Result<Vec<u8>, Self::Error> {
        let json = serde_json::to_string(&self).map_err(|e| e.to_string())?;
        Ok(json.into_bytes())
    }
}

/// 视频上传事件，推送至 status_topic 供订阅方实时联动
#[derive(Debug, Serialize)]
struct VideoEvent {
    event: String,
    device_id: String,
    filename: String,
    ts: u64,
}
#[repr(C)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OtaMetadata {
    create_at: u64,
    version: String,
    file_count: u32,
    files: Vec<OtaFileMetadata>,
}
impl OtaMetadata {
    pub fn new(version: String, files: Vec<OtaFileMetadata>) -> Self {
        Self {
            create_at: Local::now().timestamp_millis() as u64,
            file_count: files.len() as u32,
            version,
            files,
        }
    }

    /// 根据相对路径（如 `lib/foo.mpy` 或 `main.mpy`）查找文件元信息
    pub fn find_file(&self, rel_path: &str) -> Option<&OtaFileMetadata> {
        let (path, name) = split_rel_path(rel_path);
        self.files.iter().find(|f| f.path == path && f.name == name)
    }
}
#[repr(C)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OtaFileMetadata {
    name: String,
    #[serde(default)]
    path: String,
    md5: String,
    #[serde(default)]
    size: u64,
}
impl OtaFileMetadata {
    pub fn new(name: String, path: String, md5: String, size: u64) -> Self {
        Self {
            name,
            path,
            md5,
            size,
        }
    }

    pub fn md5(&self) -> &str {
        &self.md5
    }
}

/// 把 `lib/sub/foo.mpy` 拆成 (`"lib/sub"`, `"foo.mpy"`)；无 `/` 时 path 为空串
pub fn split_rel_path(rel: &str) -> (String, String) {
    match rel.rfind('/') {
        Some(idx) => (rel[..idx].to_string(), rel[idx + 1..].to_string()),
        None => (String::new(), rel.to_string()),
    }
}
/// 每 10 分钟检测最新已发布版本，合并其 config（sqlite）后以 retained 广播。
/// 设备每次订阅 cmd topic 即收到最后一条 fleet_update，无需服务端精确把握设备在线时刻。
pub fn interval_publish_service(app_state: &AppState) {
    let app = app_state.clone();
    tokio::spawn(async move {
        // interval 首次 tick 立即触发，之后每 10 分钟一次
        let mut interval = interval(Duration::from_secs(600));
        loop {
            interval.tick().await;
            match crate::service::ota::latest_published_version().await {
                Some(version) => {
                    let decision =
                        crate::service::ota::resolve_broadcast(&app.db, &version).await;
                    if decision.skip {
                        info!(
                            "version {version} 文件夹为空且无 config，跳过广播（保留上一条 retained）"
                        );
                        continue;
                    }
                    if app.send_fleet_update(&version, decision.config.as_ref()).await.is_err() {
                        warn!("fleet_update publish failed");
                    } else {
                        info!("fleet_update broadcast version={version}");
                    }
                }
                None => info!("no published version yet, skip"),
            }
        }
    });
}
