//! OPDS v1.2 routes — mirrors `opds/OpdsAPI.kt` + `controller/OpdsV1Controller.kt`.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use suwayomi_rest::state::AppState;

use crate::constants::{MIME_OPDS_CATALOG, MIME_OPENSEARCH};
use crate::feeds::{self, FeedCtx};

const BASE_URL: &str = "/api/opds/v1.2";

pub fn opds_router() -> Router<AppState> {
    Router::new()
        .route("/", get(root_feed))
        .route("/search", get(search_description))
        .route("/history", get(history_feed))
        .route("/explore", get(explore_sources_feed))
        .route("/explore/source/{source_id}", get(explore_source_feed))
        .route("/library/series", get(library_series_feed))
        .route("/library/sources", get(library_sources_feed))
        .route("/library/categories", get(categories_feed))
        .route("/library/genres", get(genres_feed))
        .route("/library/statuses", get(statuses_feed))
        .route("/library/languages", get(languages_feed))
        .route("/library-updates", get(library_updates_feed))
        .route("/source/{source_id}", get(library_source_feed))
        .route("/category/{category_id}", get(category_feed))
        .route("/genre/{genre}", get(genre_feed))
        .route("/status/{status_id}", get(status_feed))
        .route("/language/{lang_code}", get(language_feed))
        .route("/series/{series_id}/chapters", get(series_chapters_feed))
        .route("/series/{series_id}/chapter/{chapter_index}/metadata", get(chapter_metadata_feed))
}

fn ctx<'a>(state: &'a AppState, lang: &'a str) -> FeedCtx<'a> {
    FeedCtx {
        db: &state.db,
        base_url: BASE_URL,
        lang,
        fetcher: Some(state.fetcher.clone()),
    }
}

fn xml(body: String) -> Response {
    ([(axum::http::header::CONTENT_TYPE, MIME_OPDS_CATALOG)], body).into_response()
}

fn xml_opensearch(body: String) -> Response {
    ([(axum::http::header::CONTENT_TYPE, MIME_OPENSEARCH)], body).into_response()
}

#[derive(Deserialize)]
struct LangQuery {
    lang: Option<String>,
}

#[derive(Deserialize)]
struct PageQuery {
    page_number: Option<usize>,
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

#[derive(Deserialize)]
struct SourceFeedQuery {
    page_number: Option<usize>,
    sort: Option<String>,
    lang: Option<String>,
}

async fn root_feed(State(state): State<AppState>, Query(q): Query<LangQuery>) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    xml(feeds::root_feed(&ctx(&state, lang)).await)
}

async fn search_description(Query(q): Query<LangQuery>) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    xml_opensearch(feeds::search_description(lang))
}

async fn history_feed(State(state): State<AppState>, Query(q): Query<PageQuery>) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    let page = q.page_number.unwrap_or(1).max(1);
    xml(feeds::history_feed(&ctx(&state, lang), page).await)
}

async fn explore_sources_feed(State(state): State<AppState>, Query(q): Query<LangQuery>) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    xml(feeds::explore_sources_feed(&ctx(&state, lang)).await)
}

async fn explore_source_feed(
    State(state): State<AppState>,
    Path(source_id): Path<i64>,
    Query(q): Query<SourceFeedQuery>,
) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    let page = q.page_number.unwrap_or(1).max(1);
    let sort = q.sort.as_deref().unwrap_or("popular");
    xml(feeds::explore_source_feed(&ctx(&state, lang), source_id, page, sort).await)
}

async fn library_series_feed(State(state): State<AppState>, Query(q): Query<SeriesQuery>) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    let page = q.page_number.unwrap_or(1).max(1);
    let is_search = q.query.is_some() || q.author.is_some() || q.title.is_some();
    if is_search {
        xml(feeds::search_feed(&ctx(&state, lang), q.query.as_deref(), q.author.as_deref(), q.title.as_deref(), page).await)
    } else {
        let sort = q.sort.as_deref().unwrap_or("title");
        let filter = q.filter.as_deref().unwrap_or("all");
        xml(feeds::library_series_feed(
            &ctx(&state, lang),
            q.source_id,
            q.category_id,
            q.status_id,
            q.lang_code.as_deref(),
            q.genre.as_deref(),
            page,
            sort,
            filter,
        )
        .await)
    }
}

async fn library_sources_feed(State(state): State<AppState>, Query(q): Query<LangQuery>) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    xml(feeds::library_sources_feed(&ctx(&state, lang)).await)
}

async fn categories_feed(State(state): State<AppState>, Query(q): Query<LangQuery>) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    xml(feeds::categories_feed(&ctx(&state, lang)).await)
}

async fn genres_feed(State(state): State<AppState>, Query(q): Query<LangQuery>) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    xml(feeds::genres_feed(&ctx(&state, lang)).await)
}

async fn statuses_feed(State(state): State<AppState>, Query(q): Query<LangQuery>) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    xml(feeds::statuses_feed(&ctx(&state, lang)).await)
}

async fn languages_feed(State(state): State<AppState>, Query(q): Query<LangQuery>) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    xml(feeds::languages_feed(&ctx(&state, lang)).await)
}

async fn library_updates_feed(State(state): State<AppState>, Query(q): Query<PageQuery>) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    let page = q.page_number.unwrap_or(1).max(1);
    xml(feeds::library_updates_feed(&ctx(&state, lang), page).await)
}

async fn library_source_feed(
    State(state): State<AppState>,
    Path(source_id): Path<i64>,
    Query(q): Query<SeriesQuery>,
) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    let page = q.page_number.unwrap_or(1).max(1);
    let sort = q.sort.as_deref().unwrap_or("title");
    let filter = q.filter.as_deref().unwrap_or("all");
    xml(feeds::library_series_feed(&ctx(&state, lang), Some(source_id), None, None, None, None, page, sort, filter).await)
}

async fn category_feed(
    State(state): State<AppState>,
    Path(category_id): Path<i32>,
    Query(q): Query<SeriesQuery>,
) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    let page = q.page_number.unwrap_or(1).max(1);
    let sort = q.sort.as_deref().unwrap_or("title");
    let filter = q.filter.as_deref().unwrap_or("all");
    xml(feeds::library_series_feed(&ctx(&state, lang), None, Some(category_id), None, None, None, page, sort, filter).await)
}

async fn genre_feed(State(state): State<AppState>, Path(genre): Path<String>, Query(q): Query<SeriesQuery>) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    let page = q.page_number.unwrap_or(1).max(1);
    let sort = q.sort.as_deref().unwrap_or("title");
    let filter = q.filter.as_deref().unwrap_or("all");
    xml(feeds::library_series_feed(&ctx(&state, lang), None, None, None, None, Some(&genre), page, sort, filter).await)
}

async fn status_feed(
    State(state): State<AppState>,
    Path(status_id): Path<i32>,
    Query(q): Query<SeriesQuery>,
) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    let page = q.page_number.unwrap_or(1).max(1);
    let sort = q.sort.as_deref().unwrap_or("title");
    let filter = q.filter.as_deref().unwrap_or("all");
    xml(feeds::library_series_feed(&ctx(&state, lang), None, None, Some(status_id), None, None, page, sort, filter).await)
}

async fn language_feed(
    State(state): State<AppState>,
    Path(lang_code): Path<String>,
    Query(q): Query<SeriesQuery>,
) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    let page = q.page_number.unwrap_or(1).max(1);
    let sort = q.sort.as_deref().unwrap_or("title");
    let filter = q.filter.as_deref().unwrap_or("all");
    xml(feeds::library_series_feed(&ctx(&state, lang), None, None, None, Some(&lang_code), None, page, sort, filter).await)
}

async fn series_chapters_feed(
    State(state): State<AppState>,
    Path(series_id): Path<i32>,
    Query(q): Query<SeriesQuery>,
) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    let page = q.page_number.unwrap_or(1).max(1);
    let sort = q.sort.as_deref().unwrap_or("number_asc");
    let filter = q.filter.as_deref().unwrap_or("all");
    match feeds::series_chapters_feed(&ctx(&state, lang), series_id, page, sort, filter).await {
        Ok(body) => xml(body),
        Err(_) => (StatusCode::NOT_FOUND, feeds::not_found_feed(&ctx(&state, lang), &format!("series/{series_id}/chapters"), "Manga not found")).into_response(),
    }
}

async fn chapter_metadata_feed(
    State(state): State<AppState>,
    Path((series_id, chapter_index)): Path<(i32, i32)>,
    Query(q): Query<LangQuery>,
) -> Response {
    let lang = q.lang.as_deref().unwrap_or("en");
    match feeds::chapter_metadata_feed(&ctx(&state, lang), series_id, chapter_index).await {
        Ok(body) => xml(body),
        Err(_) => (
            StatusCode::NOT_FOUND,
            feeds::not_found_feed(&ctx(&state, lang), &format!("series/{series_id}/chapter/{chapter_index}/metadata"), "Chapter not found"),
        )
            .into_response(),
    }
}
