//! 用户管理（admin+）。
//! 权限边界：admin 仅能增删改 group_user；super_admin 管理全部用户。
//! 改密/禁用/改角色都会 token_version+1，使目标用户已签发的 token 立即失效。

use axum::{
    Extension, Json,
    extract::{Path as AxumPath, State},
};
use chrono::Local;
use serde::Deserialize;

use crate::{
    AppState,
    service::{
        OtaError, Response,
        auth::{
            self, CurrentUser, ROLE_ADMIN, ROLE_GROUP_USER, ROLE_SUPER_ADMIN, UserInfo,
        },
    },
};

fn valid_role(role: &str) -> bool {
    matches!(role, ROLE_SUPER_ADMIN | ROLE_ADMIN | ROLE_GROUP_USER)
}

/// 操作者是否可以管理目标角色账号
fn can_manage(operator: &CurrentUser, target_role: &str) -> bool {
    if operator.is_super_admin() {
        true
    } else {
        operator.role == ROLE_ADMIN && target_role == ROLE_GROUP_USER
    }
}

fn require_admin(user: &CurrentUser) -> Result<(), OtaError> {
    if user.is_admin_or_above() {
        Ok(())
    } else {
        Err(OtaError::Forbidden("仅管理员以上可访问".into()))
    }
}

fn check_username(username: &str) -> Result<(), OtaError> {
    let len = username.chars().count();
    if !(2..=32).contains(&len) {
        return Err(OtaError::InvalidParam("用户名长度需 2-32 个字符".into()));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(OtaError::InvalidParam(
            "用户名仅支持字母 / 数字 / _ / -".into(),
        ));
    }
    Ok(())
}

fn check_password(password: &str) -> Result<(), OtaError> {
    let len = password.chars().count();
    if !(6..=64).contains(&len) {
        return Err(OtaError::InvalidParam("密码长度需 6-64 位".into()));
    }
    Ok(())
}

/// GET /users：super_admin 看全部；admin 只看组用户
pub async fn list_users(
    State(app_state): State<AppState>,
    Extension(current): Extension<CurrentUser>,
) -> Result<Response<Vec<UserInfo>>, OtaError> {
    require_admin(&current)?;
    let rows = if current.is_super_admin() {
        sqlx::query_as::<_, UserInfo>(
            "SELECT id, username, nickname, role, enabled, created_at, updated_at \
             FROM users ORDER BY id",
        )
    } else {
        sqlx::query_as::<_, UserInfo>(
            "SELECT id, username, nickname, role, enabled, created_at, updated_at \
             FROM users WHERE role = 'group_user' ORDER BY id",
        )
    };
    let users = rows.fetch_all(&app_state.db).await?;
    Ok(Response::success(users))
}

#[derive(Debug, Deserialize)]
pub struct CreateUserReq {
    pub username: String,
    pub password: String,
    pub nickname: Option<String>,
    pub role: String,
}

/// POST /users：admin 只能创建组用户；super_admin 可创建任意角色
pub async fn create_user(
    State(app_state): State<AppState>,
    Extension(current): Extension<CurrentUser>,
    Json(req): Json<CreateUserReq>,
) -> Result<Response<UserInfo>, OtaError> {
    require_admin(&current)?;
    let username = req.username.trim().to_string();
    // 昵称未填则默认同账号，减少创建时的输入
    let nickname = req
        .nickname
        .unwrap_or_default()
        .trim()
        .to_string();
    let nickname = if nickname.is_empty() {
        username.clone()
    } else {
        nickname
    };
    check_username(&username)?;
    if nickname.chars().count() > 32 {
        return Err(OtaError::InvalidParam("昵称最多 32 个字符".into()));
    }
    check_password(&req.password)?;
    if !valid_role(&req.role) {
        return Err(OtaError::InvalidParam("非法角色".into()));
    }
    if !current.is_super_admin() && req.role != ROLE_GROUP_USER {
        return Err(OtaError::Forbidden("管理员仅可创建组用户".into()));
    }
    let hash = auth::hash_password(&req.password)?;
    let now = Local::now().to_rfc3339();
    let result = sqlx::query(
        "INSERT INTO users (username, password_hash, nickname, role, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&username)
    .bind(&hash)
    .bind(&nickname)
    .bind(&req.role)
    .bind(&now)
    .bind(&now)
    .execute(&app_state.db)
    .await;
    if let Err(e) = result {
        if e.to_string().to_lowercase().contains("unique") {
            return Err(OtaError::InvalidParam("用户名已存在".into()));
        }
        return Err(e.into());
    }
    let user = auth::fetch_user_by_username(&app_state.db, &username).await?;
    Ok(Response::success(user.into()))
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserReq {
    pub nickname: Option<String>,
    pub role: Option<String>,
    pub enabled: Option<bool>,
}

/// PUT /users/{id}：编辑昵称/角色/启用状态。
/// 防锁定：任何人不能禁用或变更自己的角色；角色变更仅 super_admin 可做。
pub async fn update_user(
    State(app_state): State<AppState>,
    Extension(current): Extension<CurrentUser>,
    AxumPath(id): AxumPath<i64>,
    Json(req): Json<UpdateUserReq>,
) -> Result<Response<UserInfo>, OtaError> {
    require_admin(&current)?;
    let target = auth::fetch_user(&app_state.db, id).await?;
    if !can_manage(&current, &target.role) {
        return Err(OtaError::Forbidden("无权操作该用户".into()));
    }
    let mut nickname = target.nickname.clone();
    if let Some(n) = req.nickname {
        nickname = n.trim().to_string();
        if nickname.is_empty() {
            return Err(OtaError::InvalidParam("昵称不能为空".into()));
        }
        if nickname.chars().count() > 32 {
            return Err(OtaError::InvalidParam("昵称最多 32 个字符".into()));
        }
    }
    let mut role = target.role.clone();
    let mut role_changed = false;
    if let Some(r) = &req.role {
        if !valid_role(r) {
            return Err(OtaError::InvalidParam("非法角色".into()));
        }
        if *r != target.role {
            role_changed = true;
            role = r.clone();
        }
    }
    let mut enabled = target.enabled;
    let mut enabled_changed = false;
    if let Some(e) = req.enabled
        && e != target.enabled
    {
        enabled_changed = true;
        enabled = e;
    }
    // 防锁定：不能禁用自己、不能变更自己的角色
    if current.id == target.id && (!enabled || role_changed) {
        return Err(OtaError::InvalidParam("不能禁用自己或变更自己的角色".into()));
    }
    // admin 的操作对象只能是组用户（can_manage 已保证），角色变更则仅 super_admin
    if role_changed && !current.is_super_admin() {
        return Err(OtaError::Forbidden("仅主管理员可变更角色".into()));
    }
    let now = Local::now().to_rfc3339();
    // 禁用/启用变更、角色变更 → token_version+1 让旧 token 失效
    if enabled_changed || role_changed {
        sqlx::query(
            "UPDATE users SET nickname = ?, role = ?, enabled = ?, \
             token_version = token_version + 1, updated_at = ? WHERE id = ?",
        )
        .bind(&nickname)
        .bind(&role)
        .bind(enabled)
        .bind(&now)
        .bind(id)
        .execute(&app_state.db)
        .await?;
    } else {
        sqlx::query("UPDATE users SET nickname = ?, role = ?, enabled = ?, updated_at = ? WHERE id = ?")
            .bind(&nickname)
            .bind(&role)
            .bind(enabled)
            .bind(&now)
            .bind(id)
            .execute(&app_state.db)
            .await?;
    }
    let user = auth::fetch_user(&app_state.db, id).await?;
    Ok(Response::success(user.into()))
}

#[derive(Debug, Deserialize)]
pub struct ResetPasswordReq {
    pub new_password: String,
}

/// PUT /users/{id}/password：管理员重置密码（无需原密码）
pub async fn reset_password(
    State(app_state): State<AppState>,
    Extension(current): Extension<CurrentUser>,
    AxumPath(id): AxumPath<i64>,
    Json(req): Json<ResetPasswordReq>,
) -> Result<Response<()>, OtaError> {
    require_admin(&current)?;
    let target = auth::fetch_user(&app_state.db, id).await?;
    if !can_manage(&current, &target.role) {
        return Err(OtaError::Forbidden("无权操作该用户".into()));
    }
    check_password(&req.new_password)?;
    let hash = auth::hash_password(&req.new_password)?;
    let now = Local::now().to_rfc3339();
    sqlx::query(
        "UPDATE users SET password_hash = ?, token_version = token_version + 1, updated_at = ? \
         WHERE id = ?",
    )
    .bind(&hash)
    .bind(&now)
    .bind(id)
    .execute(&app_state.db)
    .await?;
    Ok(Response::success(()))
}

/// DELETE /users/{id}：不能删除自己
pub async fn delete_user(
    State(app_state): State<AppState>,
    Extension(current): Extension<CurrentUser>,
    AxumPath(id): AxumPath<i64>,
) -> Result<Response<()>, OtaError> {
    require_admin(&current)?;
    if current.id == id {
        return Err(OtaError::InvalidParam("不能删除自己".into()));
    }
    let target = auth::fetch_user(&app_state.db, id).await?;
    if !can_manage(&current, &target.role) {
        return Err(OtaError::Forbidden("无权操作该用户".into()));
    }
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(&app_state.db)
        .await?;
    Ok(Response::success(()))
}
