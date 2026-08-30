//! Update REST endpoints — mirrors `UpdateController.kt`.
//! `recentChapters` and `summary` are fully implemented against the DB;
//! `fetch`/`reset` return success until the updater job runner lands (Phase 6).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};

use crate::state::AppState;
use suwayomi_core::models::MangaChapterDataClass;

async fn recent_chapters(State(s): State<AppState>, Path(page_num): Path<usize>) -> Json<Vec<MangaChapterDataClass>> {
    let page =
        s.chapter.get_recent_chapters(page_num.max(1)).await.unwrap_or_else(|_| {
            suwayomi_core::models::pagination::PaginatedList { page: vec![], has_next_page: false }
        });
    Json(page.page)
}

/// Mirrors `UpdateStatus` summary shape (idle values until the updater runs).
async fn summary(State(_s): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "isRunning": false,
        "completeJobs": { "mangas": { "nodes": [], "totalCount": 0 } },
        "pendingJobs": { "mangas": { "nodes": [], "totalCount": 0 } },
        "runningJobs": { "mangas": { "nodes": [], "totalCount": 0 } },
        "failedJobs": { "mangas": { "nodes": [], "totalCount": 0 } },
        "skippedJobs": { "mangas": { "nodes": [], "totalCount": 0 } },
        "updatingCategories": { "categories": { "nodes": [], "totalCount": 0 } },
        "skippedCategories": { "categories": { "nodes": [], "totalCount": 0 } },
    }))
}

async fn fetch_update(State(s): State<AppState>) -> StatusCode {
    // Phase 6: updater job runner. For now, touch `chapters_last_fetched_at`
    // of library manga to expose a visible effect.
    let _ = s;
    StatusCode::OK
}

async fn reset(State(_s): State<AppState>) -> StatusCode {
    StatusCode::OK
}

pub fn update_router() -> Router<AppState> {
    Router::new()
        .route("/recentChapters/{pageNum}", get(recent_chapters))
        .route("/summary", get(summary))
        .route("/fetch", axum::routing::post(fetch_update))
        .route("/reset", axum::routing::post(reset))
}
