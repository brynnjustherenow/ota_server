use std::path::Path;

use axum::{
    extract::{Multipart, Path as AxumPath, State},
};
use futures_util::TryStreamExt;
use md5::{Digest, Md5};
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
    app_state.send_version(version).await?;
    Ok(Response::success(()))
}

/// GET /ota
/// 列出所有已发布的版本号，按 SemVer 倒序（最新在前）
pub async fn list_versions() -> Result<Response<Vec<String>>, OtaError> {
    let root = Path::new("uploads");
    if !fs::try_exists(root).await? {
        return Ok(Response::success(vec![]));
    }
    let mut entries = fs::read_dir(root).await?;
    let mut versions: Vec<String> = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let ft = entry.file_type().await?;
        if ft.is_dir() {
            // 跳过 staging 残留
            if let Ok(name) = entry.file_name().into_string()
                && !name.starts_with('.')
            {
                versions.push(name);
            }
        }
    }
    // 用 version-compare 倒序：b compare a，最新在前；无法解析的版本按字符串回退
    versions.sort_by(|a, b| {
        version_compare::compare(b, a)
            .ok()
            .and_then(|c| c.ord())
            .unwrap_or_else(|| b.cmp(a))
    });
    Ok(Response::success(versions))
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
