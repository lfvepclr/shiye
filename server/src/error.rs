//! 错误类型与 HTTP 映射。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("缺少或非法的设备标识")]
    Unauthorized,

    #[error("资源不存在")]
    NotFound,

    #[error("{0}")]
    BadRequest(String),

    #[error("请求体过大")]
    PayloadTooLarge,

    #[error("数据库错误: {0}")]
    Db(#[from] sqlx::Error),

    #[error("内部错误: {0}")]
    Internal(String),
}

impl AppError {
    pub fn internal(e: impl std::fmt::Display) -> Self {
        AppError::Internal(e.to_string())
    }

    fn status(&self) -> StatusCode {
        match self {
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::Db(sqlx::Error::RowNotFound) => StatusCode::NOT_FOUND,
            AppError::Db(_) | AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            AppError::Unauthorized => "unauthorized",
            AppError::NotFound => "not_found",
            AppError::BadRequest(_) => "bad_request",
            AppError::PayloadTooLarge => "payload_too_large",
            AppError::Db(sqlx::Error::RowNotFound) => "not_found",
            AppError::Db(_) => "db_error",
            AppError::Internal(_) => "internal",
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status.is_server_error() {
            tracing::error!(error = %self, "请求失败");
        }
        let body = json!({
            "error": self.code(),
            "message": self.to_string(),
        });
        (status, Json(body)).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
