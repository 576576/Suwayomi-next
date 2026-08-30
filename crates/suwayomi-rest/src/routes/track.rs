//! Track REST endpoints — mirrors `TrackController.kt`.
//! `/api/v1/track/list` is fully implemented (built-in tracker registry);
//! login/search/bind/update return success until tracker services land (Phase 6).

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
const TRACKERS: &[(i32, &str, Option<&str>)] = &[
    (1, "MyAnimeList", Some("https://myanimelist.net/")),
    (2, "Anilist", Some("https://anilist.co/api/v2/oauth/authorize")),
    (3, "Kitsu", None),
    (4, "Shikimori", Some("https://shikimori.one/oauth/authorize")),
    (5, "Bangumi", Some("https://bgm.tv/oauth/authorize")),
    (7, "MangaUpdates", None),
];

fn tracker_list() -> Vec<TrackerDataClass> {
    TRACKERS
        .iter()
        .map(|(id, name, auth)| TrackerDataClass {
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

async fn thumbnail(State(_s): State<AppState>) -> StatusCode {
    // No tracker icon assets in the Rust build yet (Phase 6 tracker service).
    StatusCode::NOT_FOUND
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
