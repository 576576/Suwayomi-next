//! Category endpoints — mirrors `CategoryController.kt`.

use axum::extract::{Path, Query, State};
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::Deserialize;

use crate::error::ApiResult;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CategoryCreate {
    pub name: String,
}

#[derive(Deserialize)]
pub struct CategoryModify {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub is_default: Option<bool>,
    #[serde(default)]
    pub include_in_update: Option<i32>,
    #[serde(default)]
    pub include_in_download: Option<i32>,
}

#[derive(Deserialize)]
pub struct ReorderQuery {
    pub from: i32,
    pub to: i32,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(category_list).post(category_create))
        .route("/reorder", patch(category_reorder))
        .route("/{category_id}", get(category_mangas).patch(category_modify).delete(category_delete))
        .route("/{category_id}/meta", patch(category_meta))
}

async fn category_list(State(s): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let list = s.category.get_category_list().await?;
    Ok(Json(serde_json::to_value(&list).map_err(|e| crate::error::ApiError::Internal(e.to_string()))?))
}

async fn category_create(
    State(s): State<AppState>,
    Json(body): Json<CategoryCreate>,
) -> ApiResult<Json<serde_json::Value>> {
    let ids = s.category.create_categories(&[body.name]).await?;
    Ok(Json(serde_json::json!({ "id": ids[0] })))
}

async fn category_reorder(
    State(s): State<AppState>,
    Query(q): Query<ReorderQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    s.category.reorder_category(q.from, q.to).await?;
    Ok(Json(serde_json::json!({ "message": "success" })))
}

async fn category_mangas(
    State(s): State<AppState>,
    Path(category_id): Path<i32>,
) -> ApiResult<Json<serde_json::Value>> {
    let list = s.category_manga.get_category_manga_list(category_id).await?;
    Ok(Json(serde_json::to_value(&list).map_err(|e| crate::error::ApiError::Internal(e.to_string()))?))
}

async fn category_modify(
    State(s): State<AppState>,
    Path(category_id): Path<i32>,
    Json(body): Json<CategoryModify>,
) -> ApiResult<Json<serde_json::Value>> {
    s.category
        .update_category(category_id, body.name, body.is_default, body.include_in_update, body.include_in_download)
        .await?;
    Ok(Json(serde_json::json!({ "message": "success" })))
}

async fn category_delete(
    State(s): State<AppState>,
    Path(category_id): Path<i32>,
) -> ApiResult<Json<serde_json::Value>> {
    s.category.remove_category(category_id).await?;
    Ok(Json(serde_json::json!({ "message": "success" })))
}

#[derive(Deserialize)]
pub struct MetaParams {
    pub key: String,
    pub value: String,
}

async fn category_meta(
    State(s): State<AppState>,
    Path(category_id): Path<i32>,
    Json(body): Json<MetaParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut map = std::collections::HashMap::new();
    map.insert(category_id, std::collections::HashMap::from([(body.key, body.value)]));
    s.category.modify_metas(&map).await?;
    Ok(Json(serde_json::json!({ "message": "success" })))
}
