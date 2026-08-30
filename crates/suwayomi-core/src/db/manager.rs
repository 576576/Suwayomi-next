//! Connection management — PostgreSQL only.
//!
//! Mirrors `DBManager.kt` PostgreSQL path. All tables live in the
//! `suwayomi` schema (matches M0054).

use sqlx::postgres::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::db::migrator::MIGRATOR;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

/// PostgreSQL-backed database handle.
#[derive(Debug, Clone)]
pub struct Db {
    pool: PgPool,
}

impl Db {
    /// Mirrors `DBManager` PostgreSQL path (`databaseUrl` JDBC-style is
    /// translated by the caller; here we accept a sqlx URL).
    ///
    /// Sets `search_path` to the `suwayomi` schema on every connection,
    /// mirroring the Kotlin side's `defaultSchema` (M0054).
    pub async fn connect(url: &str) -> Result<Self, DbError> {
        let pool = PgPoolOptions::new()
            .max_connections(6)
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    sqlx::query("SET search_path TO suwayomi").execute(conn).await?;
                    Ok(())
                })
            })
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    /// Runs the schema migrations for the active backend.
    pub async fn migrate(&self) -> Result<(), DbError> {
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
