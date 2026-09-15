//! Errors surfaced by the database layer.
//!
//! One error type for both backends: the callers (`suwayomi-domain`,
//! `-graphql`, `-rest`, `-opds`) only ever propagate it, so erasing the
//! backend-specific detail into a message keeps the port mechanical.

/// Result alias used throughout the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Database error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sqlite error: {0}")]
    Sqlite(String),
    #[error("postgres error: {0}")]
    Postgres(String),
    #[error("connection pool error: {0}")]
    Pool(String),
    #[error("no rows returned by a query that expected one")]
    RowNotFound,
    #[error("column not found: {0}")]
    ColumnNotFound(String),
    #[error("unexpected NULL in column {0}")]
    UnexpectedNull(String),
    #[error("cannot decode column {column} as {expected}")]
    Decode { column: String, expected: &'static str },
    #[error("migration error: {0}")]
    Migrate(String),
    #[error("sql error: {0}")]
    Other(String),
}

impl From<rusqlite::Error> for Error {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value.to_string())
    }
}

impl From<rheos_tokio_rusqlite::Error> for Error {
    fn from(value: rheos_tokio_rusqlite::Error) -> Self {
        match value {
            rheos_tokio_rusqlite::Error::Rusqlite(e) => Self::Sqlite(e.to_string()),
            other => Self::Other(other.to_string()),
        }
    }
}

impl From<tokio_postgres::Error> for Error {
    fn from(value: tokio_postgres::Error) -> Self {
        // `tokio_postgres::Error`'s own `Display` is just "db error"; the
        // SQLSTATE and the server message only live on `as_db_error()`.
        match value.as_db_error() {
            Some(db) => Self::Postgres(format!("[{}] {db}", db.code().code())),
            None => Self::Postgres(value.to_string()),
        }
    }
}

impl From<deadpool_postgres::PoolError> for Error {
    fn from(value: deadpool_postgres::PoolError) -> Self {
        Self::Pool(value.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::Other(value.to_string())
    }
}
