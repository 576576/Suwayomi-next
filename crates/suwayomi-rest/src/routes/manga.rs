//! Manga endpoints — mirrors `MangaController.kt`.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct OnlineParams {
    #[serde(default)]
    pub online_fetch: bool,
}

#[derive(Deserialize)]
pub struct MetaParams {
    pub key: String,
    pub value: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{manga_id}", get(get_manga))
        .route("/{manga_id}/full", get(get_manga_full))
        .route("/{manga_id}/thumbnail", get(get_thumbnail))
        .route("/{manga_id}/library", get(add_to_library).delete(remove_from_library))
        .route("/{manga_id}/meta", patch(modify_meta))
        .route("/{manga_id}/chapters", get(chapter_list))
        .route("/{manga_id}/chapter/batch", post(chapter_batch))
        .route(
            "/{manga_id}/chapter/{chapter_index}",
            get(chapter_retrieve).patch(chapter_modify).put(chapter_modify).delete(chapter_delete),
        )
        .route("/{manga_id}/chapter/{chapter_index}/meta", patch(chapter_meta))
        .route("/{manga_id}/chapter/{chapter_index}/page/{index}", get(page_retrieve))
        .route("/{manga_id}/chapter/{chapter_index}/page/{index}/image", get(page_image))
        .route("/{manga_id}/category", get(category_list))
        .route("/{manga_id}/category/{category_id}", get(add_to_category).delete(remove_from_category))
}

async fn get_manga(
    State(s): State<AppState>,
    Path(manga_id): Path<i32>,
    Query(q): Query<OnlineParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let dc = s.manga.get_manga(manga_id, q.online_fetch).await?;
    Ok(Json(serde_json::to_value(&dc).map_err(|e| ApiError::Internal(e.to_string()))?))
}

async fn get_manga_full(
    State(s): State<AppState>,
    Path(manga_id): Path<i32>,
    Query(q): Query<OnlineParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let dc = s.manga.get_manga_full(manga_id, q.online_fetch).await?;
    Ok(Json(serde_json::to_value(&dc).map_err(|e| ApiError::Internal(e.to_string()))?))
}

/// Mirrors `MangaController.thumbnail` — 取源站的封面图；先看磁盘缓存，
/// 没有就抓一次并落盘。带 `cache-control`，浏览器端缓存一天。
async fn get_thumbnail(
    State(s): State<AppState>,
    Path(manga_id): Path<i32>,
) -> crate::error::ApiResult<axum::response::Response> {
    use axum::http::{header, HeaderValue};

    let row: Option<suwayomi_core::schema::MangaRow> = suwayomi_db::query_as("SELECT * FROM manga WHERE id = $1")
        .bind(manga_id)
        .fetch_optional(s.db.pool())
        .await
        .map_err(ApiError::from)?;
    let url = row
        .and_then(|r| r.thumbnail_url)
        .filter(|u| !u.trim().is_empty())
        .ok_or_else(|| ApiError::NotFound(format!("manga {manga_id} has no thumbnail")))?;

    let (bytes, ctype) = load_thumbnail(manga_id, &url)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("manga {manga_id} thumbnail unavailable")))?;

    let mut resp = axum::response::Response::builder()
        .status(StatusCode::OK)
        .body(axum::body::Body::from(bytes))
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let headers = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&ctype) {
        headers.insert(header::CONTENT_TYPE, v);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("max-age=86400"));
    Ok(resp)
}

/// 封面字节：本地路径直接读，否则查缓存、再退到抓取。
async fn load_thumbnail(manga_id: i32, url: &str) -> Option<(Vec<u8>, String)> {
    let dir = suwayomi_core::config::cache_root().join("thumbnails");
    let img_path = dir.join(format!("{manga_id}.img"));
    let mime_path = dir.join(format!("{manga_id}.mime"));

    if let Ok(bytes) = tokio::fs::read(&img_path).await {
        let ctype = tokio::fs::read_to_string(&mime_path).await.unwrap_or_else(|_| "image/png".into());
        return Some((bytes, ctype));
    }

    // 已下载到本地的封面（扩展把图落了盘的情况）走文件读。
    if !url.starts_with("http://") && !url.starts_with("https://") {
        let bytes = tokio::fs::read(url).await.ok()?;
        let ctype = sniff_image_mime(&bytes).to_string();
        return Some((bytes, ctype));
    }

    let client = reqwest::Client::builder()
        .user_agent("Suwayomi-next/1.0")
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| "image/png".into());
    let bytes = resp.bytes().await.ok()?.to_vec();
    let _ = tokio::fs::create_dir_all(&dir).await;
    let _ = tokio::fs::write(&img_path, &bytes).await;
    let _ = tokio::fs::write(&mime_path, &ctype).await;
    Some((bytes, ctype))
}

fn sniff_image_mime(b: &[u8]) -> &'static str {
    if b.len() > 3 && &b[0..4] == b"RIFF" {
        "image/webp"
    } else if b.len() > 2 && b[0] == 0xff && b[1] == 0xd8 {
        "image/jpeg"
    } else if b.len() > 7 && &b[0..8] == b"\x89PNG\r\n\x1a\n" {
        "image/png"
    } else {
        "application/octet-stream"
    }
}

async fn add_to_library(State(s): State<AppState>, Path(manga_id): Path<i32>) -> ApiResult<Json<serde_json::Value>> {
    s.library.add_manga_to_library(manga_id).await?;
    Ok(Json(serde_json::json!({ "message": "success" })))
}

async fn remove_from_library(
    State(s): State<AppState>,
    Path(manga_id): Path<i32>,
) -> ApiResult<Json<serde_json::Value>> {
    s.library.remove_manga_from_library(manga_id).await?;
    Ok(Json(serde_json::json!({ "message": "success" })))
}

async fn modify_meta(
    State(s): State<AppState>,
    Path(manga_id): Path<i32>,
    Json(body): Json<MetaParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut map = std::collections::HashMap::new();
    map.insert(manga_id, std::collections::HashMap::from([(body.key, body.value)]));
    s.manga.modify_metas(&map).await?;
    Ok(Json(serde_json::json!({ "message": "success" })))
}

async fn chapter_list(
    State(s): State<AppState>,
    Path(manga_id): Path<i32>,
    Query(q): Query<OnlineParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let list = s.chapter.get_chapter_list(manga_id, q.online_fetch).await?;
    Ok(Json(serde_json::to_value(&list).map_err(|e| ApiError::Internal(e.to_string()))?))
}

#[derive(Deserialize)]
pub struct ChapterBatchBody {
    #[serde(default)]
    pub chapter_indexes: Option<Vec<i32>>,
    #[serde(default)]
    pub chapter_ids: Option<Vec<i32>>,
    pub change: ChapterChange,
}

#[derive(Deserialize)]
pub struct ChapterChange {
    #[serde(default)]
    pub is_read: Option<bool>,
    #[serde(default)]
    pub is_bookmarked: Option<bool>,
    #[serde(default)]
    pub last_page_read: Option<i32>,
    #[serde(default)]
    pub delete: Option<bool>,
}

async fn chapter_batch(
    State(s): State<AppState>,
    Path(manga_id): Path<i32>,
    Json(body): Json<ChapterBatchBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let change = body.change;
    let chapter_ids = body.chapter_ids.clone();
    let chapter_indexes = body.chapter_indexes.clone();

    if change.delete == Some(true) {
        if let Some(ids) = &chapter_ids {
            s.chapter.delete_chapters(ids).await?;
        } else if let Some(indexes) = &chapter_indexes {
            let ids = resolve_indexes(&s, manga_id, indexes).await;
            s.chapter.delete_chapters(&ids).await?;
        }
    }
    if let Some(indexes) = &chapter_indexes {
        s.chapter
            .modify_chapters_by_indexes(manga_id, indexes, change.is_read, change.is_bookmarked, change.last_page_read, true)
            .await?;
    }
    if let Some(ids) = &chapter_ids {
        s.chapter.modify_chapters_by_ids(ids, change.is_read, change.is_bookmarked, change.last_page_read, true).await?;
    }
    Ok(Json(serde_json::json!({ "message": "success" })))
}

async fn resolve_indexes(s: &AppState, manga_id: i32, indexes: &[i32]) -> Vec<i32> {
    let mut out = Vec::new();
    for idx in indexes {
        if let Ok(id) = find_chapter_id(s, manga_id, *idx).await {
            out.push(id);
        }
    }
    out
}

async fn find_chapter_id(s: &AppState, manga_id: i32, index: i32) -> Result<i32, suwayomi_domain::error::DomainError> {
    let sql = suwayomi_domain::sql::bind_placeholders("SELECT id FROM chapter WHERE manga = ? AND source_order = ?");
    suwayomi_db::query_scalar::<i32>(&sql)
        .bind(manga_id)
        .bind(index)
        .fetch_optional(s.db.pool())
        .await
        .map_err(suwayomi_domain::error::DomainError::Db)?
        .ok_or_else(|| suwayomi_domain::error::DomainError::NotFound("chapter not found".into()))
}

/// Resolves a chapter for the offline-page endpoints. The API parameter is
/// `source_order`, but rows written by older builds sometimes embedded the
/// chapter *number* (e.g. `/chapter/1/page/…` for a chapter whose
/// source_order is 0) into stored `image_url` values. Fall back to
/// `chapter_number` so those archives keep serving.
async fn find_chapter_id_offline(s: &AppState, manga_id: i32, index: i32) -> Result<i32, suwayomi_domain::error::DomainError> {
    if let Ok(id) = find_chapter_id(s, manga_id, index).await {
        return Ok(id);
    }
    let sql = suwayomi_domain::sql::bind_placeholders("SELECT id FROM chapter WHERE manga = ? AND chapter_number = ?");
    suwayomi_db::query_scalar::<i32>(&sql)
        .bind(manga_id)
        .bind(index)
        .fetch_optional(s.db.pool())
        .await
        .map_err(suwayomi_domain::error::DomainError::Db)?
        .ok_or_else(|| suwayomi_domain::error::DomainError::NotFound("chapter not found".into()))
}

async fn chapter_retrieve(
    State(s): State<AppState>,
    Path((manga_id, chapter_index)): Path<(i32, i32)>,
) -> ApiResult<Json<serde_json::Value>> {
    let id = find_chapter_id(&s, manga_id, chapter_index).await.map_err(ApiError::from)?;
    let row = s.chapter.fetch_by_id(id).await?;
    let dc = suwayomi_domain::manga::chapter_row_to_data_class(&row);
    Ok(Json(serde_json::to_value(&dc).map_err(|e| ApiError::Internal(e.to_string()))?))
}

#[derive(Deserialize)]
pub struct ChapterModifyBody {
    #[serde(default)]
    pub is_read: Option<bool>,
    #[serde(default)]
    pub is_bookmarked: Option<bool>,
    #[serde(default)]
    pub mark_prev_read: Option<bool>,
    #[serde(default)]
    pub last_page_read: Option<i32>,
}

async fn chapter_modify(
    State(s): State<AppState>,
    Path((manga_id, chapter_index)): Path<(i32, i32)>,
    Json(body): Json<ChapterModifyBody>,
) -> ApiResult<Json<serde_json::Value>> {
    s.chapter
        .modify_chapter(
            manga_id,
            chapter_index,
            body.is_read,
            body.is_bookmarked,
            body.mark_prev_read,
            body.last_page_read,
            true,
        )
        .await?;
    Ok(Json(serde_json::json!({ "message": "success" })))
}

async fn chapter_delete(
    State(s): State<AppState>,
    Path((manga_id, chapter_index)): Path<(i32, i32)>,
) -> ApiResult<Json<serde_json::Value>> {
    s.chapter.delete_chapter(manga_id, chapter_index).await?;
    Ok(Json(serde_json::json!({ "message": "success" })))
}

async fn chapter_meta(
    State(s): State<AppState>,
    Path((manga_id, chapter_index)): Path<(i32, i32)>,
    Json(body): Json<MetaParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let id = find_chapter_id(&s, manga_id, chapter_index).await.map_err(ApiError::from)?;
    let mut map = std::collections::HashMap::new();
    map.insert(id, std::collections::HashMap::from([(body.key, body.value)]));
    s.chapter.modify_metas(&map).await?;
    Ok(Json(serde_json::json!({ "message": "success" })))
}

async fn page_retrieve(
    State(s): State<AppState>,
    Path((manga_id, chapter_index, index)): Path<(i32, i32, i32)>,
) -> ApiResult<Json<serde_json::Value>> {
    let id = find_chapter_id_offline(&s, manga_id, chapter_index).await.map_err(ApiError::from)?;
    let page = s.page.get_page(id, index).await?;
    Ok(Json(serde_json::json!({ "index": page.index, "imageUrl": page.image_url })))
}

/// Serves the raw image bytes for a page of a downloaded archive (CBZ):
/// the chapter's `real_url` points at the archive on disk, the page row's
/// `url` holds the image file name inside it.
async fn page_image(
    State(s): State<AppState>,
    Path((manga_id, chapter_index, index)): Path<(i32, i32, i32)>,
) -> Response {
    let find = async {
        let cid = find_chapter_id_offline(&s, manga_id, chapter_index).await?;
        let sql = suwayomi_domain::sql::bind_placeholders("SELECT real_url FROM chapter WHERE id = ?");
        let real: Option<String> = suwayomi_db::query_scalar(&sql).bind(cid).fetch_optional(s.db.pool()).await?;
        let real = real.filter(|r| !r.is_empty()).ok_or_else(|| {
            suwayomi_domain::error::DomainError::NotFound("no archive for this chapter".into())
        })?;
        let page = s.page.get_page(cid, index).await?;
        let file_name = page.url;
        Ok::<_, suwayomi_domain::error::DomainError>((real, file_name))
    };
    let (archive, file_name) = match find.await {
        Ok(v) => v,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };
    let Some(bytes) = suwayomi_domain::source::local::read_archive_image(std::path::Path::new(&archive), &file_name)
    else {
        return (StatusCode::NOT_FOUND, "image not found in archive").into_response();
    };
    let mime = match std::path::Path::new(&file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        _ => "image/jpeg",
    };
    let mut resp = bytes.into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(mime) {
        resp.headers_mut().insert(axum::http::header::CONTENT_TYPE, v);
    }
    if let Ok(v) = axum::http::HeaderValue::from_str("public, max-age=31536000, immutable") {
        resp.headers_mut().insert(axum::http::header::CACHE_CONTROL, v);
    }
    resp
}

async fn category_list(State(s): State<AppState>, Path(manga_id): Path<i32>) -> ApiResult<Json<serde_json::Value>> {
    let list = s.category_manga.get_manga_categories(manga_id).await?;
    Ok(Json(serde_json::to_value(&list).map_err(|e| ApiError::Internal(e.to_string()))?))
}

async fn add_to_category(
    State(s): State<AppState>,
    Path((manga_id, category_id)): Path<(i32, i32)>,
) -> ApiResult<Json<serde_json::Value>> {
    s.category_manga.add_mangas_to_categories(&[manga_id], &[category_id]).await?;
    Ok(Json(serde_json::json!({ "message": "success" })))
}

async fn remove_from_category(
    State(s): State<AppState>,
    Path((manga_id, category_id)): Path<(i32, i32)>,
) -> ApiResult<Json<serde_json::Value>> {
    s.category_manga.remove_manga_from_category(manga_id, category_id).await?;
    Ok(Json(serde_json::json!({ "message": "success" })))
}
