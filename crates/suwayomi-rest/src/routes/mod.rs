//! Route registration — mirrors `MangaAPI.kt` + `GlobalAPI.kt` endpoint
//! layout under `/api/v1/`.

pub mod category;
pub mod chapter;
pub mod extension;
pub mod global;
pub mod manga;
pub mod meta_handler;
pub mod source;

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
        // Phase 6 stubs — endpoints registered with matching routes returning 501
        .nest("/backup", stub_router())
        .nest("/downloads", stub_router())
        .nest("/download", stub_router())
        .nest("/update", stub_router())
        .nest("/track", stub_router())
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
