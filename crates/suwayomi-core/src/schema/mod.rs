//! Database schema — mirrors `manga/model/table/*.kt` (Exposed tables).
//!
//! Row structs implement `sqlx::FromRow` for runtime (non-macro) queries so
//! no `DATABASE_URL` is required at compile time. Column/table names match the
//! migration baselines in `migrations/`.

pub mod rows;

pub use rows::*;
