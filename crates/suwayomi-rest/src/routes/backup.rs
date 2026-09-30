//! REST backup endpoints — mirrors `controller/BackupController.kt`.
//! Export + import + validate are implemented (gzipped protobuf).

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::json;
use std::collections::HashMap;

use suwayomi_api::AppState;

pub fn backup_router() -> Router<AppState> {
    Router::new()
        .route("/export", get(backup_export))
        .route("/export/file", get(backup_export_file))
        .route("/import", post(backup_import))
        .route("/import/file", post(backup_import_file))
        .route("/validate", post(backup_validate))
        .route("/validate/file", post(backup_validate_file))
}

/// 导出内容开关：GraphQL `createBackup` 把它们编进下载 URL 的 query，缺省全选。
fn export_flags(query: &HashMap<String, String>) -> suwayomi_core::backup::BackupFlags {
    suwayomi_core::backup::BackupFlags::from_query_pairs(query.iter().map(|(k, v)| (k.as_str(), v.as_str())))
}

/// 导出用的输入：服务端设置取自当前生效配置，图源设置问沙盒要。
///
/// 两个导出端点共用同一份装配 —— 各写一份的话，会出现「附件下载带设置、内联流不带」
/// 这种只在某一条路径上成立的差异。
async fn export_inputs(
    state: &AppState,
) -> (suwayomi_core::config::ServerConfig, Vec<suwayomi_core::backup::BackupSourcePreferences>) {
    (state.config.snapshot(), state.fetcher.backup_source_preferences().await)
}

/// Mirrors `protobufExport`: streams the gzipped protobuf backup as the body.
async fn backup_export(State(state): State<AppState>, Query(query): Query<HashMap<String, String>>) -> Response {
    let flags = export_flags(&query);
    let (config, source_preferences) = export_inputs(&state).await;
    let inputs = suwayomi_core::backup::BackupInputs { server_config: Some(&config), source_preferences };
    match suwayomi_core::backup::create_backup(state.db.pool(), flags, inputs).await {
        Ok(bytes) => ([(axum::http::header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response(),
        Err(e) => {
            tracing::error!(%e, "backup export failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "backup export failed").into_response()
        }
    }
}

/// Mirrors `protobufExportFile`: same payload, advertised as an attachment.
async fn backup_export_file(State(state): State<AppState>, Query(query): Query<HashMap<String, String>>) -> Response {
    let flags = export_flags(&query);
    let (config, source_preferences) = export_inputs(&state).await;
    let inputs = suwayomi_core::backup::BackupInputs { server_config: Some(&config), source_preferences };
    match suwayomi_core::backup::create_backup(state.db.pool(), flags, inputs).await {
        Ok(bytes) => {
            // Mirror the autobackup / Mihon naming scheme
            // (org.suwayomi.next_2026-08-30_01-44.tachibk, local time): saving
            // the download next to data/autobackup keeps one naming for both.
            let filename = format!("org.suwayomi.next_{}.tachibk", chrono::Local::now().format("%Y-%m-%d_%H-%M"));
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

/// Mirrors `protobufImport`: body is a gzipped Tachiyomi/Mihon protobuf backup.
async fn backup_import(State(state): State<AppState>, body: Bytes) -> Response {
    match suwayomi_core::backup::restore_backup(state.db.pool(), &body, suwayomi_core::backup::BackupFlags::default())
        .await
    {
        Ok(summary) => {
            // 图源设置住扩展自己的 JVM 存储里，core 够不着，恢复后交回沙盒写。
            state.fetcher.apply_source_preferences(&summary.source_preferences).await;
            ([(axum::http::header::CONTENT_TYPE, "application/json")], summary_json(&summary, &[])).into_response()
        }
        Err(e) => {
            tracing::error!(%e, "backup import failed");
            (StatusCode::BAD_REQUEST, format!("backup import failed: {e}")).into_response()
        }
    }
}

/// Mirrors `protobufImportFile`: same body semantics (upload field handled as raw bytes).
async fn backup_import_file(State(state): State<AppState>, body: Bytes) -> Response {
    backup_import(State(state), body).await
}

/// Mirrors `protobufValidate`: reports missing sources/trackers without restoring.
async fn backup_validate(State(state): State<AppState>, body: Bytes) -> Response {
    let summary = match suwayomi_core::backup::validate_backup(&body).await {
        Ok(summary) => summary,
        Err(e) => {
            tracing::error!(%e, "backup validate failed");
            return (StatusCode::BAD_REQUEST, format!("backup validate failed: {e}")).into_response();
        }
    };
    let backup = match suwayomi_core::backup::decode_gz_backup(&body) {
        Ok(backup) => backup,
        Err(e) => {
            tracing::error!(%e, "backup validate failed");
            return (StatusCode::BAD_REQUEST, format!("backup validate failed: {e}")).into_response();
        }
    };

    // 备份里出现过、但本机没登录的追踪器才算「缺」（参考实现 `ProtoBackupValidator`）。
    let mut sync_ids: Vec<i32> =
        backup.backup_manga.iter().flat_map(|m| m.tracking.iter().map(|t| t.sync_id)).collect();
    sync_ids.sort_unstable();
    sync_ids.dedup();
    let mut missing_trackers: Vec<String> = Vec::new();
    for sync_id in sync_ids {
        let Some(service) = state.tracker.find(sync_id) else {
            continue;
        };
        if service.is_logged_in().await.unwrap_or(false) {
            continue;
        }
        missing_trackers.push(service.name().to_string());
    }
    missing_trackers.sort();

    ([(axum::http::header::CONTENT_TYPE, "application/json")], summary_json(&summary, &missing_trackers))
        .into_response()
}

async fn backup_validate_file(State(state): State<AppState>, body: Bytes) -> Response {
    backup_validate(State(state), body).await
}

fn summary_json(summary: &suwayomi_core::backup::RestoreSummary, missing_trackers: &[String]) -> String {
    json!({
        "missingSources": summary.missing_sources,
        "mangasMissingSources": summary.mangas_missing_sources,
        "missingTrackers": missing_trackers,
        "restoredManga": summary.restored_manga,
        "restoredCategories": summary.restored_categories,
        "restoredChapters": summary.restored_chapters,
        "errors": summary.errors,
    })
    .to_string()
}
