//! Domain-layer error type.

use suwayomi_core::db::DbError;

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("db setup error: {0}")]
    DbSetup(#[from] DbError),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("source error: {0}")]
    Source(String),
    #[error("sandbox http error: {0}")]
    Sandbox(String),
}

impl From<reqwest::Error> for DomainError {
    fn from(e: reqwest::Error) -> Self {
        Self::Sandbox(e.to_string())
    }
}

impl DomainError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, DomainError>;
