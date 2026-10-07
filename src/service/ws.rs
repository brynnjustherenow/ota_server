//! WebSocket 实时推送：设备活动事件（MQTT 消息 / 视频上传）实时广播到前端。
//!
//! 连接地址：`/ws?token=<JWT>`（浏览器 WebSocket 无法自定义 Authorization 头，
//! 走 query token，校验逻辑与 Bearer 一致）。
//! 组用户连接只接收名下设备的消息（逐条按归属过滤）。

use axum::{
    extract::{Query, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::Response,
};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::{
    AppState,
    service::{
        OtaError,
        auth::{self, CurrentUser, ROLE_GROUP_USER},
        device::is_device_owned_by,
    },
};

/// 广播的活动事件载荷
#[derive(Debug, Clone, Serialize)]
pub struct ActivityMsg {
    pub device_id: String,
    /// "mqtt" | "video"
    pub source: String,
    /// 事件类别：
    /// - "event"：生命周期事件（BOOT/RESET/APP_START/NEXT_CYCLE...）
    /// - "video"：视频上传
    /// - "env"：环境上报；"record"：周期数据上报
    /// - "activity"：其他 MQTT 消息（仅代表设备在线活动）
    pub kind: String,
    /// MQTT 事件名（kind=event 时有值），视频上传为 None
    pub event: Option<String>,
    /// 附加信息：视频文件名 / ev_detail
    pub detail: Option<String>,
    /// unix 秒
    pub ts: i64,
}

/// 广播通道容量：低速事件流，256 足够；无订阅者时 send 报错直接忽略
pub fn new_hub() -> broadcast::Sender<ActivityMsg> {
    broadcast::channel(256).0
}

/// 广播一条活动事件（尽力而为，无订阅者时静默）
pub fn broadcast_activity(tx: &broadcast::Sender<ActivityMsg>, msg: ActivityMsg) {
    let _ = tx.send(msg);
}

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub token: String,
}

/// GET（Upgrade）/ws?token=...
/// token 校验与 Bearer 中间件一致；失败返回 401（不升级）。
pub async fn ws_handler(
    State(app_state): State<AppState>,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, OtaError> {
    let current = auth::resolve_user(&app_state, q.token.trim()).await?;
    Ok(ws.on_upgrade(move |socket| handle_socket(app_state, socket, current)))
}

async fn handle_socket(app_state: AppState, mut socket: WebSocket, current: CurrentUser) {
    let mut rx = app_state.activity_tx.subscribe();
    tracing::debug!(user_id = current.id, role = %current.role, "ws connected");
    loop {
        tokio::select! {
            // 广播 → 按角色/归属过滤后下发
            recv = rx.recv() => {
                let Ok(msg) = recv else { break };
                // DB 异常时按无归属处理（fail closed，避免越权泄露）
                if current.role == ROLE_GROUP_USER
                    && !is_device_owned_by(&app_state.db, &msg.device_id, current.id)
                        .await
                        .unwrap_or(false)
                {
                    continue;
                }
                let Ok(text) = serde_json::to_string(&msg) else { continue };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            // 客户端消息只作 keepalive（ping 文本），收到即忽略；None = 连接关闭
            incoming = socket.recv() => {
                if incoming.is_none() { break; }
            }
        }
    }
    tracing::debug!(user_id = current.id, "ws closed");
}
