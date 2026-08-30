//! Migration runner — mirrors `DBManager.databaseUp()`.
//!
//! Migrations are embedded at compile time (via `sqlx::migrate!`) so the
//! binary does not depend on a runtime `migrations/` directory.
//!
//! Compatibility note: the Kotlin version tracks applied migrations through
//! the `de.neonew.exposed.migrations` framework. The sqlx runner uses its own
//! `_sqlx_migrations` table. Phase 7's `tools/h2-dump` migrates *data* into a
//! freshly-migrated database, so the history tables never need to be shared.

use sqlx::migrate::Migrator;

/// PostgreSQL migrations (`migrations/` — baseline + future increments).
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),
}
