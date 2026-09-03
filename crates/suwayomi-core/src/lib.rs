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

/// Build metadata derived by `build.rs` (commit-count based versioning).
pub mod version {
    /// Version name — `r{versionCode}` for auto/local builds (e.g. `r3064`),
    /// or the `SUWAYOMI_VERSION_NAME` value injected by release CI.
    pub const VERSION: &str = env!("SUWAYOMI_VERSION_NAME");
    /// Internal version code — commit count + 3000 (string, parse at use site).
    pub const VERSION_CODE: &str = env!("SUWAYOMI_VERSION_CODE");
    /// Commit count at build time (string, parse at use site).
    pub const VERSION_COUNT: &str = env!("SUWAYOMI_VERSION_COUNT");
    /// Build time as Unix epoch seconds (string, parse at use site).
    pub const BUILD_TIME_EPOCH_SECS: &str = env!("SUWAYOMI_BUILD_TIME");
    /// Release channel — `alpha` / `beta` / `release` (injected by release CI,
    /// default `release`). Reported to the WebUI as `aboutServer.buildType`.
    pub const BUILD_TYPE: &str = env!("SUWAYOMI_BUILD_TYPE");
}

