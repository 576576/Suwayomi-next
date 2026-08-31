//! Track REST endpoints — mirrors `TrackController.kt`.
//! `/api/v1/track/list` is fully implemented (built-in tracker registry);
//! login/search/bind/update return success until tracker services land (Phase 6).
//! `/api/v1/track/{id}/thumbnail` proxies the tracker's official logo with
//! on-disk caching under `<cache>/trackers/` (fallback: embedded placeholder).

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};

use crate::state::AppState;

/// Mirrors `TrackerDataClass`.
#[derive(serde::Serialize)]
pub struct TrackerDataClass {
    pub id: i32,
    pub name: String,
    pub icon: String,
    pub is_login: bool,
    pub auth_url: Option<String>,
}

/// Built-in tracker registry (mirrors `TrackerManager.services`).
/// 4th tuple element = official logo URL (None → embedded placeholder).
const TRACKERS: &[(i32, &str, Option<&str>, Option<&str>)] = &[
    (1, "MyAnimeList", Some("https://myanimelist.net/"), Some("https://cdn.myanimelist.net/img/sp/icon/apple-touch-icon-256.png")),
    (2, "Anilist", Some("https://anilist.co/api/v2/oauth/authorize"), Some("https://anilist.co/img/logo_al.png")),
    (3, "Kitsu", None, None),
    (4, "Shikimori", Some("https://shikimori.one/oauth/authorize"), None),
    (5, "Bangumi", Some("https://bgm.tv/oauth/authorize"), Some("https://bgm.tv/img/logo.png")),
    (7, "MangaUpdates", None, None),
];

/// 内嵌占位图标（128×128 圆角深灰蓝方块，下载不可用时兜底）。
const PLACEHOLDER_PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAIAAAACACAYAAADDPmHLAAABDUlEQVR42u3RAQ2AQBADwROJDgSgnvdAYD9kmlXQmXmw47y0mm/m6IzEoRmDEzMGx5UM/ioNPFUa+Kg08E5s4JoSwC+xgVNKAI/EBu4oAXwRGzgCAAABAKACwAuxgQsAABAAAAIAQAAACAAAAQAgAAAEAIAAABAAAAIAQAAACAAAAQAgAAAEAIAAABAAAAIAQAAACAAAAQAgAAAEAIAAABAAAAIAQAAACAAAAQAgAAAEAIAAABAAAAIAQAAACAAAAQAgAAAEAAAALwAAIAAABACAAAAQAAACAEAAAAgAAAEAIAAABOCXAGteKN8HAACALwAAUAfAIH4fQA/AIH4fQA/AIH4fQA/AIH6fQf8+g/59Bv37GPrrMWxxPYYtrkfy3t03wO75FXwW2NIAAAAASUVORK5CYII=";

fn placeholder_png() -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(PLACEHOLDER_PNG_B64)
        .unwrap_or_default()
}

fn tracker_list() -> Vec<TrackerDataClass> {
    TRACKERS
        .iter()
        .map(|(id, name, auth, _icon)| TrackerDataClass {
            id: *id,
            name: name.to_string(),
            icon: format!("/api/v1/track/{id}/thumbnail"),
            is_login: false,
            auth_url: auth.map(|s| s.to_string()),
        })
        .collect()
}

async fn list(State(_s): State<AppState>) -> Json<Vec<TrackerDataClass>> {
    Json(tracker_list())
}

async fn login(State(_s): State<AppState>) -> StatusCode {
    // Phase 6: tracker credential login; returns OK without side effects for now.
    StatusCode::OK
}

async fn logout(State(_s): State<AppState>) -> StatusCode {
    StatusCode::OK
}

async fn search(State(_s): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "trackSearches": [] }))
}

async fn bind(State(_s): State<AppState>) -> StatusCode {
    StatusCode::OK
}

async fn update(State(_s): State<AppState>) -> StatusCode {
    StatusCode::OK
}

async fn thumbnail(State(_s): State<AppState>, axum::extract::Path(id): axum::extract::Path<i32>) -> axum::response::Response {
    use axum::response::IntoResponse;

    let cache_dir = crate::routes::cache_root().join("trackers");
    let cached: Option<Vec<u8>> = {
        let mut found = None;
        for ext in ["png", "jpg", "webp"] {
            let cand = cache_dir.join(format!("{id}.{ext}"));
            if cand.is_file() {
                found = std::fs::read(&cand).ok();
                break;
            }
        }
        found
    };
    let bytes = match cached {
        Some(b) => b,
        None => {
            let url = TRACKERS.iter().find(|t| t.0 == id).and_then(|t| t.3);
            let fetched = match url {
                Some(u) => {
                    match reqwest::get(u).await {
                        Ok(resp) => resp.bytes().await.ok().map(|b| b.to_vec()),
                        Err(_) => None,
                    }
                }
                None => None,
            };
            match fetched {
                Some(b) => {
                    let _ = std::fs::create_dir_all(&cache_dir);
                    let ext = if b.len() > 3 && &b[0..4] == b"RIFF" {
                        "webp"
                    } else if b.len() > 2 && b[0] == 0xff && b[1] == 0xd8 {
                        "jpg"
                    } else {
                        "png"
                    };
                    let _ = std::fs::write(cache_dir.join(format!("{id}.{ext}")), &b);
                    b
                }
                None => placeholder_png(),
            }
        }
    };
    let ctype = if bytes.len() > 3 && &bytes[0..4] == b"RIFF" {
        "image/webp"
    } else if bytes.len() > 2 && bytes[0] == 0xff && bytes[1] == 0xd8 {
        "image/jpeg"
    } else {
        "image/png"
    };
    ([(axum::http::header::CONTENT_TYPE, ctype)], bytes).into_response()
}

pub fn track_router() -> Router<AppState> {
    Router::new()
        .route("/list", get(list))
        .route("/login", axum::routing::post(login))
        .route("/logout", axum::routing::post(logout))
        .route("/search", axum::routing::post(search))
        .route("/bind", axum::routing::post(bind))
        .route("/update", axum::routing::post(update))
        .route("/{trackerId}/thumbnail", get(thumbnail))
}
