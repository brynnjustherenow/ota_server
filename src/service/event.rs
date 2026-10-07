use axum::{
    Extension,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{
    AppState,
    service::{
        OtaError, Response,
        auth::{CurrentUser, ROLE_GROUP_USER},
        device,
    },
};

#[derive(Debug, Serialize, FromRow)]
pub struct DeviceEvent {
    pub id: i64,
    pub device_id: String,
    pub event: String,
    pub detail: String,
    pub ev_ts: i64,
    pub received_at: String,
}

#[derive(Debug, Deserialize)]
pub struct EventQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub device_id: Option<String>,
}

fn default_limit() -> i64 {
    50
}

/// GET /events?limit=50&device_id=CAM001
/// 列出设备生命周期事件（BOOT/RESET/APP_START/NEXT_CYCLE），按 id 倒序。
/// 组用户只能看到名下设备的事件。
pub async fn list_events(
    State(app_state): State<AppState>,
    Extension(current): Extension<CurrentUser>,
    Query(q): Query<EventQuery>,
) -> Result<Response<Vec<DeviceEvent>>, OtaError> {
    let limit = q.limit.clamp(1, 500);
    // 组用户：先按名下设备过滤（指定了 device_id 时校验归属，非名下返回空列表避免泄露存在性）
    let group_scoped = current.role == ROLE_GROUP_USER;
    if group_scoped
        && let Some(did) = &q.device_id
        && !device::is_device_owned_by(&app_state.db, did, current.id).await?
    {
        return Ok(Response::success(vec![]));
    }
    let rows = match (&q.device_id, group_scoped) {
        (Some(did), false) => {
            sqlx::query_as::<_, DeviceEvent>(
                "SELECT id, device_id, event, detail, ev_ts, received_at \
                 FROM device_events WHERE device_id = ? ORDER BY id DESC LIMIT ?",
            )
            .bind(did)
            .bind(limit)
            .fetch_all(&app_state.db)
            .await?
        }
        (None, false) => {
            sqlx::query_as::<_, DeviceEvent>(
                "SELECT id, device_id, event, detail, ev_ts, received_at \
                 FROM device_events ORDER BY id DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all(&app_state.db)
            .await?
        }
        // 组用户无 device_id 过滤：IN 子查询限定名下设备
        (None, true) => {
            sqlx::query_as::<_, DeviceEvent>(
                "SELECT id, device_id, event, detail, ev_ts, received_at \
                 FROM device_events WHERE device_id IN \
                 (SELECT device_id FROM devices WHERE owner_user_id = ?) \
                 ORDER BY id DESC LIMIT ?",
            )
            .bind(current.id)
            .bind(limit)
            .fetch_all(&app_state.db)
            .await?
        }
        // 组用户 + 指定名下 device_id（归属已在上方校验通过）
        (Some(did), true) => {
            sqlx::query_as::<_, DeviceEvent>(
                "SELECT id, device_id, event, detail, ev_ts, received_at \
                 FROM device_events WHERE device_id = ? ORDER BY id DESC LIMIT ?",
            )
            .bind(did)
            .bind(limit)
            .fetch_all(&app_state.db)
            .await?
        }
    };
    Ok(Response::success(rows))
}
