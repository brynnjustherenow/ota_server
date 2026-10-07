//! JWT 鉴权：登录 / 个人信息 / 改密 / 改昵称 + 全局鉴权中间件。
//!
//! 设备端接口不走鉴权（设备无法携带用户 token），由 auth_guard 内的
//! is_public 按 method+path 白名单放行；其余接口一律要求 Bearer token。
//! group_user 对 OTA 管理接口一律 403（数据归属过滤在设备阶段实现）。

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::SaltString,
};
use axum::{
    Extension, Json,
    extract::{Request, State},
    http::{Method, header::AUTHORIZATION},
    middleware::Next,
    response::{IntoResponse, Response as AxumResponse},
};
use chrono::{Local, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::{
    AppState,
    service::{OtaError, Response},
};

pub const ROLE_SUPER_ADMIN: &str = "super_admin";
pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_GROUP_USER: &str = "group_user";

/// JWT claims：sub=user_id，ver=token_version（改密/禁用/改角色后 +1 使旧 token 失效）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub username: String,
    pub role: String,
    pub ver: i64,
    pub exp: usize,
    pub iat: usize,
}

/// 中间件注入的当前登录用户
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: i64,
    pub role: String,
}
impl CurrentUser {
    pub fn is_super_admin(&self) -> bool {
        self.role == ROLE_SUPER_ADMIN
    }
    pub fn is_admin_or_above(&self) -> bool {
        self.role == ROLE_SUPER_ADMIN || self.role == ROLE_ADMIN
    }
}

/// DB 完整行（含敏感字段，禁止直接序列化给客户端）
#[derive(Debug, FromRow)]
pub struct UserFull {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub nickname: String,
    pub role: String,
    pub enabled: bool,
    pub token_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// 对外暴露的用户信息
#[derive(Debug, Serialize, FromRow)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub nickname: String,
    pub role: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}
impl From<UserFull> for UserInfo {
    fn from(u: UserFull) -> Self {
        Self {
            id: u.id,
            username: u.username,
            nickname: u.nickname,
            role: u.role,
            enabled: u.enabled,
            created_at: u.created_at,
            updated_at: u.updated_at,
        }
    }
}

// ─────────────── 密码与密钥工具 ───────────────

pub fn hash_password(password: &str) -> Result<String, OtaError> {
    let salt = SaltString::generate(&mut rand::thread_rng());
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| OtaError::InvalidInput("密码哈希失败".into()))
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// 生成 64 位 hex 随机 secret（启动时写入 .conf.toml 的 [auth].jwt_secret）
pub fn generate_jwt_secret() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn sign_token(state: &AppState, user: &UserFull) -> Result<String, OtaError> {
    let now = Utc::now().timestamp() as usize;
    let claims = Claims {
        sub: user.id,
        username: user.username.clone(),
        role: user.role.clone(),
        ver: user.token_version,
        exp: now + (state.config.auth.token_expire_days as usize) * 86400,
        iat: now,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.config.auth.jwt_secret.as_bytes()),
    )
    .map_err(OtaError::from)
}

// ─────────────── 全局鉴权中间件 ───────────────

pub async fn auth_guard(
    State(app_state): State<AppState>,
    mut req: Request,
    next: Next,
) -> AxumResponse {
    if is_public(req.method(), req.uri().path()) {
        return next.run(req).await;
    }
    let Some(token) = bearer_token(&req) else {
        return OtaError::UnAuthed.into_response();
    };
    let current = match resolve_user(&app_state, &token).await {
        Ok(u) => u,
        Err(e) => return e.into_response(),
    };
    // 组用户禁止访问 OTA 管理接口（manifest/文件下载已在 is_public 放行，与用户角色无关）
    if current.role == ROLE_GROUP_USER && is_ota_admin_path(req.uri().path()) {
        return OtaError::Forbidden("组用户无权访问 OTA 管理接口".into()).into_response();
    }
    req.extensions_mut().insert(current);
    next.run(req).await
}

fn bearer_token(req: &Request) -> Option<String> {
    req.headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 校验 token 并取出当前用户（Bearer 中间件与 /ws?token= 共用）
pub async fn resolve_user(state: &AppState, token: &str) -> Result<CurrentUser, OtaError> {
    let claims = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.config.auth.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| OtaError::UnAuthed)?
    .claims;
    let user = sqlx::query_as::<_, UserFull>(
        "SELECT id, username, password_hash, nickname, role, enabled, token_version, created_at, updated_at \
         FROM users WHERE id = ?",
    )
    .bind(claims.sub)
    .fetch_optional(&state.db)
    .await?;
    match user {
        Some(u) if u.enabled && u.token_version == claims.ver => Ok(CurrentUser {
            id: u.id,
            role: u.role,
        }),
        _ => Err(OtaError::UnAuthed),
    }
}

/// 设备端匿名接口白名单（设备无法携带用户 token）：
/// - GET /health、POST /auth/login
/// - GET /ota/{version}/manifest、GET /ota/{version}/files/*（设备拉 OTA）
/// - POST /video/{device_id}（设备原始视频上传）
/// - /Mtpi/*、/Mpi/*（分片上传协议，POST/PUT）
fn is_public(method: &Method, path: &str) -> bool {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match segs.first() {
        Some(&"health") => segs.len() == 1 && method == Method::GET,
        Some(&"auth") => segs.len() == 2 && segs[1] == "login" && method == Method::POST,
        // WebSocket 升级：token 在 query，由 ws_handler 自校验（Bearer 中间件放行）
        Some(&"ws") => segs.len() == 1,
        // /ota/{version}/manifest 与 /ota/{version}/files/{...}
        Some(&"ota") => {
            (segs.len() == 3 && segs[2] == "manifest" || segs.len() >= 4 && segs[2] == "files")
                && method == Method::GET
        }
        // 仅 POST /video/{device_id}（上传）放行；GET 列表是管理端接口需登录
        Some(&"video") => segs.len() == 2 && method == Method::POST,
        Some(&"Mtpi") | Some(&"Mpi") => true,
        _ => false,
    }
}

/// 需要管理员以上角色的 OTA 管理路径（公开部分已在 is_public 提前放行）
fn is_ota_admin_path(path: &str) -> bool {
    path == "/ota" || path.starts_with("/ota/")
}

// ─────────────── 用户查询辅助 ───────────────

pub async fn fetch_user(db: &SqlitePool, id: i64) -> Result<UserFull, OtaError> {
    sqlx::query_as::<_, UserFull>(
        "SELECT id, username, password_hash, nickname, role, enabled, token_version, created_at, updated_at \
         FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| OtaError::InvalidParam("用户不存在".into()))
}

pub async fn fetch_user_by_username(db: &SqlitePool, username: &str) -> Result<UserFull, OtaError> {
    sqlx::query_as::<_, UserFull>(
        "SELECT id, username, password_hash, nickname, role, enabled, token_version, created_at, updated_at \
         FROM users WHERE username = ?",
    )
    .bind(username)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| OtaError::InvalidParam("用户不存在".into()))
}

// ─────────────── HTTP handlers ───────────────

#[derive(Debug, Deserialize)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResp {
    pub token: String,
    pub user: UserInfo,
}

/// POST /auth/login
pub async fn login(
    State(app_state): State<AppState>,
    Json(req): Json<LoginReq>,
) -> Result<Response<LoginResp>, OtaError> {
    let username = req.username.trim().to_string();
    if username.is_empty() || req.password.is_empty() {
        return Err(OtaError::BadCredentials("用户名或密码错误".into()));
    }
    let Some(user) = sqlx::query_as::<_, UserFull>(
        "SELECT id, username, password_hash, nickname, role, enabled, token_version, created_at, updated_at \
         FROM users WHERE username = ?",
    )
    .bind(&username)
    .fetch_optional(&app_state.db)
    .await?
    else {
        return Err(OtaError::BadCredentials("用户名或密码错误".into()));
    };
    if !user.enabled {
        return Err(OtaError::Forbidden("账号已被禁用".into()));
    }
    if !verify_password(&user.password_hash, &req.password) {
        return Err(OtaError::BadCredentials("用户名或密码错误".into()));
    }
    let token = sign_token(&app_state, &user)?;
    Ok(Response::success(LoginResp {
        token,
        user: user.into(),
    }))
}

/// GET /auth/me
pub async fn me(
    State(app_state): State<AppState>,
    Extension(current): Extension<CurrentUser>,
) -> Result<Response<UserInfo>, OtaError> {
    let user = fetch_user(&app_state.db, current.id).await?;
    Ok(Response::success(user.into()))
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordReq {
    pub old_password: String,
    pub new_password: String,
}

/// PUT /auth/password：验证原密码后修改；token_version+1 使旧 token 失效，
/// 返回新 token 供前端原地续期。
pub async fn change_password(
    State(app_state): State<AppState>,
    Extension(current): Extension<CurrentUser>,
    Json(req): Json<ChangePasswordReq>,
) -> Result<Response<LoginResp>, OtaError> {
    if req.new_password.chars().count() < 6 {
        return Err(OtaError::InvalidParam("新密码至少 6 位".into()));
    }
    if req.new_password.chars().count() > 64 {
        return Err(OtaError::InvalidParam("新密码过长（最多 64 位）".into()));
    }
    let user = fetch_user(&app_state.db, current.id).await?;
    if !verify_password(&user.password_hash, &req.old_password) {
        return Err(OtaError::BadCredentials("原密码不正确".into()));
    }
    let hash = hash_password(&req.new_password)?;
    let now = Local::now().to_rfc3339();
    sqlx::query(
        "UPDATE users SET password_hash = ?, token_version = token_version + 1, updated_at = ? \
         WHERE id = ?",
    )
    .bind(&hash)
    .bind(&now)
    .bind(current.id)
    .execute(&app_state.db)
    .await?;
    let user = fetch_user(&app_state.db, current.id).await?;
    let token = sign_token(&app_state, &user)?;
    Ok(Response::success(LoginResp {
        token,
        user: user.into(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct UpdateProfileReq {
    pub nickname: String,
}

/// PUT /auth/profile：修改自己的昵称（所有角色可改自己）
pub async fn update_profile(
    State(app_state): State<AppState>,
    Extension(current): Extension<CurrentUser>,
    Json(req): Json<UpdateProfileReq>,
) -> Result<Response<UserInfo>, OtaError> {
    let nickname = req.nickname.trim().to_string();
    if nickname.is_empty() {
        return Err(OtaError::InvalidParam("昵称不能为空".into()));
    }
    if nickname.chars().count() > 32 {
        return Err(OtaError::InvalidParam("昵称最多 32 个字符".into()));
    }
    let now = Local::now().to_rfc3339();
    sqlx::query("UPDATE users SET nickname = ?, updated_at = ? WHERE id = ?")
        .bind(&nickname)
        .bind(&now)
        .bind(current.id)
        .execute(&app_state.db)
        .await?;
    let user = fetch_user(&app_state.db, current.id).await?;
    Ok(Response::success(user.into()))
}

// ─────────────── 初始化 ───────────────

/// 首次启动（users 表为空）创建主管理员 admin/admin123
pub async fn init_admin(db: &SqlitePool) {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(db)
        .await
        .expect("users count");
    if count.0 > 0 {
        return;
    }
    let hash = hash_password("admin123").expect("hash initial admin password");
    let now = Local::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO users (username, password_hash, nickname, role, created_at, updated_at) \
         VALUES ('admin', ?, '主管理员', 'super_admin', ?, ?)",
    )
    .bind(&hash)
    .bind(&now)
    .bind(&now)
    .execute(db)
    .await
    .expect("create initial admin");
    tracing::info!("已创建初始主管理员 admin / admin123，请尽快登录修改密码");
}
