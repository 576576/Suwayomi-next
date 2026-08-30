//! Database layer — mirrors `suwayomi.tachidesk.server.database.*`.
//! PostgreSQL only.

pub mod manager;
pub mod migrator;

pub use manager::{Db, DbError};
