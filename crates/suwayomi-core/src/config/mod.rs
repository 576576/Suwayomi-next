//! Server configuration — mirrors `suwayomi.server.ServerConfig`
//! and `server-config` module. Full settings registry lands with the
//! settings subsystem (Phase 3); this is the minimal core used by the DB layer.

use serde::{Deserialize, Serialize};

/// Mirrors `graphql/types/DatabaseType.kt`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatabaseType {
    H2,
    Postgresql,
}

/// Mirrors `KoreaderSyncChecksumMethod` (checksum source for KOReader sync).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KoreaderSyncChecksumMethod {
    /// Hash of the downloaded chapter archive contents.
    Binary,
    /// MD5 of the `<manga title> - <chapter name>` filename.
    Filename,
}

/// Mirrors `KoreaderSyncConflictStrategy` (KOReader progress conflict policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KoreaderSyncConflictStrategy {
    Prompt,
    KeepRemote,
    KeepLocal,
    Disabled,
}

/// Mirrors the core `ServerConfig` settings consumed at startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerConfig {
    pub ip: String,
    pub port: i32,
    pub database_type: DatabaseType,
    pub database_url: String,
    pub database_username: String,
    pub database_password: String,
    pub use_hikari_connection_pool: bool,
    pub initial_open_in_browser_enabled: bool,
    pub auth_mode: String,
    pub auth_username: String,
    pub auth_password: String,
    /// KOReader sync: checksum source (default Filename).
    pub koreader_sync_checksum_method: KoreaderSyncChecksumMethod,
    /// KOReader sync: conflict strategy when remote progress is newer.
    pub koreader_sync_strategy_forward: KoreaderSyncConflictStrategy,
    /// KOReader sync: conflict strategy when local progress is newer.
    pub koreader_sync_strategy_backward: KoreaderSyncConflictStrategy,
    /// KOReader sync: percentage tolerance before a pull counts as a change.
    pub koreader_sync_percentage_tolerance: f32,
    /// SyncYomi: master switch.
    pub sync_yomi_enabled: bool,
    /// SyncYomi server host (e.g. https://sync.example.com).
    pub sync_yomi_host: String,
    /// SyncYomi API key (X-API-Token).
    pub sync_yomi_api_key: String,
    /// SyncYomi: include manga in backup.
    pub sync_data_manga: bool,
    /// SyncYomi: include chapters in backup.
    pub sync_data_chapters: bool,
    /// SyncYomi: include tracking in backup.
    pub sync_data_tracking: bool,
    /// SyncYomi: include history in backup.
    pub sync_data_history: bool,
    /// SyncYomi: include categories in backup.
    pub sync_data_categories: bool,
    /// SyncYomi: periodic interval in seconds (0 = manual only).
    pub sync_interval: i64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            ip: "0.0.0.0".into(),
            // Windows 上 4501-4900 常被 Hyper-V 动态保留（bind 报 10013），
            // 默认 8090 避开该区间；Docker 镜像内仍用 4567（Linux 无此问题）。
            port: 8090,
            database_type: DatabaseType::H2,
            database_url: String::new(),
            database_username: String::new(),
            database_password: String::new(),
            use_hikari_connection_pool: false,
            initial_open_in_browser_enabled: true,
            auth_mode: "DISABLED".into(),
            auth_username: String::new(),
            auth_password: String::new(),
            koreader_sync_checksum_method: KoreaderSyncChecksumMethod::Filename,
            koreader_sync_strategy_forward: KoreaderSyncConflictStrategy::KeepRemote,
            koreader_sync_strategy_backward: KoreaderSyncConflictStrategy::KeepRemote,
            koreader_sync_percentage_tolerance: 0.02,
            sync_yomi_enabled: false,
            sync_yomi_host: String::new(),
            sync_yomi_api_key: String::new(),
            sync_data_manga: true,
            sync_data_chapters: true,
            sync_data_tracking: true,
            sync_data_history: true,
            sync_data_categories: true,
            sync_interval: 0,
        }
    }
}
