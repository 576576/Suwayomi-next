//! Source endpoints — mirrors `SourceController.kt`. Fetching goes through
//! the `SourceFetcher` (JVM sandbox in Phase 5); list/detail are DB-backed.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use sqlx::Row;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct SearchParams {
    pub query: String,
    #[serde(default = "default_page")]
    pub page_num: u32,
}

fn default_page() -> u32 {
    1
}

#[derive(Deserialize)]
pub struct PageParams {
    #[serde(default = "default_page")]
    pub page_num: u32,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/list", get(list))
        .route("/{source_id}", get(retrieve))
        .route("/{source_id}/popular/{page_num}", get(popular))
        .route("/{source_id}/latest/{page_num}", get(latest))
        .route("/{source_id}/search", get(search))
        .route("/{source_id}/preferences", get(get_preferences).post(set_preferences))
        .route("/{source_id}/filters", get(get_filters).post(set_filters))
}

async fn list(State(s): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let sql = "SELECT * FROM source ORDER BY name ASC";
    let rows = sqlx::query(sql).fetch_all(s.db.pool()).await.map_err(ApiError::from)?;
    let out: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.try_get::<i64, _>("id").unwrap_or(0).to_string(),
                "name": r.try_get::<String, _>("name").unwrap_or_default(),
                "lang": r.try_get::<String, _>("lang").unwrap_or_default(),
                "isNsfw": false,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "sources": out })))
}

async fn retrieve(State(s): State<AppState>, Path(source_id): Path<i64>) -> ApiResult<Json<serde_json::Value>> {
    let sql = suwayomi_domain::sql::bind_placeholders("SELECT * FROM source WHERE id = ?");
    let row = sqlx::query(&sql)
        .bind(source_id)
        .fetch_optional(s.db.pool())
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("source not found".into()))?;
    Ok(Json(serde_json::json!({
        "id": row.try_get::<i64, _>("id").unwrap_or(0).to_string(),
        "name": row.try_get::<String, _>("name").unwrap_or_default(),
        "lang": row.try_get::<String, _>("lang").unwrap_or_default(),
    })))
}

async fn popular(
    State(s): State<AppState>,
    Path((source_id, page_num)): Path<(i64, u32)>,
) -> ApiResult<Json<serde_json::Value>> {
    let page = s.manga_list.get_manga_list(source_id, page_num, true).await?;
    Ok(Json(serde_json::to_value(&page).map_err(|e| ApiError::Internal(e.to_string()))?))
}

async fn latest(
    State(s): State<AppState>,
    Path((source_id, page_num)): Path<(i64, u32)>,
) -> ApiResult<Json<serde_json::Value>> {
    let page = s.manga_list.get_manga_list(source_id, page_num, false).await?;
    Ok(Json(serde_json::to_value(&page).map_err(|e| ApiError::Internal(e.to_string()))?))
}

async fn search(
    State(s): State<AppState>,
    Path(source_id): Path<i64>,
    Query(q): Query<SearchParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let page = s.manga_list.fetcher.search_manga(source_id, &q.query, q.page_num).await.map_err(ApiError::from)?;
    let out = s.manga_list.process_entries(source_id, &page).await?;
    Ok(Json(serde_json::to_value(&out).map_err(|e| ApiError::Internal(e.to_string()))?))
}

async fn get_preferences(
    State(_s): State<AppState>,
    Path(_source_id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!([])))
}

async fn set_preferences(
    State(_s): State<AppState>,
    Path(_source_id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({ "message": "success" })))
}

async fn get_filters(State(_s): State<AppState>, Path(_source_id): Path<i64>) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!([])))
}

async fn set_filters(State(_s): State<AppState>, Path(_source_id): Path<i64>) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({ "message": "success" })))
}
