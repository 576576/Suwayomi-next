//! Download REST endpoints — mirrors `DownloadController.kt`.
//! Wired to the real `DownloadManager` (queue + worker + event bus).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};

use suwayomi_domain::download::JobState;

use crate::state::AppState;

fn job_json(job: &suwayomi_domain::download::DownloadJob) -> serde_json::Value {
    serde_json::json!({
        "chapterId": job.chapter_id,
        "mangaId": job.manga_id,
        "mangaTitle": job.manga_title,
        "chapterName": job.chapter_name,
        "state": match job.state {
            JobState::Queued => "QUEUED",
            JobState::Downloading => "DOWNLOADING",
            JobState::Finished => "FINISHED",
            JobState::Error => "ERROR",
        },
        "progress": job.progress,
        "tries": job.tries,
    })
}

async fn queue_json(state: &AppState) -> serde_json::Value {
    let queue = state.download.snapshot().await;
    serde_json::json!({
        "queue": queue.iter().map(job_json).collect::<Vec<_>>(),
        "state": if state.download.is_running() { "STARTED" } else { "STOPPED" },
    })
}

async fn start(State(s): State<AppState>) -> Json<serde_json::Value> {
    s.download.start().await;
    Json(queue_json(&s).await)
}

async fn stop(State(s): State<AppState>) -> Json<serde_json::Value> {
    s.download.stop().await;
    Json(queue_json(&s).await)
}

async fn clear(State(s): State<AppState>) -> Json<serde_json::Value> {
    s.download.clear().await;
    Json(queue_json(&s).await)
}

/// Resolves (mangaId, chapterIndex=source_order) to a chapter id.
async fn chapter_id_by_index(state: &AppState, manga_id: i32, chapter_index: i32) -> Option<i32> {
    sqlx::query_scalar("SELECT id FROM chapter WHERE manga = $1 AND source_order = $2")
        .bind(manga_id)
        .bind(chapter_index)
        .fetch_optional(state.db.pool())
        .await
        .ok()
        .flatten()
}

async fn queue_chapter(
    State(s): State<AppState>,
    Path((manga_id, chapter_index)): Path<(i32, i32)>,
) -> StatusCode {
    match chapter_id_by_index(&s, manga_id, chapter_index).await {
        Some(cid) => match s.download.enqueue_chapter(cid).await {
            Ok(()) => StatusCode::OK,
            Err(_) => StatusCode::BAD_REQUEST,
        },
        None => StatusCode::NOT_FOUND,
    }
}

async fn unqueue_chapter(
    State(s): State<AppState>,
    Path((manga_id, chapter_index)): Path<(i32, i32)>,
) -> StatusCode {
    match chapter_id_by_index(&s, manga_id, chapter_index).await {
        Some(cid) => {
            let _ = s.download.dequeue_chapter(cid).await;
            StatusCode::OK
        }
        None => StatusCode::NOT_FOUND,
    }
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
