use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response as AxumResponse},
};
use mqtt5::MqttError;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod auth;
pub mod device;
pub mod env;
pub mod event;
pub mod http;
pub mod ota;
pub mod user;
pub mod util;
pub mod video;
pub mod ws;
#[derive(Error, Debug)]
pub enum OtaError {
    #[error("mqtt error: [{0}]")]
    MQTTERROR(#[from] MqttError),
    #[error("未定义行为")]
    UNDEFINEDOPTIONS(i16),
    #[error("文件未找到: {0}")]
    FileNotFound(String),
    #[error("无效输入: {0}")]
    InvalidInput(String),
    #[error("I/O 错误: {0}")]
    IoError(#[from] std::io::Error),
    #[error("DB 错误: {0}")]
    DbError(#[from] sqlx::Error),
    #[error("jwt 错误: {0}")]
    JwtError(#[from] jsonwebtoken::errors::Error),
    #[error("没有权限")]
    UnAuthed,
    #[error("禁止访问: {0}")]
    Forbidden(String),
    #[error("认证失败: {0}")]
    BadCredentials(String),
    #[error("参数错误: {0}")]
    InvalidParam(String),
}
impl OtaError {
    pub fn code(&self) -> i16 {
        match self {
            Self::MQTTERROR(_)
            | Self::UNDEFINEDOPTIONS(_)
            | Self::InvalidInput(_)
            | Self::IoError(_)
            | Self::DbError(_)
            | Self::JwtError(_) => 500,
            Self::FileNotFound(_) => 404,
            Self::UnAuthed | Self::BadCredentials(_) => 401,
            Self::Forbidden(_) => 403,
            Self::InvalidParam(_) => 400,
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Response<T: Sized + Send> {
    code: i16,
    data: Option<T>,
    message: String,
}
impl<T: Send> Response<T> {
    pub fn success(data: T) -> Self {
        Self {
            code: 200,
            data: Some(data),
            message: "success".to_string(),
        }
    }
    pub fn error(err: OtaError) -> Self {
        Self {
            code: err.code(),
            data: None,
            message: err.to_string(),
        }
    }
}
impl<T: Send> From<OtaError> for Response<T> {
    fn from(value: OtaError) -> Self {
        Self::error(value)
    }
}

impl<T: Serialize + Send> IntoResponse for Response<T> {
    fn into_response(self) -> AxumResponse {
        let code = match self.code {
            200 => StatusCode::OK,
            400 => StatusCode::BAD_REQUEST,
            401 => StatusCode::UNAUTHORIZED,
            403 => StatusCode::FORBIDDEN,
            404 => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (code, Json(self)).into_response()
    }
}

impl IntoResponse for OtaError {
    fn into_response(self) -> AxumResponse {
        Response::<()>::error(self).into_response()
    }
}
