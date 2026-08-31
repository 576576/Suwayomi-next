//! Route registration — mirrors `MangaAPI.kt` + `GlobalAPI.kt` endpoint
//! layout under `/api/v1/`.

pub mod backup;
pub mod category;
pub mod chapter;
pub mod download;
pub mod extension;
pub mod global;
pub mod manga;
pub mod meta_handler;
pub mod source;
pub mod track;
pub mod update;

use axum::Router;

use crate::state::AppState;

/// 统一缓存根：`<发布根>/cache`（`SUWAYOMI_CACHE_DIR` 可覆盖），内分子目录
/// （extensions/icons、trackers 等）。发布布局 bin/suwayomi-server.exe 时根 =
/// exe 的上级；否则退回当前工作目录。
pub fn cache_root() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("SUWAYOMI_CACHE_DIR") {
        if !dir.is_empty() {
            return std::path::PathBuf::from(dir);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.file_name().map(|n| n == "bin").unwrap_or(false) {
                if let Some(base) = dir.parent() {
                    return base.join("cache");
                }
            }
        }
    }
    std::path::PathBuf::from("cache")
}

/// Builds the `/api/v1/**` router (OPDS & GraphQL mounted separately).
pub fn api_v1_router() -> Router<AppState> {
    Router::new()
        .nest("/extension", extension::router())
        .nest("/source", source::router())
        .nest("/manga", manga::router())
        .nest("/chapter", chapter::router())
        .nest("/category", category::router())
        .nest("/meta", global::meta_router())
        .nest("/settings", global::settings_router())
        .nest("/webview", global::webview_router())
        // Phase 6: implemented controllers (track/update/downloads backed by
        // DB or queue-manager contract; backup still stubbed until protobuf).
        .nest("/downloads", download::downloads_router())
        .nest("/download", download::download_router())
        .nest("/update", update::update_router())
        .nest("/track", track::track_router())
        .nest("/backup", backup::backup_router())
}
