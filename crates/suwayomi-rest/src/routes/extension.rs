//! Extension endpoints — mirrors `ExtensionController.kt` (DB-backed parts;
//! install/uninstall need the JVM sandbox, Phase 5).

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use sqlx::Row;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/list", get(list))
        .route("/icon/{pkg_name}", get(icon))
        .route("/install/{pkg_name}", get(install))
        .route("/update/{pkg_name}", get(update))
        .route("/uninstall/{pkg_name}", get(uninstall))
}

async fn list(State(s): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let rows = sqlx::query("SELECT * FROM extension ORDER BY name ASC")
        .fetch_all(s.db.pool())
        .await
        .map_err(ApiError::from)?;
    let out: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "apkName": r.try_get::<Option<String>, _>("apk_name").unwrap_or_default(),
                "iconUrl": r.try_get::<String, _>("icon_url").unwrap_or_default(),
                "name": r.try_get::<String, _>("name").unwrap_or_default(),
                "pkgName": r.try_get::<String, _>("pkg_name").unwrap_or_default(),
                "versionName": r.try_get::<String, _>("version_name").unwrap_or_default(),
                "versionCode": r.try_get::<i64, _>("version_code").unwrap_or(0),
                "lang": r.try_get::<String, _>("lang").unwrap_or_default(),
                "isNsfw": false,
                "installed": r.try_get::<bool, _>("is_installed").unwrap_or(false),
                "hasUpdate": r.try_get::<bool, _>("has_update").unwrap_or(false),
                "obsolete": r.try_get::<bool, _>("is_obsolete").unwrap_or(false),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "extensions": out })))
}

async fn icon(State(_s): State<AppState>, Path(_pkg): Path<String>) -> ApiResult<axum::response::Response> {
    Err(ApiError::NotFound("icon streaming unavailable in this phase".into()))
}

async fn install(State(_s): State<AppState>, Path(_pkg): Path<String>) -> ApiResult<Json<serde_json::Value>> {
    // extension install requires the JVM sandbox (Phase 5)
    Err(ApiError::Internal("extension install requires the JVM sandbox (Phase 5)".into()))
}

async fn update(State(_s): State<AppState>, Path(_pkg): Path<String>) -> ApiResult<Json<serde_json::Value>> {
    Err(ApiError::Internal("extension update requires the JVM sandbox (Phase 5)".into()))
}

async fn uninstall(State(_s): State<AppState>, Path(_pkg): Path<String>) -> ApiResult<Json<serde_json::Value>> {
    Err(ApiError::Internal("extension uninstall requires the JVM sandbox (Phase 5)".into()))
}
