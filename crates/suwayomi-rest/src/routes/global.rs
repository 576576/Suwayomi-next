//! Global endpoints — mirrors `GlobalAPI.kt` (meta / settings / webview).

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

use super::meta_handler;
use crate::error::{ApiError, ApiResult};
use suwayomi_api::AppState;

pub fn meta_router() -> Router<AppState> {
    Router::new().route("/", get(get_meta).patch(modify_meta))
}

pub fn settings_router() -> Router<AppState> {
    Router::new().route("/about", get(about)).route("/check-update", get(check_update))
}

pub fn webview_router() -> Router<AppState> {
    Router::new().route("/", get(webview))
}

async fn get_meta(State(s): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let map = meta_handler::get_global_meta(&s).await?;
    Ok(Json(serde_json::to_value(&map).map_err(|e| ApiError::Internal(e.to_string()))?))
}

async fn modify_meta(
    State(s): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> ApiResult<Json<serde_json::Value>> {
    let key = body.get("key").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    let value = body.get("value").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    meta_handler::set_global_meta(&s, key, value).await?;
    Ok(Json(serde_json::json!({ "message": "success" })))
}

async fn about(State(_s): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "name": "Suwayomi (next)",
        // 真实版本（`core/build.rs` 由 commit count 推导，alpha 为 `r{code}`、
        // release/beta 为 CI 注入的 3.y.z）。**不能写 `env!("CARGO_PKG_VERSION")`**
        // ——那是本 crate 的 Cargo.toml 版本，恒为 workspace 的 0.1.0，会让同一个
        // server 在 REST 与 GraphQL（`aboutServer.version` 读的就是这里这个常量）
        // 报出两个不同的版本号。
        "version": suwayomi_core::version::VERSION,
        "serverInitialized": true,
    })))
}

async fn check_update(State(_s): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({ "hasUpdate": false, "version": null, "releaseDate": null })))
}

async fn webview(State(_s): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    // WebView (KCEF) removed per R3 — endpoint kept for compatibility
    Err(ApiError::NotFound("webview removed (R3: CEF → Tauri)".into()))
}
