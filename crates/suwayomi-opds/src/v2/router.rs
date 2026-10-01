//! OPDS v2 routes (`application/opds+json`) — the JSON sibling of the 1.2
//! router. Paths mirror 1.2 so a client only swaps the version segment.

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Deserialize;
use serde_json::json;
use suwayomi_api::AppState;

use crate::v2::feeds::{self, V2Ctx, V2Error};
use crate::v2::json::{MIME_DIVINA_JSON, MIME_OPDS_JSON, to_json};

const BASE_URL: &str = "/api/opds/v2";

pub fn v2_router() -> Router<AppState> {
    Router::new()
        .route("/", get(root_feed))
        .route("/library/series", get(library_series_feed))
        .route("/series/{series_id}/chapters", get(series_chapters_feed))
        .route("/series/{series_id}/chapter/{chapter_id}/manifest", get(chapter_manifest))
}

fn ctx<'a>(state: &'a AppState, lang: &'a str) -> V2Ctx<'a> {
    V2Ctx { db: &state.db, base_url: BASE_URL, lang }
}

fn json(body: String, media_type: &'static str) -> Response {
    ([(header::CONTENT_TYPE, media_type)], body).into_response()
}

fn error_response(status: StatusCode, message: &str) -> Response {
    (status, json(to_json(&json!({ "error": message })), "application/json")).into_response()
}

#[derive(Deserialize)]
struct LangQuery {
    lang: Option<String>,
}

#[derive(Deserialize)]
struct SeriesQuery {
    query: Option<String>,
    author: Option<String>,
    title: Option<String>,
    page_number: Option<usize>,
    lang: Option<String>,
    sort: Option<String>,
    filter: Option<String>,
    source_id: Option<i64>,
    category_id: Option<i32>,
    status_id: Option<i32>,
    lang_code: Option<String>,
    genre: Option<String>,
}

async fn root_feed(State(state): State<AppState>, Query(q): Query<LangQuery>) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    json(to_json(&feeds::root_feed(&ctx(&state, lang))), MIME_OPDS_JSON)
}

async fn library_series_feed(State(state): State<AppState>, Query(q): Query<SeriesQuery>) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    let page = q.page_number.unwrap_or(1).max(1);
    let ctx = ctx(&state, lang);

    // As in 1.2: search parameters on the library route switch it to search.
    let is_search = q.query.is_some() || q.author.is_some() || q.title.is_some();
    let feed = if is_search {
        feeds::search_feed(&ctx, q.query.as_deref(), q.author.as_deref(), q.title.as_deref(), page).await
    } else {
        feeds::library_series_feed(
            &ctx,
            q.source_id,
            q.category_id,
            q.status_id,
            q.lang_code.as_deref(),
            q.genre.as_deref(),
            page,
            q.sort.as_deref().unwrap_or("title"),
            q.filter.as_deref().unwrap_or("all"),
        )
        .await
    };
    json(to_json(&feed), MIME_OPDS_JSON)
}

async fn series_chapters_feed(
    State(state): State<AppState>,
    Path(series_id): Path<i32>,
    Query(q): Query<SeriesQuery>,
) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    let ctx = ctx(&state, lang);
    let page = q.page_number.unwrap_or(1).max(1);
    feeds::series_chapters_feed(
        &ctx,
        series_id,
        page,
        q.sort.as_deref().unwrap_or("number_asc"),
        q.filter.as_deref().unwrap_or("all"),
    )
    .await
    .map_or_else(
        |_| {
            (StatusCode::NOT_FOUND, json(to_json(&feeds::not_found_feed(&ctx, "Manga not found")), MIME_OPDS_JSON))
                .into_response()
        },
        |feed| json(to_json(&feed), MIME_OPDS_JSON),
    )
}

async fn chapter_manifest(
    State(state): State<AppState>,
    Path((series_id, chapter_id)): Path<(i32, i32)>,
    Query(q): Query<LangQuery>,
) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    let ctx = ctx(&state, lang);
    match feeds::chapter_manifest(&ctx, series_id, chapter_id).await {
        Ok(manifest) => json(to_json(&manifest), MIME_DIVINA_JSON),
        Err(V2Error::NotFound) => {
            (StatusCode::NOT_FOUND, json(to_json(&feeds::not_found_feed(&ctx, "Chapter not found")), MIME_OPDS_JSON))
                .into_response()
        }
        Err(V2Error::PageCountUnknown) => error_response(StatusCode::BAD_GATEWAY, "chapter page count is unknown"),
    }
}
