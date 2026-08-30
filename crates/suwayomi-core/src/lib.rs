//! suwayomi-core
//!
//! Mirrors the following Suwayomi-Server Kotlin packages:
//! - `suwayomi.tachidesk.manga.model.*`       → models / schema
//! - `suwayomi.tachidesk.server.database.*`   → db
//! - `eu.kanade.tachiyomi.source.model.*`     → source
//! - `suwayomi.tachidesk.server.settings.*`   → config

pub mod config;
pub mod db;
pub mod models;
pub mod schema;
pub mod source;
