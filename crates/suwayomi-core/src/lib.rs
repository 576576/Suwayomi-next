//! suwayomi-core
//!
//! Mirrors the following Suwayomi Kotlin packages:
//! - `suwayomi.manga.model.*`       → models / schema
//! - `suwayomi.server.database.*`   → db
//! - `eu.kanade.tachiyomi.source.model.*`     → source
//! - `suwayomi.server.settings.*`   → config

pub mod backup;
pub mod config;
pub mod db;
pub mod models;
pub mod schema;
pub mod source;
