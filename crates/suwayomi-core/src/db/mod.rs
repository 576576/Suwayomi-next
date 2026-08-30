//! Database layer — mirrors `suwayomi.tachidesk.server.database.*`.
//!
//! Backends (Phase 6): embedded PGlite by default (`Db::connect_embedded`,
//! in-process, no external server), external PostgreSQL as fallback
//! (`Db::connect` with a `postgres://` URL).

pub mod manager;
pub mod migrator;

pub use manager::{Db, DbError, DbMode};
