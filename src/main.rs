use std::{fs, time::Duration};

use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method, StatusCode, header, uri::Port},
    routing::{get, post},
};
mod hardware;
mod service;

use mqtt5::{ConnectOptions, MqttClient};
use serde::{Deserialize, Serialize};
use toasty::schema::app;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tracing::{info, warn};
use tracing_subscriber::fmt::format;

#[derive(Clone)]
pub struct AppState {
    ota_client: MqttClient,
    db_pool: String,
    config: Config,
}
impl AppState {
    fn new(client: MqttClient, pool: String, config: Config) -> Self {
        Self {
            ota_client: client,
            db_pool: pool,
            config: config,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    port: u16,
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
            mqtt_conf: MqttConfig::default(),
        }
    }
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
    let client = init_mqtt_client(&conf.mqtt_conf)
        .await
        .expect("create mqtt client failed");

    let port = conf.port;
    let app_state = AppState {
        ota_client: client,
        db_pool: "".to_string(),
        config: conf,
    };
    let _ = app_state.send_version("1.0.1".to_string()).await;
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
        .route("/ota/{version}/publish", post(service::ota::ota_publish))
        .route("/ota/{version}/notify", post(service::ota::ota_update))
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
        .with_state(app_state)
        .layer(cors);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "Hello, World!")
}
/// init the mqtt client and subscribe the topics
async fn init_mqtt_client(config: &MqttConfig) -> Result<MqttClient, String> {
    let client = mqtt5::MqttClient::new("ota_server");
    let opts = ConnectOptions::new("ota_server".to_string());
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
