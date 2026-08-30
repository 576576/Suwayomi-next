//! Chapter endpoints — mirrors `MangaController.kt` chapter routes.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/batch", post(batch)).route("/{chapter_id}/download", get(download).head(download))
}

async fn batch(
    State(s): State<AppState>,
    Json(body): Json<super::manga::ChapterBatchBody>,
) -> ApiResult<Json<serde_json::Value>> {
    // mangaId-less batch edit by chapter ids
    let change = body.change;
    let ids = body.chapter_ids.clone();
    if change.delete == Some(true) {
        if let Some(ids) = &ids {
            s.chapter.delete_chapters(ids).await?;
        }
    }
    if let Some(ids) = &ids {
        s.chapter.modify_chapters_by_ids(ids, change.is_read, change.is_bookmarked, change.last_page_read).await?;
    }
    Ok(Json(serde_json::json!({ "message": "success" })))
}

async fn download(State(_s): State<AppState>, Path(_chapter_id): Path<i32>) -> ApiResult<axum::response::Response> {
    // chapter archive download lands in Phase 6
    Err(ApiError::NotFound("chapter download unavailable in this phase".into()))
}
