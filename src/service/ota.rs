use std::path::Path;

use axum::{
    Json,
    extract::{Multipart, Path as AxumPath, Query, State},
};
use futures_util::TryStreamExt;
use md5::{Digest, Md5};
use serde::Deserialize;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::io::StreamReader;

use crate::{
    AppState,
    hardware::mqtt::{OtaFileMetadata, OtaMetadata, split_rel_path},
    service::{OtaError, Response},
};

/// 作为设备 config 模板的参考配置（来自项目根 device_cfg.json，编译期嵌入）。
/// 前端「生成设备模板」按钮会拉取此内容作为起点。
const DEVICE_CFG_TEMPLATE: &str = include_str!("../../device_cfg.json");

#[derive(Deserialize)]
pub struct NotifyQuery {
    /// 可选 device_id：非空时下发的 MQTT 消息会带上 device_id，仅匹配设备应用。
    #[serde(default)]
    pub device_id: Option<String>,
    /// 是否携带该设备专属 config（默认 true）。
    /// false 时即使设备有专属 config 也忽略，退回到版本级（全局）config；
    /// 全局 config 也不存在则不下发 config（仅版本号 + device_id）。
    #[serde(default = "default_true")]
    pub carry_device_config: bool,
}

fn default_true() -> bool {
    true
}

pub async fn ota_update(
    AxumPath(version): AxumPath<String>,
    State(app_state): State<AppState>,
    Query(q): Query<NotifyQuery>,
) -> Result<Response<()>, OtaError> {
    let device_id = q
        .device_id
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    // 指定 device_id 时：
    //   carry_device_config=true  → 设备专属 config（无则退回全局）
    //   carry_device_config=false → 忽略设备专属，直接用全局（全局无则 None = 无配置改动）
    // 未指定 device_id 时走版本级广播决策。
    let config = if let Some(did) = device_id {
        if q.carry_device_config {
            let dev_cfg = get_device_config_raw(&app_state.db, &version, did).await;
            if dev_cfg.is_some() {
                dev_cfg
            } else {
                get_version_config_raw(&app_state.db, &version).await
            }
        } else {
            get_version_config_raw(&app_state.db, &version).await
        }
    } else {
        let decision = resolve_broadcast(&app_state.db, &version).await;
        if decision.skip {
            return Err(OtaError::InvalidInput(format!(
                "版本 {version} 文件夹不存在/为空 且 无 config，跳过广播"
            )));
        }
        decision.config
    };

    app_state
        .send_fleet_update(&version, config.as_ref(), device_id)
        .await?;
    Ok(Response::success(()))
}

/// 版本文件夹是否存在且非空（至少一个条目）
pub async fn version_folder_has_files(version: &str) -> bool {
    let dir = Path::new("uploads").join(version);
    let Ok(mut entries) = fs::read_dir(&dir).await else {
        return false;
    };
    while let Ok(Some(_)) = entries.next_entry().await {
        return true;
    }
    false
}

/// 广播决策：版本文件夹不存在/为空 且 无 config → skip=true（跳过，保留上一条 retained）。
/// 否则 skip=false，config 为该版本 config（可能 None）。
pub struct BroadcastDecision {
    pub skip: bool,
    pub config: Option<serde_json::Value>,
}

pub async fn resolve_broadcast(pool: &sqlx::SqlitePool, version: &str) -> BroadcastDecision {
    let config = get_version_config_raw(pool, version).await;
    let has_files = version_folder_has_files(version).await;
    if !has_files && config.is_none() {
        return BroadcastDecision { skip: true, config: None };
    }
    BroadcastDecision { skip: false, config }
}

/// GET /ota
/// 列出所有已发布的版本号，按 SemVer 倒序（最新在前）
pub async fn list_versions() -> Result<Response<Vec<String>>, OtaError> {
    Ok(Response::success(sorted_versions().await))
}

/// 读取 uploads/ 下所有版本目录，按 SemVer 倒序返回（最新在前）
async fn sorted_versions() -> Vec<String> {
    let root = Path::new("uploads");
    let mut entries = match fs::read_dir(root).await {
        Ok(e) => e,
        Err(_) => return vec![],
    };
    let mut versions: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let Ok(ft) = entry.file_type().await else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        if let Ok(name) = entry.file_name().into_string()
            && !name.starts_with('.')
        {
            versions.push(name);
        }
    }
    versions.sort_by(|a, b| {
        version_compare::compare(b, a)
            .ok()
            .and_then(|c| c.ord())
            .unwrap_or_else(|| b.cmp(a))
    });
    versions
}

#[derive(Deserialize)]
pub struct ConfigReq {
    config: serde_json::Value,
}

/// 附加配置的必填校验：config 必须是 JSON 对象，且根级含非空字符串的 `version` 字段。
/// 无论全局还是设备专属，缺 version 一律拒绝。
fn require_config_version(config: &serde_json::Value) -> Result<(), OtaError> {
    let obj = config
        .as_object()
        .ok_or_else(|| OtaError::InvalidInput("config 必须是 JSON 对象".to_string()))?;
    let ok = obj
        .get("version")
        .and_then(|v| v.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if !ok {
        return Err(OtaError::InvalidInput(
            "config 必填字段 version 缺失或为空（根级非空字符串）".to_string(),
        ));
    }
    Ok(())
}

/// 取某版本的 config（供 interval/notify 下发）。无则 None。
pub async fn get_version_config_raw(
    pool: &sqlx::SqlitePool,
    version: &str,
) -> Option<serde_json::Value> {
    let row: Option<(String,)> = sqlx::query_as("SELECT config FROM version_config WHERE version = ?")
        .bind(version)
        .fetch_optional(pool)
        .await
        .ok()?;
    row.and_then(|(s,)| serde_json::from_str(&s).ok())
}/// POST /ota/{version}/config  body: {"config": {...}}
pub async fn set_version_config(
    State(app_state): State<AppState>,
    AxumPath(version): AxumPath<String>,
    Json(req): Json<ConfigReq>,
) -> Result<Response<()>, OtaError> {
    require_config_version(&req.config)?;
    let config_json = serde_json::to_string(&req.config)
        .map_err(|e| OtaError::InvalidInput(format!("序列化 config 失败: {e}")))?;
    sqlx::query("INSERT OR REPLACE INTO version_config (version, config) VALUES (?, ?)")
        .bind(&version)
        .bind(&config_json)
        .execute(&app_state.db)
        .await?;
    Ok(Response::success(()))
}

/// GET /ota/{version}/config
pub async fn get_version_config(
    State(app_state): State<AppState>,
    AxumPath(version): AxumPath<String>,
) -> Result<Response<serde_json::Value>, OtaError> {
    match get_version_config_raw(&app_state.db, &version).await {
        Some(v) => Ok(Response::success(v)),
        None => Err(OtaError::FileNotFound(format!("版本 {version} 无配置"))),
    }
}

/// 版本发布：先写入 staging 目录，全部成功后原子替换为 uploads/{version}/，
/// 任一步失败都会清理 staging，不会留下半成品目录。若目标版本已存在则覆盖。
pub async fn ota_publish(
    AxumPath(version): AxumPath<String>,
    mut multipart: Multipart,
) -> Result<Response<()>, OtaError> {
    let root = Path::new("uploads");
    let target = root.join(&version);
    let staging = root.join(format!(".{version}.staging"));

    let work: Result<(), OtaError> = async {
        fs::create_dir_all(root).await?;
        // 清理可能残留的 staging 目录
        let _ = fs::remove_dir_all(&staging).await;
        fs::create_dir(&staging).await?;

        let mut files: Vec<OtaFileMetadata> = Vec::new();
        let mut buf = vec![0u8; 16 * 1024];

        while let Some(field) = multipart
            .next_field()
            .await
            .map_err(|_| OtaError::InvalidInput("multipart 解析失败".into()))?
        {
            let raw = field
                .file_name()
                .ok_or_else(|| OtaError::InvalidInput("缺少文件名".into()))?
                .to_string();
            // 防路径穿越：禁止 `..`、反斜杠、绝对路径
            if raw.contains("..") || raw.contains('\\') || raw.starts_with('/') {
                return Err(OtaError::InvalidInput(format!("非法文件路径: {raw}")));
            }
            // 拆分嵌套路径，如 `lib/foo.mpy` -> (path="lib", name="foo.mpy")
            let (path, filename) = split_rel_path(&raw);

            // 写入子目录（如有）
            let file_dir = if path.is_empty() {
                staging.clone()
            } else {
                let d = staging.join(&path);
                fs::create_dir_all(&d).await?;
                d
            };

            let body = field.map_err(std::io::Error::other);
            let mut reader = StreamReader::new(body);
            let mut file = fs::File::create(file_dir.join(&filename)).await?;
            let mut hasher = Md5::new();
            let mut size: u64 = 0;
            loop {
                let n = reader
                    .read(&mut buf)
                    .await
                    .map_err(|_| OtaError::InvalidInput("读取分片失败".into()))?;
                if n == 0 {
                    break;
                }
                file.write_all(&buf[..n]).await?;
                hasher.update(&buf[..n]);
                size += n as u64;
            }
            let md5_hex = format!("{:x}", hasher.finalize());
            files.push(OtaFileMetadata::new(filename, path, md5_hex, size));
        }

        let metadata = OtaMetadata::new(version, files);
        let manifest = serde_json::to_vec_pretty(&metadata)
            .map_err(|e| OtaError::InvalidInput(format!("序列化 manifest 失败: {e}")))?;
        fs::write(staging.join("manifest.json"), manifest).await?;

        // 提交：清理旧版本（若存在），再原子 rename staging -> target
        if fs::try_exists(&target).await? {
            fs::remove_dir_all(&target).await?;
        }
        fs::rename(&staging, &target).await?;
        Ok(())
    }
    .await;

    if let Err(e) = work {
        // 任一步失败都清理 staging，避免残留
        let _ = fs::remove_dir_all(&staging).await;
        return Err(e);
    }

    Ok(Response::success(()))
}

// ───────────────────────── 设备专属 config ─────────────────────────
//
// 版本级 config（version_config 表）：对整个 fleet 生效，每 10 分钟自动广播。
// 设备级 config（device_version_config 表）：仅对单个 device_id 生效，
// 需手动「发布」时携带 device_id，下发的 MQTT 消息会带 device_id 字段，
// 设备端收到后比对自身 ID，匹配才应用（覆盖版本级 config）。

/// 取某版本下某设备的 config。无则 None。
pub async fn get_device_config_raw(
    pool: &sqlx::SqlitePool,
    version: &str,
    device_id: &str,
) -> Option<serde_json::Value> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT config FROM device_version_config WHERE version = ? AND device_id = ?")
            .bind(version)
            .bind(device_id)
            .fetch_optional(pool)
            .await
            .ok()?;
    row.and_then(|(s,)| serde_json::from_str(&s).ok())
}

/// GET /ota/{version}/devices  → 列出该版本下已配置过的 device_id
pub async fn list_device_configs(
    State(app_state): State<AppState>,
    AxumPath(version): AxumPath<String>,
) -> Result<Response<Vec<String>>, OtaError> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT device_id FROM device_version_config WHERE version = ? ORDER BY device_id")
            .bind(&version)
            .fetch_all(&app_state.db)
            .await?;
    Ok(Response::success(rows.into_iter().map(|(d,)| d).collect()))
}

/// GET /ota/{version}/config/{device_id}
pub async fn get_device_config(
    State(app_state): State<AppState>,
    AxumPath((version, device_id)): AxumPath<(String, String)>,
) -> Result<Response<serde_json::Value>, OtaError> {
    match get_device_config_raw(&app_state.db, &version, &device_id).await {
        Some(v) => Ok(Response::success(v)),
        None => Err(OtaError::FileNotFound(format!(
            "版本 {version} 下设备 {device_id} 无专属配置"
        ))),
    }
}

/// POST /ota/{version}/config/{device_id}  body: {"config": {...}}
pub async fn set_device_config(
    State(app_state): State<AppState>,
    AxumPath((version, device_id)): AxumPath<(String, String)>,
    Json(req): Json<ConfigReq>,
) -> Result<Response<()>, OtaError> {
    save_device_config(&app_state.db, &version, &device_id, &req.config).await?;
    Ok(Response::success(()))
}

async fn save_device_config(
    pool: &sqlx::SqlitePool,
    version: &str,
    device_id: &str,
    config: &serde_json::Value,
) -> Result<(), OtaError> {
    require_config_version(config)?;
    let config_json = serde_json::to_string(config)
        .map_err(|e| OtaError::InvalidInput(format!("序列化 config 失败: {e}")))?;
    sqlx::query(
        "INSERT OR REPLACE INTO device_version_config (version, device_id, config) VALUES (?, ?, ?)",
    )
    .bind(version)
    .bind(device_id)
    .bind(&config_json)
    .execute(pool)
    .await?;
    Ok(())
}

/// DELETE /ota/{version}/config/{device_id}
/// 删除 DB 记录，并清空该设备的专属 retained topic（`cmd_topic/{device_id}`），
/// 避免残留旧专属消息长期留在 broker 上。
pub async fn delete_device_config(
    State(app_state): State<AppState>,
    AxumPath((version, device_id)): AxumPath<(String, String)>,
) -> Result<Response<()>, OtaError> {
    sqlx::query("DELETE FROM device_version_config WHERE version = ? AND device_id = ?")
        .bind(&version)
        .bind(&device_id)
        .execute(&app_state.db)
        .await?;
    // best-effort 清空 retained（失败不阻塞删除）
    let topic = format!("{}/{device_id}", app_state.config.mqtt_conf.cmd_topic);
    let _ = app_state.clear_retained(&topic).await;
    Ok(Response::success(()))
}

/// POST /ota/{version}/publish_device/{device_id}
/// 保存该设备的 config 并立即广播（payload 带 device_id，仅匹配设备应用）。
pub async fn publish_device_config(
    State(app_state): State<AppState>,
    AxumPath((version, device_id)): AxumPath<(String, String)>,
    Json(req): Json<ConfigReq>,
) -> Result<Response<()>, OtaError> {
    save_device_config(&app_state.db, &version, &device_id, &req.config).await?;
    app_state
        .send_fleet_update(&version, Some(&req.config), Some(&device_id))
        .await?;
    Ok(Response::success(()))
}

/// GET /ota/template  → 返回 device_cfg.json 模板（前端「生成设备模板」用）
pub async fn get_config_template() -> Result<Response<serde_json::Value>, OtaError> {
    let v: serde_json::Value = serde_json::from_str(DEVICE_CFG_TEMPLATE)
        .map_err(|e| OtaError::InvalidInput(format!("模板解析失败: {e}")))?;
    Ok(Response::success(v))
}
