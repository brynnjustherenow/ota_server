use std::path::Path;

use axum::{
    body::Body,
    http::StatusCode,
    response::Response as AxumResponse,
};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, SeekFrom},
};
use tokio_util::io::ReaderStream;

use crate::service::OtaError;

/// 解析后的字节范围
struct RangeSpec {
    start: u64,
    end: u64, // inclusive
    total: u64,
}

/// 解析 `Range: bytes=...` 头。支持三种形式：
/// - `bytes=START-END`
/// - `bytes=START-`（START 到末尾）
/// - `bytes=-N`（最后 N 字节）
///
/// 无法解析或超出 total 范围返回 None（调用方应回退为 200 全量返回）
fn parse_range(header: &str, total: u64) -> Option<RangeSpec> {
    let h = header.trim();
    let h = h.strip_prefix("bytes=")?;
    let (start_s, end_s) = h.split_once('-')?;

    let (start, end) = if start_s.is_empty() {
        // bytes=-N：最后 N 字节
        let n: u64 = end_s.parse().ok()?;
        if n == 0 || total == 0 {
            return None;
        }
        let start = total.checked_sub(n)?;
        (start, total - 1)
    } else {
        let start: u64 = start_s.parse().ok()?;
        if start >= total {
            return None;
        }
        let end = if end_s.is_empty() {
            total - 1
        } else {
            let end: u64 = end_s.parse().ok()?;
            if end < start {
                return None;
            }
            end.min(total - 1)
        };
        (start, end)
    };

    Some(RangeSpec { start, end, total })
}

/// 根据扩展名猜视频 MIME，浏览器原生 `<video>` 才能正确选 codec
pub fn guess_video_mime(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    if lower.ends_with(".mp4") || lower.ends_with(".m4v") {
        "video/mp4"
    } else if lower.ends_with(".webm") {
        "video/webm"
    } else if lower.ends_with(".mov") {
        "video/quicktime"
    } else if lower.ends_with(".mkv") {
        "video/x-matroska"
    } else if lower.ends_with(".ogv") {
        "video/ogg"
    } else {
        "application/octet-stream"
    }
}

/// 通用文件流式响应，支持 HTTP Range（206 Partial Content）。
///
/// - `range_header`：请求头 `Range` 的值（已是 str），缺省则全量返回 200
/// - `etag`：若附带，会在响应头加上（用于缓存校验）
/// - `not_found_msg`：文件不存在时的错误消息
pub async fn serve_file(
    path: &Path,
    filename: &str,
    content_type: &str,
    range_header: Option<&str>,
    etag: Option<&str>,
    not_found_msg: impl Into<String>,
) -> Result<AxumResponse, OtaError> {
    let file = fs::File::open(path)
        .await
        .map_err(|_| OtaError::FileNotFound(not_found_msg.into()))?;
    let total = file.metadata().await?.len();

    let mut builder = AxumResponse::builder()
        .header("accept-ranges", "bytes")
        .header("content-type", content_type)
        .header(
            "content-disposition",
            format!("attachment; filename=\"{filename}\""),
        );
    if let Some(e) = etag {
        builder = builder.header("etag", e);
    }

    // 有 Range 头且能解析 → 206
    if total > 0
        && let Some(r) = range_header.and_then(|h| parse_range(h, total))
    {
        let len = r.end - r.start + 1;
        let mut file = file;
        file.seek(SeekFrom::Start(r.start)).await?;
        let body = Body::from_stream(ReaderStream::new(file.take(len)));
        return Ok(builder
            .status(StatusCode::PARTIAL_CONTENT)
            .header("content-range", format!("bytes {}-{}/{}", r.start, r.end, r.total))
            .header("content-length", len.to_string())
            .body(body)
            .expect("build 206 response"));
    }

    // 否则全量 200
    let body = Body::from_stream(ReaderStream::new(file));
    Ok(builder
        .status(StatusCode::OK)
        .header("content-length", total.to_string())
        .body(body)
        .expect("build 200 response"))
}
