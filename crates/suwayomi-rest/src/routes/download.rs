//! Download REST endpoints — mirrors `DownloadController.kt`.
//! Queue management is wired to the (Phase 6) download manager; for now the
//! manager is a no-op that reports an empty stopped queue, keeping the API
//! contract and response shapes identical to Kotlin.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};

use crate::state::AppState;

fn empty_queue() -> serde_json::Value {
    serde_json::json!({ "queue": [], "state": "STOPPED" })
}

async fn start(State(_s): State<AppState>) -> Json<serde_json::Value> {
    Json(empty_queue())
}

async fn stop(State(_s): State<AppState>) -> Json<serde_json::Value> {
    Json(empty_queue())
}

async fn clear(State(_s): State<AppState>) -> Json<serde_json::Value> {
    Json(empty_queue())
}

async fn queue_chapter(State(_s): State<AppState>, Path((_manga_id, _chapter_index)): Path<(i32, i32)>) -> StatusCode {
    StatusCode::OK
}

async fn unqueue_chapter(
    State(_s): State<AppState>,
    Path((_manga_id, _chapter_index)): Path<(i32, i32)>,
) -> StatusCode {
    StatusCode::OK
}

async fn reorder_chapter(
    State(_s): State<AppState>,
    Path((_manga_id, _chapter_index, _to)): Path<(i32, i32, i32)>,
) -> StatusCode {
    StatusCode::OK
}

async fn queue_batch(State(_s): State<AppState>) -> StatusCode {
    StatusCode::OK
}

async fn unqueue_batch(State(_s): State<AppState>) -> StatusCode {
    StatusCode::OK
}

pub fn downloads_router() -> Router<AppState> {
    Router::new().route("/start", get(start)).route("/stop", get(stop)).route("/clear", get(clear))
}

pub fn download_router() -> Router<AppState> {
    Router::new()
        .route("/{mangaId}/chapter/{chapterIndex}", get(queue_chapter).delete(unqueue_chapter))
        .route("/{mangaId}/chapter/{chapterIndex}/reorder/{to}", axum::routing::patch(reorder_chapter))
        .route("/batch", axum::routing::post(queue_batch).delete(unqueue_batch))
}
