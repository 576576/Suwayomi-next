//! Extension endpoints — mirrors `ExtensionController.kt`. Install/update/
//! uninstall drive the JVM sandbox hot reload + source registration.

use axum::extract::{Path, State};
use axum::routing::{get, post};
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
        .route("/refresh", post(refresh))
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
                "isNsfw": r.try_get::<i32, _>("content_warning").unwrap_or(0) > 0,
                "installed": r.try_get::<bool, _>("is_installed").unwrap_or(false),
                "hasUpdate": r.try_get::<bool, _>("has_update").unwrap_or(false),
                "obsolete": r.try_get::<bool, _>("is_obsolete").unwrap_or(false),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "extensions": out })))
}

async fn icon(State(s): State<AppState>, Path(pkg): Path<String>) -> ApiResult<axum::response::Response> {
    let icon_url: Option<String> =
        sqlx::query_scalar("SELECT icon_url FROM extension WHERE pkg_name = $1")
            .bind(&pkg)
            .fetch_optional(s.db.pool())
            .await
            .map_err(ApiError::from)?;
    let url = icon_url.filter(|u| !u.is_empty()).ok_or_else(|| ApiError::NotFound("no icon".into()))?;

    // 磁盘缓存：<cache>/extensions/icons/{pkg}.{png|jpg|webp}（按内容类型定扩展名，
    // 避免回源下载）。统一缓存根见 cache_root()。
    let cache_dir = crate::routes::cache_root().join("extensions").join("icons");
    // 读取：依次尝试常见图标扩展名；均无则下载并落盘
    let mut bytes: Option<Vec<u8>> = None;
    for ext in ["png", "jpg", "webp"] {
        let cand = cache_dir.join(format!("{pkg}.{ext}"));
        if cand.is_file() {
            bytes = Some(std::fs::read(&cand).map_err(|e| ApiError::Internal(e.to_string()))?);
            break;
        }
    }
    let bytes = match bytes {
        Some(b) => b,
        None => {
            let resp = reqwest::get(&url).await.map_err(|e| ApiError::Internal(format!("icon fetch: {e}")))?;
            let b = resp.bytes().await.map_err(|e| ApiError::Internal(format!("icon read: {e}")))?;
            let b = b.to_vec();
            let _ = std::fs::create_dir_all(&cache_dir);
            let ext = match guess_content_type(&b) {
                "image/jpeg" => "jpg",
                "image/webp" => "webp",
                _ => "png",
            };
            let _ = std::fs::write(cache_dir.join(format!("{pkg}.{ext}")), &b);
            b
        }
    };
    let ctype = guess_content_type(&bytes);
    let resp = axum::response::Response::builder()
        .header("Content-Type", ctype)
        .body(axum::body::Body::from(bytes))
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(resp)
}

async fn install(State(s): State<AppState>, Path(pkg): Path<String>) -> ApiResult<Json<serde_json::Value>> {
    s.extension_store
        .install(&pkg)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "installed": true, "pkgName": pkg })))
}

async fn update(State(s): State<AppState>, Path(pkg): Path<String>) -> ApiResult<Json<serde_json::Value>> {
    s.extension_store
        .install(&pkg) // install() replaces the previous version's file
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "updated": true, "pkgName": pkg })))
}

async fn uninstall(State(s): State<AppState>, Path(pkg): Path<String>) -> ApiResult<Json<serde_json::Value>> {
    s.extension_store
        .uninstall(&pkg)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "uninstalled": true, "pkgName": pkg })))
}

/// POST /refresh — pulls every configured repo index into the extension table.
async fn refresh(State(s): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let n = s.extension_store.refresh_stores().await.map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "refreshed": n })))
}

fn guess_content_type(bytes: &[u8]) -> &'static str {
    if bytes.len() > 3 && bytes[0] == 0x89 && bytes[1] == b'P' && bytes[2] == b'N' && bytes[3] == b'G' {
        "image/png"
    } else if bytes.len() > 2 && bytes[0] == 0xff && bytes[1] == 0xd8 {
        "image/jpeg"
    } else if bytes.len() > 3 && &bytes[0..4] == b"RIFF" {
        "image/webp"
    } else {
        "image/*"
    }
}
