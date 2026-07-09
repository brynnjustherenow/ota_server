use std::path::Path;

use axum::{
    Json,
    extract::{Multipart, Path as AxumPath, State},
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

pub async fn ota_update(
    AxumPath(version): AxumPath<String>,
    State(app_state): State<AppState>,
) -> Result<Response<()>, OtaError> {
    let decision = resolve_broadcast(&app_state.db, &version).await;
    if decision.skip {
        return Err(OtaError::InvalidInput(format!(
            "版本 {version} 文件夹不存在/为空 且 无 config，跳过广播"
        )));
    }
    app_state
        .send_fleet_update(&version, decision.config.as_ref())
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

pub async fn latest_published_version() -> Option<String> {
    sorted_versions().await.into_iter().next()
}

#[derive(Deserialize)]
pub struct ConfigReq {
    config: serde_json::Value,
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
}

/// POST /ota/{version}/config  body: {"config": {...}}
pub async fn set_version_config(
    State(app_state): State<AppState>,
    AxumPath(version): AxumPath<String>,
    Json(req): Json<ConfigReq>,
) -> Result<Response<()>, OtaError> {
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
