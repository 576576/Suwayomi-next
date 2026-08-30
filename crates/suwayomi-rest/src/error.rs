//! Error type mapped to HTTP responses.
//! Mirrors `JavalinSetup.kt` exception mapping:
//! NPE/NoSuchElement → 404, IOException → 500, IllegalArgumentException → 400,
//! Unauthorized → 401, Forbidden → 403.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub enum ApiError {
    NotFound(String),
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    Internal(String),
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        ApiError::Internal(e.to_string())
    }
}

impl From<suwayomi_domain::error::DomainError> for ApiError {
    fn from(e: suwayomi_domain::error::DomainError) -> Self {
        match e {
            suwayomi_domain::error::DomainError::NotFound(m) => ApiError::NotFound(m),
            suwayomi_domain::error::DomainError::Invalid(m) => ApiError::BadRequest(m),
            suwayomi_domain::error::DomainError::Source(m) => ApiError::Internal(m),
            suwayomi_domain::error::DomainError::Db(e) => ApiError::Internal(e.to_string()),
            suwayomi_domain::error::DomainError::DbSetup(e) => ApiError::Internal(e.to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m),
            ApiError::Forbidden(m) => (StatusCode::FORBIDDEN, m),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (status, Json(json!({ "message": message }))).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
