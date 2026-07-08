use std::{
    cmp::Reverse,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::HeaderMap,
    response::Response as AxumResponse,
};
use chrono::Local;
use futures_util::TryStreamExt;
use md5::{Digest, Md5};
use serde::Serialize;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};
use tokio_util::io::StreamReader;

use crate::{AppState, service::{OtaError, Response}};

#[derive(Serialize)]
pub struct VideoInfo {
    device_id: String,
    filename: String,
    size: u64,
    md5: String,
}

#[derive(Serialize)]
pub struct VideoMeta {
    filename: String,
    size: u64,
    modified_at: u64,
}

fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
}

/// POST /video/{device_id}
/// 设备用 raw body 上传视频。文件名取自 Content-Disposition，缺省按时间戳生成。
/// 写入 video/{device_id}/{filename}，staging -> rename 原子提交。
/// 提交成功后向 status_topic 推送 video_uploaded 事件（失败仅告警，不影响上传结果）。
pub async fn upload_video(
    State(app_state): State<AppState>,
    AxumPath(device_id): AxumPath<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response<VideoInfo>, OtaError> {
    if !is_safe_name(&device_id) {
        return Err(OtaError::InvalidInput(format!("非法 device_id: {device_id}")));
    }

    let filename = parse_filename(&headers)
        .unwrap_or_else(|| format!("{}.mp4", Local::now().format("%Y%m%d_%H%M%S")));
    if !is_safe_name(&filename) {
        return Err(OtaError::InvalidInput(format!("非法 filename: {filename}")));
    }

    let dir = Path::new("video").join(&device_id);
    let staging = dir.join(format!(".{filename}.staging"));
    let final_path = dir.join(&filename);

    let result: Result<(u64, String), OtaError> = async {
        fs::create_dir_all(&dir).await?;

        let (size, md5_hex) = {
            let mut reader =
                StreamReader::new(body.into_data_stream().map_err(std::io::Error::other));
            let mut file = fs::File::create(&staging).await?;
            let mut hasher = Md5::new();
            let mut total: u64 = 0;
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                let n = reader.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                file.write_all(&buf[..n]).await?;
                hasher.update(&buf[..n]);
                total += n as u64;
            }
            file.flush().await?;
            (total, format!("{:x}", hasher.finalize()))
        };

        // 提交：若同名旧文件存在则删除，再原子 rename
        if fs::try_exists(&final_path).await? {
            fs::remove_file(&final_path).await?;
        }
        fs::rename(&staging, &final_path).await?;
        Ok((size, md5_hex))
    }
    .await;

    let (size, md5) = match result {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_file(&staging).await;
            return Err(e);
        }
    };

    // 视频已落盘，MQTT 通知失败不应回滚上传
    if let Err(e) = app_state.send_video_event(&device_id, &filename).await {
        tracing::warn!(error = %e, %device_id, %filename, "publish video_uploaded event failed");
    }

    Ok(Response::success(VideoInfo {
        device_id,
        filename,
        size,
        md5,
    }))
}

/// GET /video
/// 列出所有上传过视频的 device_id（video/ 下的子目录）
pub async fn list_devices() -> Result<Response<Vec<String>>, OtaError> {
    let root = Path::new("video");
    if !fs::try_exists(root).await? {
        return Ok(Response::success(vec![]));
    }
    let mut entries = fs::read_dir(root).await?;
    let mut devices = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let ft = entry.file_type().await?;
        if ft.is_dir() {
            if let Ok(name) = entry.file_name().into_string() {
                devices.push(name);
            }
        }
    }
    devices.sort();
    Ok(Response::success(devices))
}

/// GET /video/{device_id}
/// 列出该设备的所有视频，按修改时间倒序（最新在前）；目录不存在返回空数组
pub async fn list_videos(
    AxumPath(device_id): AxumPath<String>,
) -> Result<Response<Vec<VideoMeta>>, OtaError> {
    if !is_safe_name(&device_id) {
        return Err(OtaError::InvalidInput(format!("非法 device_id: {device_id}")));
    }

    let dir = Path::new("video").join(&device_id);
    if !fs::try_exists(&dir).await? {
        return Ok(Response::success(vec![]));
    }

    let mut entries = fs::read_dir(&dir).await?;
    let mut items: Vec<VideoMeta> = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        // 跳过 staging 残留
        if name.starts_with('.') {
            continue;
        }
        let meta = entry.metadata().await?;
        // 只列普通文件，跳过子目录
        if !meta.is_file() {
            continue;
        }
        let modified_at = meta
            .modified()
            .map(system_time_to_ms)
            .unwrap_or(0);
        items.push(VideoMeta {
            filename: name,
            size: meta.len(),
            modified_at,
        });
    }
    // 新文件在前
    items.sort_by_key(|v| Reverse(v.modified_at));
    Ok(Response::success(items))
}

/// GET /video/{device_id}/{filename}
/// 流式下载指定视频，支持 HTTP Range（用于 `<video>` 拖动进度条）
pub async fn download_video(
    AxumPath((device_id, filename)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Result<AxumResponse, OtaError> {
    if !is_safe_name(&device_id) {
        return Err(OtaError::InvalidInput(format!("非法 device_id: {device_id}")));
    }
    if !is_safe_name(&filename) {
        return Err(OtaError::InvalidInput(format!("非法 filename: {filename}")));
    }

    let path = Path::new("video").join(&device_id).join(&filename);
    let range = headers
        .get("range")
        .and_then(|v| v.to_str().ok());
    let content_type = crate::service::util::guess_video_mime(&filename);
    crate::service::util::serve_file(
        &path,
        &filename,
        content_type,
        range,
        None,
        format!("视频 {device_id}/{filename} 不存在"),
    )
    .await
}

/// 从 `Content-Disposition: attachment; filename="xxx"` 解析文件名
fn parse_filename(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get("content-disposition")?.to_str().ok()?;
    for part in raw.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("filename=") {
            return Some(rest.trim_matches('"').to_string());
        }
    }
    None
}

fn system_time_to_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
