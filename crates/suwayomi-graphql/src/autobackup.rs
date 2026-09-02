//! Scheduled auto-backup: periodically writes `org.suwayomi.next_*.tachibk`
//! into the configured backup folder (`backupPath`, default `data/autobackup`)
//! while `autoBackupFrequency` (seconds, 0 = disabled) is non-zero.
//!
//! The cadence is read live from the persisted `settings` global_meta blob
//! (the same source the settings query overlays), so changes made in the UI
//! take effect without a restart. The last-run time is stored under
//! `last_auto_backup_at` (epoch seconds) so restarts don't immediately create
//! another file.

use std::collections::HashMap;
use std::path::PathBuf;

use suwayomi_domain::meta::{MetaService, MetaTable};

use crate::settings::SettingsType;
use crate::state::GraphQLState;

/// global_meta key tracking when the last auto backup was written (epoch seconds).
const LAST_AUTO_BACKUP_AT: &str = "last_auto_backup_at";

/// How often the scheduler re-checks whether a backup is due.
const TICK_SECS: u64 = 30;

pub fn spawn(state: GraphQLState) {
    tokio::spawn(async move {
        loop {
            run_if_due(&state).await;
            tokio::time::sleep(std::time::Duration::from_secs(TICK_SECS)).await;
        }
    });
}

/// Creates a backup now if one is due according to `autoBackupFrequency`.
pub async fn run_if_due(state: &GraphQLState) {
    let (frequency_secs, folder) = match load_settings(state).await {
        Some(v) => v,
        None => return,
    };
    if frequency_secs <= 0 {
        return;
    }

    let meta = MetaService::new(state.db.clone());
    let last: i64 = meta
        .get_map(MetaTable::Global, 0)
        .await
        .ok()
        .and_then(|m| m.get(LAST_AUTO_BACKUP_AT).and_then(|v| v.parse().ok()))
        .unwrap_or(0);
    let now = suwayomi_core::models::now_epoch_secs();
    // First run (no timestamp yet) backs up immediately; afterwards only once
    // the configured interval has elapsed.
    if last != 0 && now.saturating_sub(last) < frequency_secs as i64 {
        return;
    }

    match create_backup_file(state, &folder).await {
        Ok(()) => {
            let mut m = HashMap::new();
            m.insert(LAST_AUTO_BACKUP_AT.to_string(), now.to_string());
            let mut by_ref = HashMap::new();
            by_ref.insert(0i64, m);
            if let Err(e) = meta.modify(MetaTable::Global, &by_ref).await {
                tracing::warn!(%e, "autobackup: failed to persist last backup time");
            }
            tracing::info!("autobackup: wrote backup to {}", folder.display());
        }
        Err(e) => {
            tracing::warn!(%e, "autobackup: failed");
        }
    }
}

async fn create_backup_file(state: &GraphQLState, folder: &PathBuf) -> Result<(), String> {
    let bytes = suwayomi_core::backup::create_backup(state.db.pool()).await.map_err(|e| e.to_string())?;
    std::fs::create_dir_all(folder).map_err(|e| format!("mkdir: {e}"))?;
    let filename = format!("org.suwayomi.next_{}.tachibk", chrono::Local::now().format("%Y-%m-%d_%H-%M"));
    let path = folder.join(filename);
    std::fs::write(&path, &bytes).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

/// Mirrors the settings query: config defaults overlaid with the persisted
/// `settings` global_meta JSON blob.
async fn load_settings(state: &GraphQLState) -> Option<(i32, PathBuf)> {
    use sqlx::Row;
    let mut settings = SettingsType::from_config(&state.config);
    let sql = "SELECT value FROM global_meta WHERE meta_key = 'settings'";
    if let Ok(Some(row)) = sqlx::query(sql).fetch_optional(state.db.pool()).await {
        if let Ok(value) = row.try_get::<String, _>("value") {
            if let Ok(blob) = serde_json::from_str::<serde_json::Value>(&value) {
                settings.apply_overrides(&blob);
            }
        }
    }
    let frequency = settings.auto_backup_frequency;
    let folder = if settings.backup_path.trim().is_empty() {
        state.data_dir.join("autobackup")
    } else {
        PathBuf::from(settings.backup_path.trim())
    };
    Some((frequency, folder))
}
