use std::{collections::HashMap, fs, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method, StatusCode, header, uri::Port},
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
}
impl AppState {
    fn new(client: MqttClient, db: sqlx::SqlitePool, config: Config) -> Self {
        Self {
            ota_client: client,
            db,
            config: config,
            uploads: Arc::new(Mutex::new(HashMap::new())),
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
    let client = init_mqtt_client(&conf.mqtt_conf, &conf.mqtt_client_id)
        .await
        .expect("create mqtt client failed");

    let port = conf.port;
    // SQLite：存储每个版本对应的 config（随 fleet_update 下发 merge）
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
    let app_state = AppState {
        ota_client: client,
        db,
        config: conf,
        uploads: Arc::new(Mutex::new(HashMap::new())),
    };
    // 每 10 分钟广播最新版本（retained），设备订阅即收
    hardware::mqtt::interval_publish_service(&app_state);
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
        .with_state(app_state)
        .layer(cors);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
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
/// init the mqtt client and subscribe the topics
async fn init_mqtt_client(config: &MqttConfig, client_id: &str) -> Result<MqttClient, String> {
    let client = mqtt5::MqttClient::new(client_id);
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
    // subscribe the status topic
    if client
        .subscribe(status_topic, |message| {
            let topic = message.topic;
            let message = message.payload;
            let message = String::from_utf8(message).expect("Found invalid UTF-8");
            info!("recv message from topic [{topic}],content is [{message}]");
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
    let conf_default = Config::default();
    let path = ".conf.toml";
    let exist = fs::exists(&path).unwrap();
    if !exist {
        let str = toml::to_string(&conf_default)
            .expect("failed to serialize config, this won't be happen...");
        info!("config not exist,create it..");
        fs::write(&path, str);
        info!("create done..");
        return conf_default;
    }
    let result = fs::read_to_string(&path);
    let Ok(str) = result else {
        let str = toml::to_string(&conf_default)
            .expect("failed to serialize config, this won't be happen...");
        info!("config cannot read,overwrite it..");
        fs::write(&path, str);
        info!("overwrite done..");
        return conf_default;
    };
    if str.is_empty() {
        let str = toml::to_string(&conf_default)
            .expect("failed to serialize config, this won't be happen...");
        info!("config is empty,write it..");
        fs::write(&path, str);
        info!("write done..");
        return conf_default;
    }
    let conf = toml::from_str(&str).unwrap_or(conf_default);
    conf
}
