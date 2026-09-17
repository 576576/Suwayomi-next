//! Extension endpoints — mirrors `ExtensionController.kt`. Install/update/
//! uninstall drive the JVM sandbox hot reload + source registration.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

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
    let rows = suwayomi_db::query("SELECT * FROM extension ORDER BY name ASC")
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

/// `GET /api/v1/extension/icon/{pkg}` —— 扩展图标。
///
/// 来源按代价从低到高：磁盘缓存 `<cache>/extensions/icons/` → 沙盒（扩展自己 APK
/// 里那张，`GET /icon/{pkg}`）→ `extension.icon_url`。
///
/// 沙盒排在 `icon_url` 之前：后者的列 DEFAULT 是个早已 404 的占位 URL
/// （`migrations/0001_schema_baseline.sql`），而系统装进来的扩展没有仓库索引行去
/// 盖掉它，该字段于是恒为死链。
///
/// 三条路都要求**真的图片字节**（`looks_like_image`），否则 404 正文会被当成图标
/// 写进缓存，并被后续请求一直"命中"。
async fn icon(State(s): State<AppState>, Path(pkg): Path<String>) -> ApiResult<axum::response::Response> {
    // 磁盘缓存：<cache>/extensions/icons/{pkg}.{png|jpg|webp}（按内容类型定扩展名）
    let cache_dir = crate::routes::cache_root().join("extensions").join("icons");

    // 缓存命中同样要过魔数，否则写坏的文件会一直命中。
    let mut bytes: Option<Vec<u8>> = None;
    for ext in ["png", "jpg", "webp"] {
        if let Ok(cached) = std::fs::read(cache_dir.join(format!("{pkg}.{ext}")))
            && looks_like_image(&cached)
        {
            bytes = Some(cached);
            break;
        }
    }

    if bytes.is_none()
        && let Some(base) = &s.sandbox_base
    {
        bytes = suwayomi_domain::source::sandbox::HttpSandboxFetcher::new(base.clone()).icon(&pkg).await;
    }

    if bytes.is_none() {
        let icon_url: Option<String> =
            suwayomi_db::query_scalar("SELECT icon_url FROM extension WHERE pkg_name = $1")
                .bind(&pkg)
                .fetch_optional(s.db.pool())
                .await
                .map_err(ApiError::from)?;
        if let Some(url) = icon_url.filter(|u| !u.is_empty()) {
            match reqwest::get(&url).await {
                // 死链会把 404 正文喂回来，状态码与魔数都要过。
                Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                    Ok(b) if looks_like_image(&b) => bytes = Some(b.to_vec()),
                    Ok(b) => tracing::debug!("icon_url for {pkg} is not an image ({} bytes)", b.len()),
                    Err(e) => tracing::warn!("icon_url read for {pkg} failed: {e}"),
                },
                Ok(resp) => tracing::debug!("icon_url for {pkg} returned {}", resp.status()),
                Err(e) => tracing::warn!("icon_url fetch for {pkg} failed: {e}"),
            }
        }
    }

    let bytes = bytes.ok_or_else(|| ApiError::NotFound("no icon".into()))?;

    // 写失败不致命（下次回源），但记一条，别让缓存静默失效。
    if let Err(e) = std::fs::create_dir_all(&cache_dir) {
        tracing::debug!("cannot create icon cache {}: {e}", cache_dir.display());
    }
    let ext = match guess_content_type(&bytes) {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    };
    if let Err(e) = std::fs::write(cache_dir.join(format!("{pkg}.{ext}")), &bytes) {
        tracing::debug!("cannot cache icon for {pkg}: {e}");
    }

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

/// 这批字节是不是一张图片 —— 判据直接复用 `guess_content_type` 的兜底值
/// （`image/*` 表示没认出来），省得两处魔数判断各写一份而漂移。
fn looks_like_image(bytes: &[u8]) -> bool {
    guess_content_type(bytes) != "image/*"
}
