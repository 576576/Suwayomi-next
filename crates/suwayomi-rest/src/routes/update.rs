//! Update REST endpoints — mirrors `UpdateController.kt`.
//! 四个端点都接真实的 `UpdateManager`：`summary` 读内存状态，`fetch`/`reset`
//! 启停任务，`recentChapters` 查库。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};

use suwayomi_core::models::{CategoryDataClass, IncludeOrExclude, MangaChapterDataClass, MangaDataClass};
use suwayomi_core::schema::CategoryRow;
use suwayomi_domain::manga::manga_row_to_data_class;
use suwayomi_domain::updater::{CategoryJobStatus, MangaJobStatus};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// `categoryId` 既可能在 query 上，也可能在表单 body 里 —— 上游用的是 Javalin
/// 的 `formParam`，两种都取。
#[derive(serde::Deserialize, Default)]
pub struct FetchParams {
    #[serde(rename = "categoryId")]
    pub category_id: Option<i32>,
}

async fn recent_chapters(State(s): State<AppState>, Path(page_num): Path<usize>) -> Json<Vec<MangaChapterDataClass>> {
    let page =
        s.chapter.get_recent_chapters(page_num.max(1)).await.unwrap_or_else(|_| {
            suwayomi_core::models::pagination::PaginatedList { page: vec![], has_next_page: false }
        });
    Json(page.page)
}

fn category_dc(c: &CategoryRow) -> CategoryDataClass {
    CategoryDataClass {
        id: c.id,
        order: c.sort_order,
        name: c.name.clone(),
        default: c.is_default,
        include_in_update: IncludeOrExclude::from_i32(c.include_in_update),
        include_in_download: IncludeOrExclude::from_i32(c.include_in_download),
        version: c.version,
        uid: c.uid,
        last_modified_at: c.last_modified_at,
    }
}

/// Mirrors `UpdateController.updateSummary` — 上游返回 `updater.statusDeprecated.value`，
/// 即 `UpdateStatus`：`{categoryStatusMap, mangaStatusMap, running}`。
///
/// 两个 map 的键都是**枚举名**，且都不补齐：`categoryStatusMap` 只带本轮真实存在的
/// 分类状态（`reset()` 清空后就是 `{}`），`mangaStatusMap` 恒带 `SKIPPED`
/// （上游 `plus(Pair(SKIPPED, skippedMangas))`），其余状态有才出现。
async fn summary(State(s): State<AppState>) -> Json<serde_json::Value> {
    let jobs = s.update.jobs().await;
    let skipped = s.update.skipped_mangas().await;
    let cats = s.update.category_status().await;
    let running = s.update.is_running().await;

    let mut category_map = serde_json::Map::new();
    for (key, st) in [("UPDATING", CategoryJobStatus::Updating), ("SKIPPED", CategoryJobStatus::Skipped)] {
        if let Some(rows) = cats.get(&st) {
            category_map.insert(key.to_string(), serde_json::json!(rows.iter().map(category_dc).collect::<Vec<_>>()));
        }
    }

    let mut manga_map = serde_json::Map::new();
    for (key, st) in [
        ("PENDING", MangaJobStatus::Pending),
        ("RUNNING", MangaJobStatus::Running),
        ("COMPLETE", MangaJobStatus::Complete),
        ("FAILED", MangaJobStatus::Failed),
    ] {
        let list: Vec<MangaDataClass> = jobs
            .iter()
            .filter(|j| j.status == st)
            .map(|j| manga_row_to_data_class(&j.manga))
            .collect();
        if !list.is_empty() {
            manga_map.insert(key.to_string(), serde_json::json!(list));
        }
    }
    manga_map.insert(
        "SKIPPED".to_string(),
        serde_json::json!(skipped.iter().map(manga_row_to_data_class).collect::<Vec<_>>()),
    );

    Json(serde_json::json!({
        "categoryStatusMap": serde_json::Value::Object(category_map),
        "mangaStatusMap": serde_json::Value::Object(manga_map),
        "running": running,
    }))
}

/// Mirrors `UpdateController.categoryUpdate` — 不给 `categoryId` 就更新整库，
/// 给了就只更新该分类（分类不存在返回 400）。
async fn fetch_update(
    State(s): State<AppState>,
    Query(q): Query<FetchParams>,
    form: Result<axum::extract::Form<FetchParams>, axum::extract::rejection::FormRejection>,
) -> ApiResult<StatusCode> {
    let category_id = q.category_id.or_else(|| form.ok().and_then(|f| f.0.category_id));
    match category_id {
        Some(id) => {
            let exists: Option<i32> = suwayomi_db::query_scalar("SELECT id FROM category WHERE id = $1")
                .bind(id)
                .fetch_optional(s.db.pool())
                .await?;
            if exists.is_none() {
                return Err(ApiError::BadRequest(format!("category {id} not found")));
            }
            s.update.start(Some(vec![id])).await;
        }
        None => s.update.start(None).await,
    }
    Ok(StatusCode::OK)
}

/// Mirrors `UpdateController.reset` — 停掉任务并复位内存状态；数据库不动。
async fn reset(State(s): State<AppState>) -> StatusCode {
    s.update.reset().await;
    StatusCode::OK
}

pub fn update_router() -> Router<AppState> {
    Router::new()
        .route("/recentChapters/{pageNum}", get(recent_chapters))
        .route("/summary", get(summary))
        .route("/fetch", axum::routing::post(fetch_update))
        .route("/reset", axum::routing::post(reset))
}
