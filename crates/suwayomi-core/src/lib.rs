//! suwayomi-core
//!
//! Mirrors the following Suwayomi Kotlin packages:
//! - `suwayomi.manga.model.*`       → models / schema
//! - `suwayomi.server.database.*`   → db
//! - `eu.kanade.tachiyomi.source.model.*`     → source
//! - `suwayomi.server.settings.*`   → config

// 测试代码允许 panic：unwrap / expect / panic! 在断言里是常规写法，
// 逐个改成 `?` 传播只会让失败信息更难读。生产代码不受这条影响
// （`cfg_attr(test, ...)`）。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::todo,
        clippy::indexing_slicing
    )
)]

pub mod auth;
pub mod backup;
pub mod config;
pub mod db;
pub mod models;
pub mod schema;
pub mod source;
pub mod text;

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
