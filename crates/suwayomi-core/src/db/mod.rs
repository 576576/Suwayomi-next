//! Database layer.
//!
//! The implementation lives in the `suwayomi-db` crate (SQLite by default via
//! `rheos-tokio-rusqlite`, external PostgreSQL via `tokio-postgres`); this
//! module keeps the historical `suwayomi_core::db::*` import paths working.
//!
//! `Db::pool()` returns the handle itself, so the many call sites that pass
//! `db.pool()` to a query builder keep compiling unchanged.

pub use suwayomi_db::{
    BackendKind, Db, DbSettings, Error as DbError, Executor, FromRow, Query, QueryAs, QueryResult, QueryScalar,
    Result as DbResult, Row, Value, query, query_as, query_scalar,
};
