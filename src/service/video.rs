use std::{
    cmp::Reverse,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    body::Body,
    extract::{Path as AxumPath, State},
    http::HeaderMap,
    response::Response as AxumResponse,
};
use chrono::Local;
use futures_util::TryStreamExt;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
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

// ===================== 三段式分片上传（init / chunk / complete）=====================
// 设备端 upload_client.py 期望扁平 JSON 响应（{upload_id, offset} 顶层），
// 故这三个端点直返 Json<T>，不套 Response<T>；错误仍走 OtaError（{code,message,data:null}）。

static UPLOAD_COUNTER: AtomicU64 = AtomicU64::new(0);

fn gen_upload_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let c = UPLOAD_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("up_{ts}_{c}")
}

pub struct UploadSession {
    device_id: String,
    filename: String,
    file_size: u64,
    temp_path: PathBuf,
    received: u64,
}

#[derive(Deserialize)]
pub struct UploadInitReq {
    pub device_id: String,
    pub filename: String,
    pub file_size: u64,
    pub upload_id: Option<String>,
}

#[derive(Serialize)]
pub struct UploadInitResp {
    pub upload_id: String,
    pub offset: u64,
}

#[derive(Serialize)]
pub struct UploadChunkResp {
    pub offset: u64,
}

#[derive(Deserialize)]
pub struct UploadCompleteReq {
    pub upload_id: String,
}

#[derive(Serialize)]
pub struct UploadCompleteResp {
    pub ok: bool,
    pub md5: Option<String>,
}

/// POST /upload/init
/// 新建或续传上传会话。返回 upload_id 与起始 offset（0 表新传，>0 表续传）。
/// 响应扁平：{"upload_id": "...", "offset": N}
pub async fn upload_init(
    State(app_state): State<AppState>,
    Json(req): Json<UploadInitReq>,
) -> Result<Json<UploadInitResp>, OtaError> {
    if !is_safe_name(&req.device_id) {
        return Err(OtaError::InvalidInput(format!("非法 device_id: {}", req.device_id)));
    }
    if !is_safe_name(&req.filename) {
        return Err(OtaError::InvalidInput(format!("非法 filename: {}", req.filename)));
    }

    let dir = Path::new("video").join(&req.device_id);
    fs::create_dir_all(&dir).await?;

    let mut uploads = app_state.uploads.lock().await;

    // 续传：客户端带上已存在的 upload_id
    if let Some(uid) = req.upload_id.as_deref().filter(|s| !s.is_empty()) {
        if let Some(sess) = uploads.get(uid) {
            return Ok(Json(UploadInitResp {
                upload_id: uid.to_string(),
                offset: sess.received,
            }));
        }
    }

    let upload_id = gen_upload_id();
    let temp_path = dir.join(format!(".{upload_id}.partial"));
    fs::File::create(&temp_path).await?; // 创建空文件，覆盖可能残留
    uploads.insert(
        upload_id.clone(),
        UploadSession {
            device_id: req.device_id,
            filename: req.filename,
            file_size: req.file_size,
            temp_path,
            received: 0,
        },
    );
    Ok(Json(UploadInitResp {
        upload_id,
        offset: 0,
    }))
}

/// PUT /upload/chunk
/// 追加写入分片。X-Offset 必须等于服务端已收字节数，否则拒绝。
/// 响应扁平：{"offset": N}
pub async fn upload_chunk(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<UploadChunkResp>, OtaError> {
    let upload_id = headers
        .get("x-upload-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| OtaError::InvalidInput("缺少 X-Upload-Id".to_string()))?
        .to_string();
    let x_offset = headers
        .get("x-offset")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| OtaError::InvalidInput("缺少/非法 X-Offset".to_string()))?;

    let mut uploads = app_state.uploads.lock().await;
    let sess = uploads
        .get_mut(&upload_id)
        .ok_or_else(|| OtaError::InvalidInput(format!("未知 upload_id: {upload_id}")))?;

    if x_offset != sess.received {
        return Err(OtaError::InvalidInput(format!(
            "offset 不匹配: 客户端={x_offset} 服务端={}",
            sess.received
        )));
    }

    let mut file = fs::OpenOptions::new()
        .write(true)
        .append(true)
        .open(&sess.temp_path)
        .await?;
    let mut reader = StreamReader::new(body.into_data_stream().map_err(std::io::Error::other));
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).await?;
        sess.received += n as u64;
    }
    file.flush().await?;

    Ok(Json(UploadChunkResp {
        offset: sess.received,
    }))
}

/// POST /upload/complete
/// 完成上传：校验大小、计算 MD5、原子 rename、广播 MQTT 事件。
/// 响应扁平：{"ok": true, "md5": "..."}
pub async fn upload_complete(
    State(app_state): State<AppState>,
    Json(req): Json<UploadCompleteReq>,
) -> Result<Json<UploadCompleteResp>, OtaError> {
    let sess = {
        let mut uploads = app_state.uploads.lock().await;
        uploads
            .remove(&req.upload_id)
            .ok_or_else(|| OtaError::InvalidInput(format!("未知 upload_id: {}", req.upload_id)))?
    };

    let dir = Path::new("video").join(&sess.device_id);
    let final_path = dir.join(&sess.filename);

    let result: Result<String, OtaError> = async {
        if sess.received != sess.file_size {
            return Err(OtaError::InvalidInput(format!(
                "大小不符: 已收={} 声明={}",
                sess.received, sess.file_size
            )));
        }
        // 计算 MD5（读一遍 partial）
        let mut f = fs::File::open(&sess.temp_path).await?;
        let mut hasher = Md5::new();
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let n = f.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let md5_hex = format!("{:x}", hasher.finalize());

        // 原子提交：删旧 → rename
        if fs::try_exists(&final_path).await? {
            fs::remove_file(&final_path).await?;
        }
        fs::rename(&sess.temp_path, &final_path).await?;
        Ok(md5_hex)
    }
    .await;

    match result {
        Ok(md5_hex) => {
            if let Err(e) = app_state.send_video_event(&sess.device_id, &sess.filename).await {
                tracing::warn!(error = %e, %sess.device_id, %sess.filename, "publish video_uploaded event failed");
            }
            Ok(Json(UploadCompleteResp {
                ok: true,
                md5: Some(md5_hex),
            }))
        }
        Err(e) => {
            let _ = fs::remove_file(&sess.temp_path).await;
            Err(e)
        }
    }
}
