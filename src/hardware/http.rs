use std::path::Path;

use axum::{
    Json,
    extract::Path as AxumPath,
    http::{HeaderMap, StatusCode},
    response::Response as AxumResponse,
};
use tokio::fs;

use crate::{
    hardware::mqtt::{OtaMetadata, split_rel_path},
    service::{OtaError, util::serve_file},
};

/// GET /ota/{version}/manifest
/// 返回该版本的清单（文件列表 + MD5 + size + path），供设备决定要下载哪些文件
pub async fn get_ota_files(
    AxumPath(version): AxumPath<String>,
) -> Result<Json<OtaMetadata>, OtaError> {
    let manifest = Path::new("uploads").join(&version).join("manifest.json");
    if !fs::try_exists(&manifest).await? {
        return Err(OtaError::FileNotFound(format!("版本 {version} 不存在")));
    }
    let content = fs::read_to_string(&manifest).await?;
    let metadata: OtaMetadata = serde_json::from_str(&content)
        .map_err(|e| OtaError::InvalidInput(format!("manifest 解析失败: {e}")))?;
    Ok(Json(metadata))
}

/// GET /ota/{version}/files/{*relpath}
/// 流式下载文件，支持 HTTP Range 与 ETag/If-None-Match 缓存校验。
pub async fn download_ota_file(
    AxumPath((version, relpath)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Result<AxumResponse, OtaError> {
    // 防路径穿越
    if relpath.contains("..") || relpath.contains('\\') || relpath.starts_with('/') {
        return Err(OtaError::InvalidInput(format!("非法文件路径: {relpath}")));
    }

    // 查 manifest 获取 md5；找不到则 lenient 处理（仍允许下载，只是不发 ETag）
    let etag: Option<String> = load_manifest(&version)
        .await
        .ok()
        .and_then(|m| m.find_file(&relpath).map(|f| format!("\"{}\"", f.md5())));

    // If-None-Match 命中则返回 304（Range 请求与 304 互斥）
    if let (Some(etag), Some(inm)) = (etag.as_ref(), headers.get("if-none-match"))
        && inm.to_str().map(|s| s == etag.as_str()).unwrap_or(false)
    {
        return Ok(not_modified(etag.clone()));
    }

    let path = Path::new("uploads").join(&version).join(&relpath);
    let (_, basename) = split_rel_path(&relpath);
    let range = headers.get("range").and_then(|v| v.to_str().ok());
    serve_file(
        &path,
        &basename,
        "application/octet-stream",
        range,
        etag.as_deref(),
        format!("文件 {version}/{relpath} 不存在"),
    )
    .await
}

async fn load_manifest(version: &str) -> Result<OtaMetadata, OtaError> {
    let path = Path::new("uploads").join(version).join("manifest.json");
    if !fs::try_exists(&path).await? {
        return Err(OtaError::FileNotFound(format!("版本 {version} 不存在")));
    }
    let content = fs::read_to_string(&path).await?;
    serde_json::from_str(&content)
        .map_err(|e| OtaError::InvalidInput(format!("manifest 解析失败: {e}")))
}

fn not_modified(etag: String) -> AxumResponse {
    AxumResponse::builder()
        .status(StatusCode::NOT_MODIFIED)
        .header("etag", etag)
        .body(axum::body::Body::empty())
        .expect("build 304 response failed")
}
