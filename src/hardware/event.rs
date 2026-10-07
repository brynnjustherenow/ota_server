use chrono::{Local, Utc};
use serde::Deserialize;
use sqlx::SqlitePool;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::service::ws::{ActivityMsg, broadcast_activity};

#[derive(Debug, Deserialize)]
struct StatusMessage {
    device_id: Option<String>,
    ev: Option<String>,
    ev_detail: Option<String>,
    ev_ts: Option<i64>,
    ts: Option<i64>,
    record_count: Option<i64>,
    /// 服务端自身发布的视频事件回环（字段名是 event 而非 ev）
    event: Option<String>,
    /// 硬件消息类型：type=env（周期环境）/ type=lifecycle（生命周期）
    #[serde(rename = "type")]
    msg_type: Option<String>,
    /// 旧 mock 格式的嵌套 env 字段（兼容保留）
    env: Option<serde_json::Value>,
    /// 周期数据上报
    record: Option<serde_json::Value>,
}

/// 处理 status topic 的消息：识别 ev 字段的生命周期事件，入库 + 按需 Bark 推送。
///
/// 推送策略（由用户选定）：
/// - RESET：总是推送（设备崩溃/OTA/信号触发的复位）
/// - BOOT：上一条事件不是 RESET 时推送（= 冷启动/断电/意外重启）；复位后的 BOOT 不推
/// - 其他 ev（APP_START/NEXT_CYCLE）：仅入库
///
/// 非 ev 消息（record/env）不入库，但活动时间与 WebSocket 广播仍会触达。
pub async fn handle_status_message(
    db: &SqlitePool,
    bark_key: &str,
    payload: &str,
    activity_tx: &broadcast::Sender<ActivityMsg>,
) -> anyhow::Result<()> {
    let msg: StatusMessage = match serde_json::from_str(payload) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };

    // 活动统计：任何携带 device_id 的消息都算设备活动（record/env 等非 ev 消息也含在内）。
    // 放在 ev 判断之前——非 ev 消息会在下方提前 return，不触达就漏计。
    // device_id 缺失时静默跳过更新（活动归属不明）。
    let did = msg
        .device_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    if let Some(did) = &did {
        crate::service::device::touch_device_activity(db, did).await;
        // env 数据源：真实硬件 type="env"（字段扁平，整个 payload 即数据）；
        // 兼容旧 mock 格式的嵌套 env 字段。
        let env_data = if msg.msg_type.as_deref() == Some("env") {
            serde_json::from_str::<serde_json::Value>(payload).ok()
        } else {
            msg.env.clone()
        };
        // env 持久化（历史查询用）：payload 原样入库;
        // ts 防御:异常值(毫秒 epoch/脏数据)回退为当前时间
        if let Some(env) = &env_data {
            let ts = msg
                .ts
                .filter(|t| (1_000_000_000..4_000_000_000).contains(t))
                .unwrap_or_else(|| Utc::now().timestamp());
            if let Err(e) = crate::service::env::insert_env(db, did, ts, env).await {
                warn!("[EVENT] persist env failed for {did}: {e}");
            }
        }
        // 服务端自身发布的视频事件回环（event=video_uploaded）：
        // 上传路径已按 kind=video 广播过，跳过避免前端收到重复推送。
        if msg.event.as_deref() != Some("video_uploaded") {
            let kind = if msg.ev.is_some() {
                "event"
            } else if env_data.is_some() {
                "env"
            } else if msg.record.is_some() {
                "record"
            } else {
                "activity"
            };
            let detail = match kind {
                "event" => msg.ev_detail.clone(),
                "env" => summarize_env(env_data.as_ref()),
                "record" => summarize_record(msg.record.as_ref()),
                _ => None,
            };
            // WebSocket 实时推送（尽力而为）
            broadcast_activity(
                activity_tx,
                ActivityMsg {
                    device_id: did.clone(),
                    source: "mqtt".into(),
                    kind: kind.into(),
                    event: msg.ev.clone(),
                    detail,
                    ts: Utc::now().timestamp(),
                },
            );
        }
    }

    let Some(ev) = msg.ev.as_deref() else {
        return Ok(());
    };

    let device_id = did.unwrap_or_else(|| "unknown".into());
    let detail = msg.ev_detail.unwrap_or_default();
    let ev_ts = msg.ev_ts.or(msg.ts).unwrap_or(0);
    let received_at = Local::now().to_rfc3339();

    let last: Option<(String,)> = sqlx::query_as(
        "SELECT event FROM device_events WHERE device_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(&device_id)
    .fetch_optional(db)
    .await?;

    sqlx::query(
        "INSERT INTO device_events (device_id, event, detail, ev_ts, received_at, raw) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&device_id)
    .bind(ev)
    .bind(&detail)
    .bind(ev_ts)
    .bind(&received_at)
    .bind(payload)
    .execute(db)
    .await?;

    info!("[EVENT] {device_id} {ev} {detail} ts={ev_ts}");

    let should_push = match ev {
        "RESET" => true,
        "BOOT" => match &last {
            Some((e,)) => e != "RESET",
            None => true,
        },
        _ => false,
    };

    if should_push && !bark_key.is_empty() {
        let title = match ev {
            "RESET" => format!("{device_id} 复位"),
            _ => format!("{device_id} 启动"),
        };
        let round_str = msg
            .record_count
            .map(|r| format!("\n轮次: {r}"))
            .unwrap_or_default();
        let body = format!("原因: {detail}{round_str}\n收到: {received_at}");
        if let Err(e) = bark_notify(bark_key, &title, &body).await {
            warn!("[EVENT] Bark 推送失败: {e}");
        }
    }

    Ok(())
}

/// env 消息摘要：真实硬件字段 temp_c/hum_pct 优先，兼容旧 mock 的 temp/humi
fn summarize_env(env: Option<&serde_json::Value>) -> Option<String> {
    let env = env?;
    let temp = env
        .get("temp_c")
        .or_else(|| env.get("temp"))
        .and_then(|v| v.as_f64());
    let hum = env
        .get("hum_pct")
        .or_else(|| env.get("humi"))
        .and_then(|v| v.as_f64());
    let mut parts = Vec::new();
    if let Some(t) = temp {
        parts.push(format!("{t:.1}°C"));
    }
    if let Some(h) = hum {
        parts.push(format!("{h:.0}%"));
    }
    if parts.is_empty() {
        Some("环境数据".into())
    } else {
        Some(parts.join(" "))
    }
}

/// record 消息摘要：优先取 seq
fn summarize_record(rec: Option<&serde_json::Value>) -> Option<String> {
    let rec = rec?;
    match rec.get("seq").and_then(|v| v.as_i64()) {
        Some(seq) => Some(format!("seq={seq}")),
        None => Some("周期数据".into()),
    }
}

async fn bark_notify(key: &str, title: &str, body: &str) -> anyhow::Result<()> {
    let url = format!("https://api.day.app/{key}");
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .form(&[("title", title), ("body", body)])
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("Bark 返回 HTTP {}", resp.status());
    }
    Ok(())
}
