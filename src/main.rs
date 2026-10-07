use std::{collections::HashMap, fs, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method, StatusCode, header, uri::Port},
    middleware,
    routing::{get, post, put},
};
mod hardware;
mod service;

use mqtt5::{ConnectOptions, MqttClient};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, warn};

#[derive(Clone)]
pub struct AppState {
    ota_client: MqttClient,
    db: sqlx::SqlitePool,
    config: Config,
    uploads: Arc<Mutex<HashMap<String, service::video::UploadSession>>>,
    /// 设备活动事件广播（WebSocket 实时推送）
    activity_tx: tokio::sync::broadcast::Sender<service::ws::ActivityMsg>,
}
impl AppState {
    #[allow(dead_code)]
    fn new(
        client: MqttClient,
        db: sqlx::SqlitePool,
        config: Config,
        activity_tx: tokio::sync::broadcast::Sender<service::ws::ActivityMsg>,
    ) -> Self {
        Self {
            ota_client: client,
            db,
            config,
            uploads: Arc::new(Mutex::new(HashMap::new())),
            activity_tx,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    port: u16,
    #[serde(default = "default_mqtt_client_id")]
    mqtt_client_id: String,
    #[serde(default)]
    mqtt_conf: MqttConfig,
    #[serde(default)]
    bark: BarkConfig,
    #[serde(default)]
    auth: AuthConfig,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct BarkConfig {
    key: String,
}
/// 鉴权配置：jwt_secret 为空时启动阶段自动生成并回写配置文件，
/// 保证重启后已签发的 token 依然有效。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthConfig {
    #[serde(default)]
    jwt_secret: String,
    #[serde(default = "default_token_expire_days")]
    token_expire_days: u64,
}
fn default_token_expire_days() -> u64 {
    7
}
impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            jwt_secret: String::new(),
            token_expire_days: default_token_expire_days(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MqttConfig {
    server_host: String,
    server_post: u16,
    cmd_topic: String,
    status_topic: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            port: 13884,
            mqtt_client_id: default_mqtt_client_id(),
            mqtt_conf: MqttConfig::default(),
            bark: BarkConfig::default(),
            auth: AuthConfig::default(),
        }
    }
}

/// 生成不冲突的默认 client_id：ota_server-<pid>。
/// 多实例运行时（PC 调试 + VPS 部署）避免 EMQX 因为相同 client_id 互踢。
fn default_mqtt_client_id() -> String {
    let pid = std::process::id();
    format!("ota_server-{pid}")
}
impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            server_host: "broker.emqx.io".to_string(),
            server_post: 1883,
            cmd_topic: "k230/cam/cmd".to_string(),
            status_topic: "k230/cam/status".to_string(),
        }
    }
}
#[tokio::main]
async fn main() {
    // initialize tracing
    tracing_subscriber::fmt::init();

    let conf = init_config().await;
    let port = conf.port;
    let bark_key = conf.bark.key.clone();

    // SQLite 先于 MQTT 初始化：status 订阅回调需要 db 句柄入库事件
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename("config.db")
                .create_if_missing(true),
        )
        .await
        .expect("sqlite pool init");
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS version_config (version TEXT PRIMARY KEY, config TEXT NOT NULL)",
    )
    .execute(&db)
    .await
    .expect("version_config migrate");
    // 设备专属 config：按 (version, device_id) 唯一。下发时携带 device_id，仅匹配设备应用。
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS device_version_config (\
         version TEXT NOT NULL,\
         device_id TEXT NOT NULL,\
         config TEXT NOT NULL,\
         PRIMARY KEY (version, device_id))",
    )
    .execute(&db)
    .await
    .expect("device_version_config migrate");
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS device_events (\
         id INTEGER PRIMARY KEY AUTOINCREMENT,\
         device_id TEXT NOT NULL,\
         event TEXT NOT NULL,\
         detail TEXT NOT NULL DEFAULT '',\
         ev_ts INTEGER NOT NULL DEFAULT 0,\
         received_at TEXT NOT NULL,\
         raw TEXT NOT NULL DEFAULT '')",
    )
    .execute(&db)
    .await
    .expect("device_events migrate");
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_events_device ON device_events(device_id, id)")
        .execute(&db)
        .await
        .expect("device_events index");
    // 用户表：三种角色 super_admin(主管理员)/admin(管理员)/group_user(组用户)。
    // token_version 用于改密/禁用/改角色后让旧 JWT 立即失效。
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (\
         id INTEGER PRIMARY KEY AUTOINCREMENT,\
         username TEXT NOT NULL UNIQUE,\
         password_hash TEXT NOT NULL,\
         nickname TEXT NOT NULL DEFAULT '',\
         role TEXT NOT NULL CHECK(role IN ('super_admin','admin','group_user')),\
         enabled INTEGER NOT NULL DEFAULT 1,\
         token_version INTEGER NOT NULL DEFAULT 0,\
         created_at TEXT NOT NULL,\
         updated_at TEXT NOT NULL)",
    )
    .execute(&db)
    .await
    .expect("users migrate");
    service::auth::init_admin(&db).await;
    // 设备表：展示名（仅前端展示，不同步硬件）、归属组用户、上次活动时间（unix 秒）。
    // 行记录按需惰性创建（首次活动/改名/分配归属），老设备不回填；
    // GET /devices 读时三来源 UNION 合并，活动时间兜底取 device_events 最新事件。
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS devices (\
         device_id TEXT PRIMARY KEY,\
         name TEXT NOT NULL DEFAULT '',\
         owner_user_id INTEGER,\
         last_active_at INTEGER,\
         created_at TEXT NOT NULL,\
         updated_at TEXT NOT NULL)",
    )
    .execute(&db)
    .await
    .expect("devices migrate");
    // 环境数据表：MQTT env 消息持久化（payload 原始 JSON，ts unix 秒便于区间查询）
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS device_env (\
         id INTEGER PRIMARY KEY AUTOINCREMENT,\
         device_id TEXT NOT NULL,\
         ts INTEGER NOT NULL,\
         payload TEXT NOT NULL,\
         received_at TEXT NOT NULL)",
    )
    .execute(&db)
    .await
    .expect("device_env migrate");
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_env_device_ts ON device_env(device_id, ts DESC)",
    )
    .execute(&db)
    .await
    .expect("device_env index");

    // WebSocket 活动广播通道（先于 MQTT 初始化：status 回调需要发送端）
    let activity_tx = service::ws::new_hub();

    let client = init_mqtt_client(
        &conf.mqtt_conf,
        &conf.mqtt_client_id,
        db.clone(),
        bark_key,
        activity_tx.clone(),
    )
    .await
    .expect("create mqtt client failed");

    let app_state = AppState {
        ota_client: client,
        db,
        config: conf,
        uploads: Arc::new(Mutex::new(HashMap::new())),
        activity_tx,
    };
    // 不再定时广播：retained 消息会保留在 broker，设备订阅即收到最后一条。
    // 全局在 cmd_topic，设备专属在 cmd_topic/{device_id}，互不覆盖。
    let cors = CorsLayer::new()
        // 只允许特定域名
        .allow_origin(Any)
        // 允许的方法
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::PATCH,
        ])
        // 允许的请求头
        .allow_headers(Any)
        // 暴露的响应头
        .expose_headers(Any)
        // 预检请求缓存时间
        .max_age(Duration::from_secs(3600));
    let app = Router::new()
        .route("/health", get(health))
        // 设备生命周期事件：BOOT/RESET/APP_START（最近 N 条）
        .route("/events", get(service::event::list_events))
        // OTA：列出所有版本 / 拉清单 / 下载
        .route("/ota", get(service::ota::list_versions))
        // 设备端：拉取清单 / 下载文件
        .route(
            "/ota/{version}/manifest",
            get(hardware::http::get_ota_files),
        )
        .route(
            "/ota/{version}/files/{*relpath}",
            get(hardware::http::download_ota_file),
        )
        // 管理端：发布新版本（上传）/ 通知设备升级（MQTT 广播）
        .route(
            "/ota/{version}/publish",
            post(service::ota::ota_publish).layer(DefaultBodyLimit::disable()),
        )
        .route("/ota/{version}/notify", post(service::ota::ota_update))
        // 管理端：按版本读写 config（sqlite，随 fleet_update 下发 merge）
        .route(
            "/ota/{version}/config",
            get(service::ota::get_version_config).post(service::ota::set_version_config),
        )
        // 管理端：设备专属 config（按 device_id 区分，发布时携带 device_id）
        .route(
            "/ota/{version}/devices",
            get(service::ota::list_device_configs),
        )
        .route(
            "/ota/{version}/config/{device_id}",
            get(service::ota::get_device_config)
                .post(service::ota::set_device_config)
                .delete(service::ota::delete_device_config),
        )
        .route(
            "/ota/{version}/publish_device/{device_id}",
            post(service::ota::publish_device_config),
        )
        // 设备 config 模板（来自项目根 device_cfg.json）
        .route("/ota/template", get(service::ota::get_config_template))
        // 设备端：上传/列出/下载视频（关闭默认 2MB body 限制）
        .route("/video", get(service::video::list_devices))
        .route(
            "/video/{device_id}",
            get(service::video::list_videos)
                .post(service::video::upload_video)
                .layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/video/{device_id}/{filename}",
            get(service::video::download_video),
        )
        // 设备端：三段式分片上传（init / chunk / complete / clean）
        // 适配新协议：挂载在 /Mtpi（测试）和 /Mpi（正式）前缀下，
        // 同时支持 /mouseVideoUpload（新毒饵站）和 /eagleVideoUpload（招鹰架）。
        // 路由按前缀组织，避免重复定义。
        .nest("/Mtpi", make_upload_routes())
        .nest("/Mpi", make_upload_routes())
        // 用户系统：登录 / 个人信息 / 改密 / 改昵称
        .route("/auth/login", post(service::auth::login))
        .route("/auth/me", get(service::auth::me))
        .route("/auth/password", put(service::auth::change_password))
        .route("/auth/profile", put(service::auth::update_profile))
        // 用户管理（admin+，handler 内再做角色细分）
        .route(
            "/users",
            get(service::user::list_users).post(service::user::create_user),
        )
        .route(
            "/users/{id}",
            put(service::user::update_user).delete(service::user::delete_user),
        )
        .route("/users/{id}/password", put(service::user::reset_password))
        // WebSocket：设备活动实时推送（token 走 query，handler 内自校验）
        .route("/ws", get(service::ws::ws_handler))
        // 环境数据：设备/日期区间查询（组用户自动限定名下）
        .route("/env", get(service::env::list_env))
        // 设备管理：列表（组用户自动过滤名下）/ 改名 / 分配归属
        .route("/devices", get(service::device::list_devices))
        .route("/devices/{device_id}", put(service::device::update_device))
        .route(
            "/devices/{device_id}/owner",
            put(service::device::update_owner),
        )
        .with_state(app_state.clone())
        // 全局鉴权：auth_guard 内部按 method+path 白名单放行设备端匿名接口
        .layer(middleware::from_fn_with_state(
            app_state.clone(),
            service::auth::auth_guard,
        ))
        .layer(cors);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    service::video::cleanup_orphan_partials().await;
    info!("server listening on port {}", port);
    axum::serve(listener, app).await.unwrap();
}

/// 构造上传路由（mouseVideoUpload + eagleVideoUpload 共用同一组 handler）。
/// 在 main 里挂载到 /Mtpi 和 /Mpi 两个前缀下。
/// 设计：handler 内部不关心 base/biz 前缀，URL 解析由 axum nest 负责。
/// 业务字段（devSerial）由请求体携带，与 biz 路径无关。
fn make_upload_routes() -> Router<AppState> {
    let make_biz = |biz: &'static str| -> Router<AppState> {
        Router::new()
            .route(&format!("/{biz}/init"), post(service::video::upload_init))
            .route(
                &format!("/{biz}/chunk"),
                put(service::video::upload_chunk).layer(DefaultBodyLimit::disable()),
            )
            // 客户端默认走 form-urlencoded（与原 Java 服务端兼容）。
            // 如果要支持 JSON 入口，在 service::video 里加 _json 变体并改路由分发。
            .route(
                &format!("/{biz}/complete"),
                post(service::video::upload_complete),
            )
            .route(&format!("/{biz}/clean"), post(service::video::upload_clean))
    };
    make_biz("mouseVideoUpload")
        .merge(make_biz("eagleVideoUpload"))
        .merge(make_biz("areatest")) // 兼容历史/其他业务前缀（如 /Mtpi/areatest/...）
}

async fn health() -> StatusCode {
    StatusCode::OK
}
/// init the mqtt client and subscribe the topic
/// if has bark_key : the client will handle mqtt message with  and push a massage to the bark
///
async fn init_mqtt_client(
    config: &MqttConfig,
    client_id: &str,
    db: sqlx::SqlitePool,
    bark_key: String,
    activity_tx: tokio::sync::broadcast::Sender<service::ws::ActivityMsg>,
) -> Result<MqttClient, String> {
    let client = MqttClient::new(client_id);
    let opts = ConnectOptions::new(client_id.to_string());
    let host = &config.server_host;
    let port = config.server_post;
    let status_topic = &config.status_topic;
    let cmd_topic = &config.cmd_topic;
    let uri = format!("mqtt://{host}:{port}");
    client
        .connect_with_options(&uri, opts)
        .await
        .map_err(|e| e.to_string())?;
    // subscribe the status topic：解析 ev 事件 → 入库 → 按需 Bark 推送
    let db_status = db.clone();
    let bark_key_status = bark_key.clone();
    if client
        .subscribe(status_topic, move |message| {
            let payload = String::from_utf8_lossy(&message.payload).to_string();
            let db = db_status.clone();
            let bark_key = bark_key_status.clone();
            let activity_tx = activity_tx.clone();
            tokio::spawn(async move {
                if let Err(e) = hardware::event::handle_status_message(
                    &db,
                    &bark_key,
                    &payload,
                    &activity_tx,
                )
                .await
                {
                    warn!("[EVENT] handle failed: {e}");
                }
            });
        })
        .await
        .is_err()
    {
        warn!("subscribe topic [{status_topic}] failed..");
    }
    // subscribe the cmd topic
    if client
        .subscribe(cmd_topic, |message| {
            let topic = message.topic;
            let message = message.payload;
            let message = String::from_utf8(message).expect("Found invalid UTF-8");
            info!("recv message from topic [{topic}],content is [{message}]");
        })
        .await
        .is_err()
    {
        warn!("subscribe topic [{cmd_topic}] failed..")
    }
    client
        .publish(cmd_topic, b"{\"message\":\"test\"}")
        .await
        .map_err(|e| e.to_string())?;

    Ok(client)
}

async fn init_mqtt_server() -> Result<(), String> {
    Ok(())
}
async fn init_config() -> Config {
    let path = ".conf.toml";
    let exist = fs::exists(&path).unwrap();
    let conf_default = Config::default();
    let mut conf = if !exist {
        let str = toml::to_string(&conf_default)
            .expect("failed to serialize config, this won't be happen...");
        info!("config not exist,create it..");
        let _ = fs::write(&path, str);
        info!("create done..");
        conf_default
    } else {
        let result = fs::read_to_string(&path);
        match result {
            Ok(str) if !str.is_empty() => toml::from_str(&str).unwrap_or(conf_default),
            _ => {
                let str = toml::to_string(&conf_default)
                    .expect("failed to serialize config, this won't be happen...");
                info!("config cannot read or empty,overwrite it..");
                let _ = fs::write(&path, str);
                conf_default
            }
        }
    };
    // [auth] 兼容：旧配置文件无 jwt_secret 时生成随机值并回写，
    // 固定持久化，避免每次重启随机生成导致已签发 token 全部失效。
    if conf.auth.jwt_secret.is_empty() {
        conf.auth.jwt_secret = service::auth::generate_jwt_secret();
        if let Ok(s) = toml::to_string(&conf) {
            let _ = fs::write(&path, s);
            info!("jwt_secret generated and persisted to {path}");
        }
    }
    conf
}
