//! Route registration — mirrors `MangaAPI.kt` + `GlobalAPI.kt` endpoint
//! layout under `/api/v1/`.

pub mod category;
pub mod chapter;
pub mod download;
pub mod extension;
pub mod global;
pub mod manga;
pub mod meta_handler;
pub mod source;
pub mod track;
pub mod update;

use axum::routing::get;
use axum::Router;

use crate::state::AppState;

/// Builds the `/api/v1/**` router (OPDS & GraphQL mounted separately).
pub fn api_v1_router() -> Router<AppState> {
    Router::new()
        .nest("/extension", extension::router())
        .nest("/source", source::router())
        .nest("/manga", manga::router())
        .nest("/chapter", chapter::router())
        .nest("/category", category::router())
        .nest("/meta", global::meta_router())
        .nest("/settings", global::settings_router())
        .nest("/webview", global::webview_router())
        // Phase 6: implemented controllers (track/update/downloads backed by
        // DB or queue-manager contract; backup still stubbed until protobuf).
        .nest("/downloads", download::downloads_router())
        .nest("/download", download::download_router())
        .nest("/update", update::update_router())
        .nest("/track", track::track_router())
        .nest("/backup", stub_router())
}

fn stub_router() -> Router<AppState> {
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};
    async fn not_implemented(State(_s): State<AppState>) -> Response {
        (StatusCode::NOT_IMPLEMENTED, "Not implemented in this phase").into_response()
    }
    Router::new().route(
        "/{*path}",
        get(not_implemented).post(not_implemented).patch(not_implemented).delete(not_implemented).put(not_implemented),
    )
}
