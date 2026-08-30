//! REST backup endpoints — mirrors `controller/BackupController.kt`.
//! Phase 6: export implemented (gzipped protobuf); import/validate pending.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::state::AppState;

pub fn backup_router() -> Router<AppState> {
    Router::new()
        .route("/export", get(backup_export))
        .route("/export/file", get(backup_export_file))
        .route("/import", get(not_implemented).post(not_implemented))
        .route("/import/file", get(not_implemented).post(not_implemented))
        .route("/validate", get(not_implemented).post(not_implemented))
        .route("/validate/file", get(not_implemented).post(not_implemented))
}

/// Mirrors `protobufExport`: streams the gzipped protobuf backup as the body.
async fn backup_export(State(state): State<AppState>) -> Response {
    match suwayomi_core::backup::create_backup(state.db.pool()).await {
        Ok(bytes) => (
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            bytes,
        )
            .into_response(),
        Err(e) => {
            tracing::error!(%e, "backup export failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "backup export failed").into_response()
        }
    }
}

/// Mirrors `protobufExportFile`: same payload, advertised as an attachment.
async fn backup_export_file(State(state): State<AppState>) -> Response {
    match suwayomi_core::backup::create_backup(state.db.pool()).await {
        Ok(bytes) => {
            let filename = format!("org.suwayomi.tachidesk_{}.tachibk", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
            let content_disposition = format!("attachment; filename=\"{filename}\"");
            (
                [
                    (axum::http::header::CONTENT_TYPE, "application/octet-stream"),
                    (axum::http::header::CONTENT_DISPOSITION, content_disposition.as_str()),
                ],
                bytes,
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(%e, "backup export failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "backup export failed").into_response()
        }
    }
}

async fn not_implemented() -> Response {
    (StatusCode::NOT_IMPLEMENTED, "backup import/validate not implemented yet (Phase 6 export only)").into_response()
}
