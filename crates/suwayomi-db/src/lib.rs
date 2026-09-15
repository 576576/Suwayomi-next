//! Dual-backend database layer.
//!
//! * **Default** — a local SQLite file driven through `rheos-tokio-rusqlite`
//!   (a `rusqlite` connection pinned to its own OS thread). No external server,
//!   no subprocess: the database is a single file next to the data directory.
//! * **Optional** — an external PostgreSQL server through `tokio-postgres`
//!   behind a `deadpool` pool, selected with `SUWAYOMI_DB_BACKEND=postgres` or
//!   by setting `SUWAYOMI_DATABASE_URL`.
//!
//! Callers write ordinary SQL with `?` placeholders and go through the same
//! builders the tree already used with sqlx:
//!
//! ```no_run
//! # async fn demo(db: suwayomi_db::Db) -> suwayomi_db::Result<()> {
//! let id: i32 = suwayomi_db::query_scalar("SELECT id FROM manga WHERE title = ?")
//!     .bind("Berserk")
//!     .fetch_one(&db)
//!     .await?;
//! # let _ = id; Ok(()) }
//! ```
//!
//! The differences that leak to callers are deliberate and small: the backend
//! is chosen at runtime, and `crate::dialect` rewrites each statement for it
//! (placeholder style, `= ANY(?)` arrays, `ILIKE`, the `suwayomi.` prefix).

pub mod backend;
pub mod config;
pub mod dialect;
pub mod error;
pub mod from_row;
pub mod migrator;
pub mod query;
pub mod row;
pub mod value;

pub use backend::{BackendKind, Db};
pub use config::DbSettings;
pub use error::{Error, Result};
pub use from_row::FromRow;
pub use query::{Executor, Query, QueryAs, QueryResult, QueryScalar, query, query_as, query_scalar};
pub use row::Row;
pub use value::{Decode, Encode, Value};

/// Helpers shared by the workspace's database-backed test suites. Not part of
/// the public API.
///
/// The integration tests point at *one* external PostgreSQL database and each
/// starts by truncating the tables it owns. Without agreement two of them
/// deadlock (one truncating while another migrates, taking `AccessExclusiveLock`
/// on the same relations in a different order) or wipe each other's fixtures
/// mid-test. Both directions have to be covered:
///
/// * **within a binary** — `#[tokio::test]` runs a binary's tests on parallel
///   threads, so a `tokio` mutex serialises them;
/// * **across binaries** — `cargo test --workspace` runs the test binaries of
///   different crates as parallel processes, which a mutex cannot see. A
///   lock file in the temp directory does.
///
/// Every test that touches the shared database holds the returned guard for its
/// whole body.
#[doc(hidden)]
pub mod test_support {
    /// Held for the duration of one database-backed test (see [`db_lock`]).
    #[must_use]
    pub struct DbLock {
        /// Released when dropped (closing the handle releases the OS lock).
        _file: std::fs::File,
        _thread: tokio::sync::OwnedMutexGuard<()>,
    }

    /// Serialises database-backed tests, across threads *and* processes.
    pub async fn db_lock() -> DbLock {
        static LOCK: std::sync::OnceLock<std::sync::Arc<tokio::sync::Mutex<()>>> = std::sync::OnceLock::new();
        // In-process first: the file lock below blocks a worker thread, and
        // taking the mutex first keeps that wait off the async runtime except
        // when another *process* is holding the lock.
        let thread = LOCK.get_or_init(|| std::sync::Arc::new(tokio::sync::Mutex::new(()))).clone().lock_owned().await;
        let file = tokio::task::spawn_blocking(|| {
            let path = std::env::temp_dir().join("suwayomi-db-tests.lock");
            let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path).expect("open test lock file");
            file.lock().expect("lock test lock file");
            file
        })
        .await
        .expect("lock task panicked");
        DbLock { _file: file, _thread: thread }
    }
}
