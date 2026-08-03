use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{AppState, service::{OtaError, Response}};

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
pub async fn list_events(
    State(app_state): State<AppState>,
    Query(q): Query<EventQuery>,
) -> Result<Response<Vec<DeviceEvent>>, OtaError> {
    let limit = q.limit.clamp(1, 500);
    let rows = if let Some(did) = &q.device_id {
        sqlx::query_as::<_, DeviceEvent>(
            "SELECT id, device_id, event, detail, ev_ts, received_at \
             FROM device_events WHERE device_id = ? ORDER BY id DESC LIMIT ?",
        )
        .bind(did)
        .bind(limit)
        .fetch_all(&app_state.db)
        .await?
    } else {
        sqlx::query_as::<_, DeviceEvent>(
            "SELECT id, device_id, event, detail, ev_ts, received_at \
             FROM device_events ORDER BY id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&app_state.db)
        .await?
    };
    Ok(Response::success(rows))
}
