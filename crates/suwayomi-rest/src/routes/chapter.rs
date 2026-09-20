//! Chapter endpoints — mirrors `MangaController.kt` chapter routes.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// `markAsRead` 在 query 上（上游是 `queryParam<Boolean?>`）。
#[derive(serde::Deserialize, Default)]
pub struct DownloadParams {
    #[serde(rename = "markAsRead")]
    pub mark_as_read: Option<bool>,
}

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
    if change.delete == Some(true)
        && let Some(ids) = &ids
    {
        s.chapter.delete_chapters(ids).await?;
    }
    if let Some(ids) = &ids {
        s.chapter.modify_chapters_by_ids(ids, change.is_read, change.is_bookmarked, change.last_page_read, true).await?;
    }
    Ok(Json(serde_json::json!({ "message": "success" })))
}

/// Mirrors `MangaController.downloadChapter` — 把已经落盘的 CBZ 发出去。`HEAD`
/// 只回头，客户端用它拿文件名与大小；`GET` 带 `markAsRead=true` 时顺带把这章
/// 标记为已读。
async fn download(
    State(s): State<AppState>,
    Path(chapter_id): Path<i32>,
    Query(q): Query<DownloadParams>,
    method: Method,
) -> ApiResult<Response> {
    let chapter = s.chapter.fetch_by_id(chapter_id).await?;

    if method == Method::GET && q.mark_as_read == Some(true) {
        s.chapter.modify_chapters_by_ids(&[chapter_id], Some(true), None, None, true).await?;
    }

    // `real_url` 是下载时写下的 CBZ 落盘路径；没下载过的章节它是空的。
    let archive = chapter
        .real_url
        .clone()
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| ApiError::NotFound(format!("chapter {chapter_id} has not been downloaded")))?;
    let meta = tokio::fs::metadata(&archive)
        .await
        .map_err(|_| ApiError::NotFound(format!("chapter {chapter_id} archive is missing on disk")))?;
    if !meta.is_file() {
        return Err(ApiError::NotFound(format!("chapter {chapter_id} archive is missing on disk")));
    }

    let file_name = std::path::Path::new(&archive)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{}.cbz", chapter.name));
    let mime = s.config.snapshot().opds_cbz_mimetype.media_type();

    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        Body::from(tokio::fs::read(&archive).await.map_err(|e| ApiError::Internal(e.to_string()))?)
    };
    let mut resp = Response::builder()
        .status(StatusCode::OK)
        .body(body)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let headers = resp.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{file_name}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    if let Ok(v) = HeaderValue::from_str(&meta.len().to_string()) {
        headers.insert(header::CONTENT_LENGTH, v);
    }
    Ok(resp)
}
