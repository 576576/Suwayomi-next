//! Error type mapped to HTTP responses.
//! Mirrors `JavalinSetup.kt` exception mapping:
//! NPE/NoSuchElement → 404, IOException → 500, IllegalArgumentException → 400,
//! Unauthorized → 401, Forbidden → 403.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug)]
pub enum ApiError {
    NotFound(String),
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    Internal(String),
}

impl From<suwayomi_db::Error> for ApiError {
    fn from(e: suwayomi_db::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

impl From<suwayomi_domain::error::DomainError> for ApiError {
    fn from(e: suwayomi_domain::error::DomainError) -> Self {
        match e {
            suwayomi_domain::error::DomainError::NotFound(m) => Self::NotFound(m),
            suwayomi_domain::error::DomainError::Invalid(m) => Self::BadRequest(m),
            suwayomi_domain::error::DomainError::Source(m) => Self::Internal(m),
            suwayomi_domain::error::DomainError::Db(e) => Self::Internal(e.to_string()),
            suwayomi_domain::error::DomainError::Sandbox(e) => Self::Internal(e),
            // 参考实现 `getTracker(id)!!` 抛 NPE → 404；站点侧错误与 TokenExpired 都是
            // IOException → 500。这里照抄那套映射。
            suwayomi_domain::error::DomainError::TrackerNotFound(id) => {
                Self::NotFound(format!("tracker {id} not found"))
            }
            suwayomi_domain::error::DomainError::Tracker(m) | suwayomi_domain::error::DomainError::TokenExpired(m) => {
                Self::Internal(m)
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::NotFound(m) => (StatusCode::NOT_FOUND, m),
            Self::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            Self::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m),
            Self::Forbidden(m) => (StatusCode::FORBIDDEN, m),
            Self::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (status, Json(json!({ "message": message }))).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
