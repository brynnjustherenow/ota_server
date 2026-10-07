//! 设备登记表：展示名 / 归属组用户 / 上次活动时间。
//!
//! 设备是隐式实体（硬件无注册流程），devices 表只存人工编辑的字段（name/owner）
//! 与活动统计（last_active_at），读时三来源 UNION（devices 表 ∪ device_events ∪
//! video/ 子目录）合并出全量设备；老设备无需迁移回填。
//! 活动时间由 MQTT 消息与视频上传路径触达（touch_device_activity）。

use std::collections::BTreeMap;

use axum::{
    Extension, Json,
    extract::{Path as AxumPath, State},
};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tracing::warn;

use crate::{
    AppState,
    service::{
        OtaError, Response,
        auth::{CurrentUser, ROLE_GROUP_USER},
    },
};

/// GET /devices 返回的设备概览（owner 字段由 LEFT JOIN users 带出）
#[derive(Debug, Serialize, FromRow)]
pub struct DeviceOverview {
    pub device_id: String,
    pub name: String,
    pub owner_user_id: Option<i64>,
    pub owner_nickname: Option<String>,
    pub owner_username: Option<String>,
    /// unix 秒；None = 无任何活动记录
    pub last_active_at: Option<i64>,
}

impl DeviceOverview {
    fn empty(device_id: String) -> Self {
        Self {
            device_id,
            name: String::new(),
            owner_user_id: None,
            owner_nickname: None,
            owner_username: None,
            last_active_at: None,
        }
    }
}

// ─────────────── 活动统计 ───────────────

/// 更新设备上次活动时间（upsert）。
/// device_id 为空/全空白时静默跳过（调用方不必预判活动归属）；
/// 失败仅告警不影响主流程——活动统计是尽力而为的辅助信息。
pub async fn touch_device_activity(db: &SqlitePool, device_id: &str) {
    let did = device_id.trim();
    if did.is_empty() {
        return;
    }
    let now_ts = Utc::now().timestamp();
    let now = Local::now().to_rfc3339();
    let r = sqlx::query(
        "INSERT INTO devices (device_id, last_active_at, created_at, updated_at) VALUES (?, ?, ?, ?) \
         ON CONFLICT(device_id) DO UPDATE \
         SET last_active_at = excluded.last_active_at, updated_at = excluded.updated_at",
    )
    .bind(did)
    .bind(now_ts)
    .bind(&now)
    .bind(&now)
    .execute(db)
    .await;
    if let Err(e) = r {
        warn!("[DEVICE] touch activity failed for {did}: {e}");
    }
}

/// 组用户名下设备 id 集合
pub async fn owned_device_ids(db: &SqlitePool, user_id: i64) -> Result<Vec<String>, OtaError> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT device_id FROM devices WHERE owner_user_id = ?")
            .bind(user_id)
            .fetch_all(db)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// 设备是否归属指定用户
pub async fn is_device_owned_by(
    db: &SqlitePool,
    device_id: &str,
    user_id: i64,
) -> Result<bool, OtaError> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM devices WHERE device_id = ? AND owner_user_id = ?")
            .bind(device_id)
            .bind(user_id)
            .fetch_optional(db)
            .await?;
    Ok(row.is_some())
}

// ─────────────── HTTP handlers ───────────────

/// GET /devices：全量设备概览。
/// 三来源 UNION：devices 表 ∪ device_events 去重 ∪ video/ 子目录；
/// 活动时间兜底：表内无记录时取 device_events 最新 received_at（chrono 解析 RFC3339）。
/// group_user 自动过滤为名下设备；admin+ 看全部。
pub async fn list_devices(
    State(app_state): State<AppState>,
    Extension(current): Extension<CurrentUser>,
) -> Result<Response<Vec<DeviceOverview>>, OtaError> {
    let mut map: BTreeMap<String, DeviceOverview> = BTreeMap::new();

    for row in sqlx::query_as::<_, DeviceOverview>(
        "SELECT d.device_id, d.name, d.owner_user_id, u.nickname AS owner_nickname, \
         u.username AS owner_username, d.last_active_at \
         FROM devices d LEFT JOIN users u ON u.id = d.owner_user_id",
    )
    .fetch_all(&app_state.db)
    .await?
    {
        map.insert(row.device_id.clone(), row);
    }

    // 事件兜底：'unknown' 是 MQTT 消息缺 device_id 时的占位入库，不算真实设备
    let event_rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT device_id, MAX(received_at) FROM device_events \
         WHERE device_id != 'unknown' GROUP BY device_id",
    )
    .fetch_all(&app_state.db)
    .await?;
    for (did, received) in event_rows {
        let ts = DateTime::parse_from_rfc3339(&received)
            .ok()
            .map(|t| t.timestamp());
        match map.get_mut(&did) {
            Some(o) => {
                if o.last_active_at.is_none() {
                    o.last_active_at = ts;
                }
            }
            None => {
                let mut ov = DeviceOverview::empty(did.clone());
                ov.last_active_at = ts;
                map.insert(did, ov);
            }
        }
    }

    for did in video_dir_devices().await? {
        map.entry(did.clone())
            .or_insert_with(|| DeviceOverview::empty(did));
    }
    let mut list: Vec<DeviceOverview> = map.into_values().collect();
    if current.role == ROLE_GROUP_USER {
        list.retain(|o| o.owner_user_id == Some(current.id));
    }
    Ok(Response::success(list))
}

/// video/ 下的子目录名（上传过视频的设备）
async fn video_dir_devices() -> Result<Vec<String>, OtaError> {
    let root = std::path::Path::new("video");
    if !tokio::fs::try_exists(root).await? {
        return Ok(vec![]);
    }
    let mut entries = tokio::fs::read_dir(root).await?;
    let mut devices = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_dir()
            && let Ok(name) = entry.file_name().into_string()
        {
            devices.push(name);
        }
    }
    Ok(devices)
}

#[derive(Debug, Deserialize)]
pub struct UpdateDeviceReq {
    pub name: String,
}

/// PUT /devices/{device_id}  body: {"name": "..."}
/// 编辑展示名（仅前端展示，不同步硬件/MQTT）。
/// admin+ 可改任意设备；group_user 仅可改名下设备。
pub async fn update_device(
    State(app_state): State<AppState>,
    Extension(current): Extension<CurrentUser>,
    AxumPath(device_id): AxumPath<String>,
    Json(req): Json<UpdateDeviceReq>,
) -> Result<Response<DeviceOverview>, OtaError> {
    if !crate::service::video::is_safe_name(&device_id) {
        return Err(OtaError::InvalidParam(format!("非法 device_id: {device_id}")));
    }
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(OtaError::InvalidParam("设备名称不能为空".into()));
    }
    if name.chars().count() > 64 {
        return Err(OtaError::InvalidParam("设备名称最多 64 个字符".into()));
    }
    // group_user 只能动自己名下的设备（未登记设备同样视为无权）
    if current.role == ROLE_GROUP_USER {
        let existing: Option<(Option<i64>,)> =
            sqlx::query_as("SELECT owner_user_id FROM devices WHERE device_id = ?")
                .bind(&device_id)
                .fetch_optional(&app_state.db)
                .await?;
        if !existing.map(|r| r.0 == Some(current.id)).unwrap_or(false) {
            return Err(OtaError::Forbidden("只能编辑名下设备".into()));
        }
    }
    let now = Local::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO devices (device_id, name, created_at, updated_at) VALUES (?, ?, ?, ?) \
         ON CONFLICT(device_id) DO UPDATE SET name = excluded.name, updated_at = excluded.updated_at",
    )
    .bind(&device_id)
    .bind(&name)
    .bind(&now)
    .bind(&now)
    .execute(&app_state.db)
    .await?;
    Ok(Response::success(fetch_overview(&app_state.db, &device_id).await?))
}

#[derive(Debug, Deserialize)]
pub struct UpdateOwnerReq {
    /// 目标组用户 id；null = 解除归属
    pub user_id: Option<i64>,
}

/// PUT /devices/{device_id}/owner  body: {"user_id": 3 | null}
/// 分配/解除归属（仅 admin+；归属对象必须是 group_user 角色的现有用户）
pub async fn update_owner(
    State(app_state): State<AppState>,
    Extension(current): Extension<CurrentUser>,
    AxumPath(device_id): AxumPath<String>,
    Json(req): Json<UpdateOwnerReq>,
) -> Result<Response<DeviceOverview>, OtaError> {
    if !current.is_admin_or_above() {
        return Err(OtaError::Forbidden("仅管理员以上可分配设备归属".into()));
    }
    if !crate::service::video::is_safe_name(&device_id) {
        return Err(OtaError::InvalidParam(format!("非法 device_id: {device_id}")));
    }
    if let Some(uid) = req.user_id {
        let user = crate::service::auth::fetch_user(&app_state.db, uid).await?;
        if user.role != ROLE_GROUP_USER {
            return Err(OtaError::InvalidParam("只能分配给组用户".into()));
        }
    }
    let now = Local::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO devices (device_id, owner_user_id, created_at, updated_at) VALUES (?, ?, ?, ?) \
         ON CONFLICT(device_id) DO UPDATE \
         SET owner_user_id = excluded.owner_user_id, updated_at = excluded.updated_at",
    )
    .bind(&device_id)
    .bind(req.user_id)
    .bind(&now)
    .bind(&now)
    .execute(&app_state.db)
    .await?;
    Ok(Response::success(fetch_overview(&app_state.db, &device_id).await?))
}

async fn fetch_overview(db: &SqlitePool, device_id: &str) -> Result<DeviceOverview, OtaError> {
    sqlx::query_as::<_, DeviceOverview>(
        "SELECT d.device_id, d.name, d.owner_user_id, u.nickname AS owner_nickname, \
         u.username AS owner_username, d.last_active_at \
         FROM devices d LEFT JOIN users u ON u.id = d.owner_user_id WHERE d.device_id = ?",
    )
    .bind(device_id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| OtaError::InvalidParam("设备不存在".into()))
}
