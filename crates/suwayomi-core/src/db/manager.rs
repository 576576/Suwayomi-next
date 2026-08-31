//! Connection management — embedded Oliphaunt (default) or external PostgreSQL.
//!
//! Mirrors `DBManager.kt` PostgreSQL path. All tables live in the
//! `suwayomi` schema (matches M0054).
//!
//! Backend selection (Phase 6):
//! - **Embedded (default)** — `Db::connect_embedded`: an in-process
//!   Oliphaunt native PostgreSQL server (the renamed pglite-oxide, running a
//!   real local PostgreSQL 18 process). No external server, Docker or install
//!   step. Unlike the old pglite-oxide WASI gateway (single serial session,
//!   proxy terminated on any SQL error), the native server exposes
//!   independent client sessions, so the pool can use multiple connections
//!   and SQL errors never kill the server.
//! - **External PostgreSQL (fallback)** — `Db::connect`: connects to a
//!   standalone PostgreSQL server through a regular `postgres://` URL.

use std::path::Path;
use std::sync::Arc;

use oliphaunt::Oliphaunt;
use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::db::migrator::MIGRATOR;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("embedded oliphaunt error: {0}")]
    Embedded(#[from] anyhow::Error),
}

/// Which database backend the server is running on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbMode {
    /// In-process Oliphaunt native server (default) — no external server required.
    Embedded,
    /// External PostgreSQL server via connection URL.
    External,
}

/// Database handle shared across the app (cloned per request via `Arc`).
#[derive(Clone)]
pub struct Db {
    pool: PgPool,
    /// Keeps the embedded Oliphaunt native PostgreSQL server alive for the
    /// lifetime of the app. `None` in external mode. `Arc` keeps `Db` cheap
    /// to clone; the server shuts down (via `Drop`) when the last clone is
    /// dropped.
    _embedded: Option<Arc<Oliphaunt>>,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db")
            .field("mode", &self.mode())
            .finish_non_exhaustive()
    }
}

/// Shared pool hardening — mirrors `DBManager.kt` + guards against stale
/// connections:
/// - `test_before_acquire` — ping before handing out a connection.
/// - `idle_timeout` — reclaim idle connections so long-sitting clients are
///   closed by us, not by the server at an arbitrary moment.
/// - `max_lifetime` — hard ceiling against connection rot.
fn hardened(options: PgPoolOptions) -> PgPoolOptions {
    options
        .test_before_acquire(true)
        .idle_timeout(std::time::Duration::from_secs(300))
        .max_lifetime(std::time::Duration::from_secs(3600))
        .acquire_timeout(std::time::Duration::from_secs(30))
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
        let pool = hardened(PgPoolOptions::new().max_connections(64))
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    sqlx::query("SET search_path TO suwayomi").execute(conn).await?;
                    Ok(())
                })
            })
            .connect(url)
            .await?;
        Ok(Self { pool, _embedded: None })
    }

    /// Connect to an embedded Oliphaunt native PostgreSQL server (default
    /// backend).
    ///
    /// `data_dir`:
    /// - `Some(path)` — persistent database rooted at `path` (created on
    ///   first open via initdb, reused on later starts).
    /// - `None` — ephemeral temporary database (fresh per process; ideal
    ///   for tests).
    ///
    /// The native server accepts independent client sessions, so the pool
    /// is configured with multiple connections (up to the server's
    /// `max_client_sessions`).
    pub async fn connect_embedded(data_dir: Option<&Path>) -> Result<Self, DbError> {
        // Register the native runtime/broker artifact tree staged by
        // oliphaunt-build (idempotent for the same path).
        oliphaunt::register_build_resources!()
            .map_err(|e| DbError::Embedded(anyhow::Error::new(e)))?;
        // Workaround for oliphaunt 0.1.1 on Windows: `materialize_runtime`
        // copies the runtime tools (exe) and lib/share trees into its cache,
        // but omits the bin/*.dll files that the PostgreSQL binaries link
        // against, so the first initdb fails with STATUS_DLL_NOT_FOUND
        // (0xc0000135). Copy the DLLs from the staged resources into every
        // existing cache bin; the first materialization creates the cache,
        // so a failed open is retried once after copying.
        //
        // Also retried after cleaning a stale postmaster: if the previous
        // process was killed without graceful shutdown (crash / taskkill /F),
        // the oliphaunt postgres child keeps running and the next start fails
        // on the `postmaster.pid` lock — remove that leftover and retry.
        let open = || async {
            let builder = Oliphaunt::builder().native_server().max_client_sessions(32);
            match data_dir {
                Some(dir) => builder.path(dir),
                None => builder.temporary(),
            }
            .open()
            .await
        };
        let server = match open().await {
            Ok(server) => server,
            Err(first) => {
                copy_runtime_bin_dlls_to_cache();
                cleanup_stale_postmaster(data_dir);
                open().await.map_err(|e| {
                    DbError::Embedded(anyhow::anyhow!(
                        "first attempt failed: {first}; retry after copying runtime DLLs and cleaning a stale postmaster failed: {e}"
                    ))
                })?
            }
        };
        let url = server
            .connection_string()
            .ok_or_else(|| DbError::Embedded(anyhow::anyhow!("embedded native server did not expose a connection string")))?;
        tracing::info!(%url, "embedded oliphaunt ready");
        let pool = hardened(PgPoolOptions::new().max_connections(64))
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    sqlx::query("SET search_path TO suwayomi").execute(conn).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await?;
        Ok(Self { pool, _embedded: Some(Arc::new(server)) })
    }

    /// Which backend this handle is running on.
    pub fn mode(&self) -> DbMode {
        if self._embedded.is_some() { DbMode::Embedded } else { DbMode::External }
    }

    /// Runs the schema migrations for the active backend.
    pub async fn migrate(&self) -> Result<(), DbError> {
        use sqlx::Executor;
        // Run the whole migration on ONE connection, serialized with other
        // concurrent migrators (e.g. parallel test binaries) via a PG
        // advisory lock. Without the lock, two `_sqlx_migrations` inserts on
        // the same version race each other ("tuple concurrently updated").
        let mut conn = self.pool.acquire().await?;
        // The `suwayomi` schema must exist before migration: sqlx's
        // `ensure_migrations_table` creates `_sqlx_migrations` with an
        // UNQUALIFIED name that resolves through search_path — on a fresh
        // database the missing schema would fail with 3F000.
        conn.execute("CREATE SCHEMA IF NOT EXISTS suwayomi").await?;
        conn.execute("SELECT pg_advisory_lock(728232364)").await?;
        let r = MIGRATOR.run(&mut conn).await;
        // SyncYomi version-bump triggers (PL/pgSQL) — supported by the real
        // PostgreSQL engine in both embedded (Oliphaunt native) and external
        // modes; idempotent: CREATE OR REPLACE + DROP TRIGGER IF EXISTS.
        let f = conn
            .execute(include_str!("../../../../migrations/pg-only/0002_sync_functions.sql"))
            .await;
        let t = conn
            .execute(include_str!("../../../../migrations/pg-only/0002_sync_triggers.sql"))
            .await;
        conn.execute("SELECT pg_advisory_unlock(728232364)").await?;
        r?;
        f.and(t)?;
        Ok(())
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

/// Workaround for oliphaunt 0.1.1 on Windows: the materialized runtime cache
/// (see `materialize_runtime` in oliphaunt's `liboliphaunt/root/runtime.rs`)
/// installs the PostgreSQL tools (exe) and the lib/share trees but omits the
/// `bin/*.dll` files that those executables link against (libpq.dll, …), so
/// initdb/postgres fail to start with STATUS_DLL_NOT_FOUND (0xc0000135).
///
/// The DLLs exist in the build-staged resources; copy them into every
/// existing cache `bin/` directory (idempotent). Called before opening the
/// server and again after a failed first open (the first materialization is
/// what creates the cache directories in the first place).
fn copy_runtime_bin_dlls_to_cache() {
    // Compile-time value emitted by oliphaunt-build (cargo:rustc-env), same
    // source used by the register_build_resources!() macro.
    let Some(resources_dir) = option_env!("OLIPHAUNT_RESOURCES_DIR") else {
        return;
    };
    let src_bin = std::path::Path::new(resources_dir)
        .join("native-runtime/liboliphaunt-native/runtime/bin");
    if !src_bin.is_dir() {
        return;
    }
    let cache_root = std::env::var_os("OLIPHAUNT_RUNTIME_CACHE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("oliphaunt-runtime-cache"));
    let Ok(cache_entries) = std::fs::read_dir(&cache_root) else {
        return;
    };
    let Ok(dlls) = std::fs::read_dir(&src_bin) else {
        return;
    };
    let dlls: Vec<_> = dlls
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dll")))
        .collect();
    for entry in cache_entries.flatten() {
        let bin = entry.path().join("bin");
        if !bin.is_dir() {
            continue;
        }
        for dll in &dlls {
            let target = bin.join(dll.file_name().unwrap_or_default());
            if !target.exists() {
                let _ = std::fs::copy(dll, &target);
            }
        }
    }
}

/// Remove a leftover postmaster from a previous run that was killed without
/// graceful shutdown. The embedded Oliphaunt postgres child survives such a
/// kill and holds `postmaster.pid`, so the next start fails with
/// "lock file postmaster.pid already exists". Only touches a postmaster
/// whose executable lives under the oliphaunt runtime cache (ours), never a
/// foreign PostgreSQL.
fn cleanup_stale_postmaster(data_dir: Option<&Path>) {
    use std::process::Command;

    let Some(dir) = data_dir else {
        return;
    };
    let pgdata = dir.join("pgdata");
    let pid_file = pgdata.join("postmaster.pid");
    let Ok(pid_text) = std::fs::read_to_string(&pid_file) else {
        return;
    };
    let Ok(pid) = pid_text.lines().next().unwrap_or("").trim().parse::<u32>() else {
        return;
    };
    // Windows: check the process image path contains our runtime cache.
    let image = Command::new("wmic")
        .args(["process", "where", &format!("ProcessId={pid}"), "get", "ExecutablePath", "/value"])
        .output()
        .ok();
    let is_ours = image
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .to_lowercase()
                .contains("oliphaunt-runtime-cache")
        })
        .unwrap_or(false);
    if !is_ours {
        tracing::warn!(pid, "postmaster.pid points to a foreign postgres; leaving it alone");
        return;
    }
    tracing::warn!(pid, "killing stale oliphaunt postmaster from an ungraceful shutdown");
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
    let _ = std::fs::remove_file(&pid_file);
    let _ = std::fs::remove_file(pgdata.join("postmaster.opts"));
}
