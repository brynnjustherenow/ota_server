use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response as AxumResponse},
};
use mqtt5::MqttError;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::time::error;

pub mod http;
pub mod ota;
pub mod util;
pub mod video;
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
    #[error("没有权限")]
    UnAuthed,
}
impl OtaError {
    pub fn code(&self) -> i16 {
        match self {
            Self::MQTTERROR(_)
            | Self::UNDEFINEDOPTIONS(_)
            | Self::InvalidInput(_)
            | Self::IoError(_)
            | Self::DbError(_) => 500,
            Self::FileNotFound(_) => 404,
            Self::UnAuthed => 400,
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
