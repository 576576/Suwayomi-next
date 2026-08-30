//! Connection management — embedded PGlite (default) or external PostgreSQL.
//!
//! Mirrors `DBManager.kt` PostgreSQL path. All tables live in the
//! `suwayomi` schema (matches M0054).
//!
//! Backend selection (Phase 6):
//! - **Embedded (default)** — `Db::connect_embedded`: an in-process PGlite
//!   (ElectricSQL PGlite engine, PostgreSQL 17) served over a local TCP
//!   loopback socket via `pglite-oxide`. No external server, Docker or
//!   install step. The engine owns a single session, so the pool is capped
//!   at one connection.
//! - **External PostgreSQL (fallback)** — `Db::connect`: connects to a
//!   standalone PostgreSQL server through a regular `postgres://` URL.
//!
//! Note: the `pglite-rs` crate (native static-lib build of PGlite) exposes
//! its ORM bridge only over unix sockets, which is inert on Windows; we use
//! `pglite-oxide` instead, which ships the same PGlite engine with a
//! cross-platform TCP gateway, keeping the whole sqlx data layer unchanged.

use std::path::Path;
use std::sync::Arc;

use pglite_oxide::{PgliteServer, PgliteServerBuilder};
use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::db::migrator::MIGRATOR;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("embedded pglite error: {0}")]
    Embedded(#[from] anyhow::Error),
}

/// Which database backend the server is running on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbMode {
    /// In-process PGlite (default) — no external server required.
    Embedded,
    /// External PostgreSQL server via connection URL.
    External,
}

/// Database handle shared across the app (cloned per request via `Arc`).
#[derive(Debug, Clone)]
pub struct Db {
    pool: PgPool,
    /// Keeps the embedded PGlite server alive for the lifetime of the app.
    /// `None` in external mode. `Arc` keeps `Db` cheap to clone; the server
    /// shuts down (via `Drop`) when the last clone is dropped.
    _server: Option<Arc<PgliteServer>>,
}

impl Db {
    /// Connect to an external PostgreSQL server (fallback backend).
    ///
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
        Ok(Self { pool, _server: None })
    }

    /// Connect to an embedded PGlite instance (default backend).
    ///
    /// `data_dir`:
    /// - `Some(path)` — persistent database rooted at `path` (created on
    ///   first open, reused on later starts).
    /// - `None` — ephemeral temporary database (fresh per process; ideal
    ///   for tests).
    ///
    /// The embedded engine owns a single session, so the pool is capped at
    /// one connection; concurrent queries queue on the pool.
    pub async fn connect_embedded(data_dir: Option<&Path>) -> Result<Self, DbError> {
        let server = match data_dir {
            Some(dir) => PgliteServerBuilder::new().path(dir).start()?,
            None => PgliteServer::temporary_tcp()?,
        };
        let url = server.connection_uri();
        tracing::info!(%url, "embedded pglite ready");
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    sqlx::query("SET search_path TO suwayomi").execute(conn).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await?;
        Ok(Self { pool, _server: Some(Arc::new(server)) })
    }

    /// Which backend this handle is running on.
    pub fn mode(&self) -> DbMode {
        if self._server.is_some() { DbMode::Embedded } else { DbMode::External }
    }

    /// Runs the schema migrations for the active backend.
    pub async fn migrate(&self) -> Result<(), DbError> {
        use sqlx::Executor;
        // Run the whole migration on ONE connection, serialized with other
        // concurrent migrators (e.g. parallel test binaries) via a PG
        // advisory lock. Without the lock, two `_sqlx_migrations` inserts on
        // the same version race each other ("tuple concurrently updated").
        let mut conn = self.pool.acquire().await?;
        // Both backends need the `suwayomi` schema to exist before migration:
        // sqlx's `ensure_migrations_table` creates `_sqlx_migrations` with an
        // UNQUALIFIED name that resolves through search_path — on a fresh
        // database the missing schema would fail with 3F000.
        conn.execute("CREATE SCHEMA IF NOT EXISTS suwayomi").await?;
        if self._server.is_some() {
            // The pglite-oxide TCP proxy terminates the embedded session on
            // ANY SQL error (e.g. 42P01 / 3F000). sqlx migrate probes
            // `_sqlx_migrations` (error 42P01 on a fresh database) which
            // would kill the session mid-migrate. Pre-creating the table,
            // schema-qualified, makes every subsequent unqualified statement
            // resolve without error.
            conn.execute(
                r#"CREATE TABLE IF NOT EXISTS suwayomi._sqlx_migrations (
                    version BIGINT PRIMARY KEY,
                    description TEXT NOT NULL,
                    installed_on TIMESTAMPTZ NOT NULL DEFAULT now(),
                    success BOOLEAN NOT NULL,
                    checksum BYTEA NOT NULL,
                    execution_time BIGINT NOT NULL
                )"#,
            )
            .await?;
        }
        conn.execute("SELECT pg_advisory_lock(728232364)").await?;
        let r = MIGRATOR.run(&mut conn).await;
        // SyncYomi version-bump triggers are PostgreSQL-only: they are written
        // in PL/pgSQL, which the embedded pglite parser cannot compile (the
        // baseline schema is pure DDL, so the embedded backend stays healthy).
        // The files live in `migrations/pg-only/` so the sqlx migrator never
        // sees them; external PostgreSQL applies them here (idempotent:
        // CREATE OR REPLACE + DROP TRIGGER IF EXISTS).
        let pg_only = if self._server.is_none() {
            let f = conn
                .execute(include_str!("../../../../migrations/pg-only/0002_sync_functions.sql"))
                .await;
            let t = conn
                .execute(include_str!("../../../../migrations/pg-only/0002_sync_triggers.sql"))
                .await;
            f.and(t)
        } else {
            Ok(sqlx::postgres::PgQueryResult::default())
        };
        conn.execute("SELECT pg_advisory_unlock(728232364)").await?;
        r?;
        pg_only?;
        Ok(())
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
