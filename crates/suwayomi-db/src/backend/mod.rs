//! Backend implementations and the shared [`Db`] handle.

pub mod postgres;
pub mod sqlite;

use std::path::Path;

use crate::config::DbSettings;
use crate::dialect::{self, Dialect};
use crate::error::{Error, Result};
use crate::query::QueryResult;
use crate::row::Row;
use crate::value::Value;

/// Which backend a [`Db`] is running on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    /// Embedded SQLite file (`rheos-tokio-rusqlite`) — the default.
    Sqlite,
    /// External PostgreSQL (`tokio-postgres`).
    Postgres,
}

impl BackendKind {
    /// Dialect used to rewrite queries for this backend.
    pub fn dialect(self) -> Dialect {
        match self {
            Self::Sqlite => Dialect::Sqlite,
            Self::Postgres => Dialect::Postgres,
        }
    }

    /// Stable lower-case identifier (settings files / logs).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
        }
    }
}

/// Database handle shared across the app (cheap to clone — every clone is the
/// same underlying pool/connection).
#[derive(Clone)]
pub struct Db {
    backend: Backend,
}

#[derive(Clone)]
enum Backend {
    Sqlite(sqlite::SqliteBackend),
    Postgres(postgres::PostgresBackend),
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").field("backend", &self.kind().as_str()).finish_non_exhaustive()
    }
}

impl Db {
    /// Opens (creating if needed) the SQLite database at `path`.
    pub async fn sqlite(path: &Path) -> Result<Self> {
        Ok(Self { backend: Backend::Sqlite(sqlite::SqliteBackend::open(path).await?) })
    }

    /// Opens a throw-away in-memory SQLite database (tests).
    pub async fn sqlite_in_memory() -> Result<Self> {
        Ok(Self { backend: Backend::Sqlite(sqlite::SqliteBackend::open_in_memory().await?) })
    }

    /// Connects to an external PostgreSQL server.
    pub async fn postgres(url: &str) -> Result<Self> {
        Ok(Self { backend: Backend::Postgres(postgres::PostgresBackend::connect(url).await?) })
    }

    /// Connects using resolved settings (the entry point used by the server).
    pub async fn connect(settings: &DbSettings) -> Result<Self> {
        match settings.kind {
            BackendKind::Sqlite => Self::sqlite(&settings.path).await,
            BackendKind::Postgres => {
                if settings.url.is_empty() {
                    return Err(Error::Other(
                        "postgres backend selected but no database URL was configured".to_owned(),
                    ));
                }
                Self::postgres(&settings.url).await
            }
        }
    }

    /// Which backend this handle is running on.
    pub fn kind(&self) -> BackendKind {
        match self.backend {
            Backend::Sqlite(_) => BackendKind::Sqlite,
            Backend::Postgres(_) => BackendKind::Postgres,
        }
    }

    /// Dialect of the active backend.
    pub fn dialect(&self) -> Dialect {
        self.kind().dialect()
    }

    /// Compatibility accessor: the old handle exposed a `PgPool` here, and
    /// hundreds of call sites pass `db.pool()` straight to a query builder.
    pub fn pool(&self) -> &Db {
        self
    }

    /// Runs a statement, returning the affected-row count.
    pub async fn execute_sql(&self, sql: &str, params: &[Value]) -> Result<QueryResult> {
        let planned = dialect::plan(self.dialect(), sql, params)?;
        match &self.backend {
            Backend::Sqlite(b) => b.execute(planned.sql, planned.params).await,
            Backend::Postgres(b) => b.execute(planned.sql, planned.params).await,
        }
    }

    /// Runs a query and materialises every row.
    pub async fn fetch_sql(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>> {
        let planned = dialect::plan(self.dialect(), sql, params)?;
        match &self.backend {
            Backend::Sqlite(b) => b.fetch(planned.sql, planned.params).await,
            Backend::Postgres(b) => b.fetch(planned.sql, planned.params).await,
        }
    }

    /// Runs a multi-statement script (DDL, migrations). No parameters.
    pub async fn batch_execute(&self, sql: &str) -> Result<()> {
        match &self.backend {
            Backend::Sqlite(b) => b.batch(sql).await,
            Backend::Postgres(b) => b.batch(sql).await,
        }
    }

    /// Applies the schema migrations for the active backend.
    pub async fn migrate(&self) -> Result<()> {
        crate::migrator::migrate(self).await
    }

    /// Closes the database, releasing the file lock / pool.
    pub async fn close(self) -> Result<()> {
        match self.backend {
            Backend::Sqlite(b) => b.close().await,
            Backend::Postgres(_) => Ok(()),
        }
    }
}
