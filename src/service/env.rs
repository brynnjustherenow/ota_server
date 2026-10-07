//! 设备环境数据(env)持久化与查询。
//!
//! MQTT status topic 里带 env 字段的消息原样入库(payload 存原始 JSON,
//! 字段结构由硬件决定,服务端不预设 schema);ts 为 unix 秒,便于区间查询。
//! 组用户只能查询名下设备的数据(与 events/video 同一套归属过滤)。

use axum::{
    Extension,
    extract::{Query, State},
};
use chrono::Local;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::{
    AppState,
    service::{
        OtaError, Response,
        auth::{CurrentUser, ROLE_GROUP_USER},
        device::is_device_owned_by,
    },
};

/// 入库(硬件事件钩子调用;payload 原样存储)
pub async fn insert_env(
    db: &SqlitePool,
    device_id: &str,
    ts: i64,
    payload: &serde_json::Value,
) -> Result<(), OtaError> {
    let received_at = Local::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO device_env (device_id, ts, payload, received_at) VALUES (?, ?, ?, ?)",
    )
    .bind(device_id)
    .bind(ts)
    .bind(payload.to_string())
    .bind(&received_at)
    .execute(db)
    .await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct EnvQuery {
    pub device_id: Option<String>,
    /// unix 秒,含端点
    pub start: Option<i64>,
    pub end: Option<i64>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    200
}

/// DB 原始行(payload 为 JSON 文本)
#[derive(Debug, FromRow)]
struct EnvRaw {
    id: i64,
    device_id: String,
    ts: i64,
    payload: String,
    received_at: String,
}

/// 对外行(payload 已解析为 JSON 对象)
#[derive(Debug, Serialize)]
pub struct EnvItem {
    id: i64,
    device_id: String,
    ts: i64,
    payload: serde_json::Value,
    received_at: String,
}

/// GET /env?device_id=&start=&end=&limit=
/// 按设备/时间区间查询,ts 倒序;组用户自动限定名下设备。
pub async fn list_env(
    State(app_state): State<AppState>,
    Extension(current): Extension<CurrentUser>,
    Query(q): Query<EnvQuery>,
) -> Result<Response<Vec<EnvItem>>, OtaError> {
    let limit = q.limit.clamp(1, 2000);
    let group_scoped = current.role == ROLE_GROUP_USER;
    // 组用户指定了设备:校验归属,非名下直接返回空(不泄露存在性)
    if group_scoped
        && let Some(did) = &q.device_id
        && !is_device_owned_by(&app_state.db, did, current.id).await?
    {
        return Ok(Response::success(vec![]));
    }

    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT id, device_id, ts, payload, received_at FROM device_env WHERE 1=1",
    );
    if let Some(did) = &q.device_id {
        qb.push(" AND device_id = ").push_bind(did);
    }
    if let Some(start) = q.start {
        qb.push(" AND ts >= ").push_bind(start);
    }
    if let Some(end) = q.end {
        qb.push(" AND ts <= ").push_bind(end);
    }
    // 组用户未指定设备:IN 子查询限定名下
    if group_scoped && q.device_id.is_none() {
        qb.push(" AND device_id IN (SELECT device_id FROM devices WHERE owner_user_id = ")
            .push_bind(current.id)
            .push(")");
    }
    qb.push(" ORDER BY ts DESC, id DESC LIMIT ").push_bind(limit);

    let raw: Vec<EnvRaw> = qb.build_query_as().fetch_all(&app_state.db).await?;
    let items = raw
        .into_iter()
        .map(|r| EnvItem {
            id: r.id,
            device_id: r.device_id,
            ts: r.ts,
            payload: serde_json::from_str(&r.payload).unwrap_or(serde_json::Value::Null),
            received_at: r.received_at,
        })
        .collect();
    Ok(Response::success(items))
}
