use chrono::Local;
use serde::Deserialize;
use sqlx::SqlitePool;
use tracing::{info, warn};

#[derive(Debug, Deserialize)]
struct StatusMessage {
    device_id: Option<String>,
    ev: Option<String>,
    ev_detail: Option<String>,
    ev_ts: Option<i64>,
    ts: Option<i64>,
    record_count: Option<i64>,
}

/// 处理 status topic 的消息：识别 ev 字段的生命周期事件，入库 + 按需 Bark 推送。
///
/// 推送策略（由用户选定）：
/// - RESET：总是推送（设备崩溃/OTA/信号触发的复位）
/// - BOOT：上一条事件不是 RESET 时推送（= 冷启动/断电/意外重启）；复位后的 BOOT 不推
/// - 其他 ev（APP_START/NEXT_CYCLE）：仅入库
///
/// 非 ev 消息（record/env）直接忽略，不入库。
pub async fn handle_status_message(
    db: &SqlitePool,
    bark_key: &str,
    payload: &str,
) -> anyhow::Result<()> {
    let msg: StatusMessage = match serde_json::from_str(payload) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };

    let Some(ev) = msg.ev.as_deref() else {
        return Ok(());
    };

    let device_id = msg.device_id.unwrap_or_else(|| "unknown".into());
    let detail = msg.ev_detail.unwrap_or_default();
    let ev_ts = msg.ev_ts.or(msg.ts).unwrap_or(0);
    let received_at = Local::now().to_rfc3339();

    let last: Option<(String,)> = sqlx::query_as(
        "SELECT event FROM device_events WHERE device_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(&device_id)
    .fetch_optional(db)
    .await?;

    sqlx::query(
        "INSERT INTO device_events (device_id, event, detail, ev_ts, received_at, raw) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&device_id)
    .bind(ev)
    .bind(&detail)
    .bind(ev_ts)
    .bind(&received_at)
    .bind(payload)
    .execute(db)
    .await?;

    info!("[EVENT] {device_id} {ev} {detail} ts={ev_ts}");

    let should_push = match ev {
        "RESET" => true,
        "BOOT" => match &last {
            Some((e,)) => e != "RESET",
            None => true,
        },
        _ => false,
    };

    if should_push && !bark_key.is_empty() {
        let title = match ev {
            "RESET" => format!("{device_id} 复位"),
            _ => format!("{device_id} 启动"),
        };
        let round_str = msg
            .record_count
            .map(|r| format!("\n轮次: {r}"))
            .unwrap_or_default();
        let body = format!("原因: {detail}{round_str}\n收到: {received_at}");
        if let Err(e) = bark_notify(bark_key, &title, &body).await {
            warn!("[EVENT] Bark 推送失败: {e}");
        }
    }

    Ok(())
}

async fn bark_notify(key: &str, title: &str, body: &str) -> anyhow::Result<()> {
    let url = format!("https://api.day.app/{key}");
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .form(&[("title", title), ("body", body)])
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("Bark 返回 HTTP {}", resp.status());
    }
    Ok(())
}
