use std::{
    cmp::Reverse,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Form, Json,
    body::Body,
    extract::{Multipart, Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response as AxumResponse},
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

use crate::{
    AppState,
    service::{OtaError, Response},
};

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

// ===================== 三段式分片上传（init / chunk / complete / clean）=====================
// 适配新协议（rat.caasai.com 风格）：
//   - 字段名 camelCase（devSerial / fileSize / uploadId / chunkSize）
//   - 响应统一 {code, msg, status, data}，HTTP 永远 200
//   - chunk 用 multipart/form-data（field name = "chunk"）
//   - complete 用 form-urlencoded（uploadId）
//   - 新增 /clean 接口清空上传记录
//   - chunk 响应 data 是裸整数 offset（与原 Java 服务端一致）

// ---------- 统一响应包装 ----------

#[derive(Serialize)]
pub struct VideoResp<T: Serialize> {
    code: i16,
    msg: String,
    status: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
}

impl<T: Serialize> VideoResp<T> {
    fn success(data: T) -> Self {
        Self {
            code: 200,
            msg: "success".to_string(),
            status: true,
            data: Some(data),
        }
    }
}

/// 业务错误：HTTP 仍返回 200，业务错误用 body.code != 200 && status = false 表示。
/// 这样客户端无论何种失败都能解析响应体，统一处理。
pub enum VideoError {
    Bad(String),
    Internal(String),
}

impl From<std::io::Error> for VideoError {
    fn from(e: std::io::Error) -> Self {
        VideoError::Internal(format!("io: {e}"))
    }
}

impl IntoResponse for VideoError {
    fn into_response(self) -> AxumResponse {
        let (code, msg) = match self {
            VideoError::Bad(m) => (400i16, m),
            VideoError::Internal(m) => (500, m),
        };
        let body = serde_json::json!({
            "code": code,
            "msg": msg,
            "status": false,
            "data": serde_json::Value::Null,
        });
        // 关键：HTTP 永远 200，业务错误看 body.code
        (StatusCode::OK, Json(body)).into_response()
    }
}

// ---------- 生成 uploadId ----------

static UPLOAD_COUNTER: AtomicU64 = AtomicU64::new(0);

fn gen_upload_id() -> String {
    // 与原 Java 服务端格式一致： yyyyMMddHHmmss + 6 位计数器，便于日志对齐
    let ts = Local::now().format("%Y%m%d%H%M%S").to_string();
    let c = UPLOAD_COUNTER.fetch_add(1, Ordering::Relaxed) % 1_000_000;
    format!("{ts}{c:06}")
}

/// 8 位随机十六进制，用于 fileUrl 防重
fn rand_hex8() -> String {
    let n = UPLOAD_COUNTER.fetch_add(7, Ordering::Relaxed);
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{:08x}", (n ^ t) & 0xFFFF_FFFF)
}

pub struct UploadSession {
    device_id: String,
    filename: String,
    file_size: u64,
    chunk_size: u64,
    temp_path: PathBuf,
    received: u64,
    created_at: Instant,
}

// ---------- init ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadInitReq {
    pub upload_id: Option<String>,
    pub dev_serial: String,
    pub file_size: f64,
    pub filename: String,
    pub chunk_size: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitData {
    upload_id: String,
    filename: Option<String>,
    directory: Option<String>,
    dev_serial: Option<String>,
    offset: u64,
    chunk_size: u64,
    total_parts: Option<u64>,
    file_size: Option<u64>,
    ok: Option<bool>,
    file_url: Option<String>,
}

/// POST /{base}/{biz}/init
/// 新建或续传上传会话。响应 data: {uploadId, offset, chunkSize, ...}
pub async fn upload_init(
    State(app_state): State<AppState>,
    Json(req): Json<UploadInitReq>,
) -> Result<Json<VideoResp<InitData>>, VideoError> {
    if !is_safe_name(&req.dev_serial) {
        return Err(VideoError::Bad(format!(
            "非法 devSerial: {}",
            req.dev_serial
        )));
    }
    if !is_safe_name(&req.filename) {
        return Err(VideoError::Bad(format!("非法 filename: {}", req.filename)));
    }

    let file_size = req.file_size as u64;
    let chunk_size = req.chunk_size.map(|v| v as u64).unwrap_or(5 * 1024 * 1024);

    let dir = Path::new("video").join(&req.dev_serial);
    fs::create_dir_all(&dir).await?;

    let mut uploads = app_state.uploads.lock().await;

    // 清理 30 分钟以上的孤儿 session
    let stale_cutoff = Instant::now() - Duration::from_secs(1800);
    uploads.retain(|_, sess| sess.created_at >= stale_cutoff);

    // 续传：客户端带上已存在的 upload_id
    if let Some(uid) = req
        .upload_id
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        if let Some(sess) = uploads.get(uid) {
            return Ok(Json(VideoResp::success(InitData {
                upload_id: uid.to_string(),
                filename: Some(sess.filename.clone()),
                directory: None,
                dev_serial: Some(sess.device_id.clone()),
                offset: sess.received,
                chunk_size: sess.chunk_size,
                total_parts: None,
                file_size: Some(sess.file_size),
                ok: None,
                file_url: None,
            })));
        }
    }

    let upload_id = gen_upload_id();
    let temp_path = dir.join(format!(".{upload_id}.partial"));
    fs::File::create(&temp_path).await?;
    uploads.insert(
        upload_id.clone(),
        UploadSession {
            device_id: req.dev_serial.clone(),
            filename: req.filename.clone(),
            file_size,
            chunk_size,
            temp_path,
            received: 0,
            created_at: Instant::now(),
        },
    );

    Ok(Json(VideoResp::success(InitData {
        upload_id,
        filename: Some(req.filename),
        directory: None,
        dev_serial: Some(req.dev_serial),
        offset: 0,
        chunk_size,
        total_parts: None,
        file_size: Some(file_size),
        ok: None,
        file_url: None,
    })))
}

// ---------- chunk ----------

/// PUT /{base}/{biz}/chunk
/// multipart/form-data，字段名 chunk。Header: X-Offset, X-Upload-Id [, X-part-number]
/// 响应 data: 裸整数 offset（与原 Java 服务端行为一致）
pub async fn upload_chunk(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<VideoResp<u64>>, VideoError> {
    let upload_id = headers
        .get("x-upload-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| VideoError::Bad("缺少 X-Upload-Id".to_string()))?
        .to_string();
    let x_offset = headers
        .get("x-offset")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| VideoError::Bad("缺少/非法 X-Offset".to_string()))?;

    // ① 短暂持锁：校验 offset，取 temp_path，立即释放
    let temp_path = {
        let uploads = app_state.uploads.lock().await;
        let sess = uploads
            .get(&upload_id)
            .ok_or_else(|| VideoError::Bad(format!("未知 uploadId: {upload_id}")))?;
        if x_offset != sess.received {
            return Err(VideoError::Bad(format!(
                "offset 不匹配: 客户端={x_offset} 服务端={}",
                sess.received
            )));
        }
        sess.temp_path.clone()
    };

    // ② 无锁：从 multipart 里读 chunk field 写文件
    let mut received = x_offset;
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .append(true)
            .open(&temp_path)
            .await?;
        let mut found_chunk = false;
        while let Some(field) = multipart
            .next_field()
            .await
            .map_err(|e| VideoError::Bad(format!("multipart 解析失败: {e}")))?
        {
            let name = field.name().unwrap_or("").to_string();
            if name != "chunk" {
                continue;
            }
            found_chunk = true;
            let mut field = field;
            loop {
                // field.chunk() 返回 Result<Option<Bytes>, MultipartError>
                // None 表示该 field 已读完；timeout 包一层后是 Result<Result<Option<Bytes>, _>, Elapsed>
                let chunk_result =
                    tokio::time::timeout(Duration::from_secs(60), field.chunk()).await;
                let chunk_opt = match chunk_result {
                    Err(_) => return Err(VideoError::Bad("读取分片超时(60s)".to_string())),
                    Ok(Err(e)) => {
                        return Err(VideoError::Bad(format!("chunk field 错误: {e}")))
                    }
                    Ok(Ok(opt)) => opt,
                };
                let chunk_bytes = match chunk_opt {
                    None => break,
                    Some(b) => b,
                };
                file.write_all(&chunk_bytes).await?;
                received += chunk_bytes.len() as u64;
            }
        }
        if !found_chunk {
            return Err(VideoError::Bad(
                "multipart body 缺少 chunk 字段".to_string(),
            ));
        }
        file.flush().await?;
    }

    // ③ 短暂持锁：回写 received
    {
        let mut uploads = app_state.uploads.lock().await;
        if let Some(sess) = uploads.get_mut(&upload_id) {
            sess.received = received;
        }
    }

    Ok(Json(VideoResp::success(received)))
}

// ---------- complete ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadCompleteReq {
    pub upload_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteData {
    ok: bool,
    file_url: String,
    upload_id: String,
}

/// POST /{base}/{biz}/complete
/// form-urlencoded: uploadId=...
/// 响应 data: {ok: true, fileUrl: "videos/<devSerial>/<filename>", uploadId: "..."}
pub async fn upload_complete(
    State(app_state): State<AppState>,
    Form(req): Form<UploadCompleteReq>,
) -> Result<Json<VideoResp<CompleteData>>, VideoError> {
    complete_impl(app_state, req.upload_id).await
}

async fn complete_impl(
    app_state: AppState,
    upload_id: String,
) -> Result<Json<VideoResp<CompleteData>>, VideoError> {
    let sess = {
        let mut uploads = app_state.uploads.lock().await;
        uploads
            .remove(&upload_id)
            .ok_or_else(|| VideoError::Bad(format!("未知 uploadId: {upload_id}")))?
    };

    let dir = Path::new("video").join(&sess.device_id);
    let final_path = dir.join(&sess.filename);
    // fileUrl 是相对路径，对外暴露（与原 Java 服务端格式一致）
    let file_url = format!("videos/{}/{}", sess.device_id, sess.filename);

    let result: Result<(), VideoError> = async {
        if sess.received != sess.file_size {
            return Err(VideoError::Bad(format!(
                "大小不符: 已收={} 声明={}",
                sess.received, sess.file_size
            )));
        }
        // 原子提交：删旧 → rename
        if fs::try_exists(&final_path).await? {
            fs::remove_file(&final_path).await?;
        }
        fs::rename(&sess.temp_path, &final_path).await?;
        Ok(())
    }
    .await;

    match result {
        Ok(()) => {
            if let Err(e) = app_state
                .send_video_event(&sess.device_id, &sess.filename)
                .await
            {
                tracing::warn!(
                    error = %e, %sess.device_id, %sess.filename,
                    "publish video_uploaded event failed"
                );
            }
            Ok(Json(VideoResp::success(CompleteData {
                ok: true,
                file_url,
                upload_id,
            })))
        }
        Err(e) => {
            let _ = fs::remove_file(&sess.temp_path).await;
            Err(e)
        }
    }
}

// ---------- clean ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadCleanReq {
    pub upload_id: String,
}

/// POST /{base}/{biz}/clean
/// 清空指定 uploadId 的上传记录（删除 .partial 临时文件 + 内存 session）。
/// 客户端上传异常时主动调 clean，避免下次 init 复用错 offset。
/// 响应：{code, msg, status}，无 data
pub async fn upload_clean(
    State(app_state): State<AppState>,
    Form(req): Form<UploadCleanReq>,
) -> Result<Json<VideoResp<()>>, VideoError> {
    clean_impl(app_state, req.upload_id).await
}

async fn clean_impl(
    app_state: AppState,
    upload_id: String,
) -> Result<Json<VideoResp<()>>, VideoError> {
    let sess = {
        let mut uploads = app_state.uploads.lock().await;
        uploads.remove(&upload_id)
    };
    if let Some(sess) = sess {
        let _ = fs::remove_file(&sess.temp_path).await;
    }
    // 无论 uploadId 是否存在都返回成功（幂等）
    Ok(Json(VideoResp {
        code: 200,
        msg: "操作成功".to_string(),
        status: true,
        data: None,
    }))
}
