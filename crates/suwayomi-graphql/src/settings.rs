//! Settings & about-server types — mirrors `SettingsType.kt` / `InfoType.kt`
//! (which are code-generated from ServerConfig in Kotlin; here hand-written
//! against `docs/graphql/schema-baseline.graphql`).

use async_graphql::{Enum, SimpleObject};
use suwayomi_core::config::{DatabaseType as CoreDatabaseType, ServerConfig};

use crate::scalars::{DurationScalar, LongString};

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum AuthMode {
    None,
    BasicAuth,
    SimpleLogin,
    UiLogin,
}

impl AuthMode {
    pub fn from_mode(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "BASIC_AUTH" => Self::BasicAuth,
            "SIMPLE_LOGIN" => Self::SimpleLogin,
            "UI_LOGIN" => Self::UiLogin,
            _ => Self::None,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(name = "DatabaseType")]
pub enum GraphqlDatabaseType {
    H2,
    Postgresql,
}

impl From<CoreDatabaseType> for GraphqlDatabaseType {
    fn from(t: CoreDatabaseType) -> Self {
        match t {
            CoreDatabaseType::H2 => Self::H2,
            CoreDatabaseType::Postgresql => Self::Postgresql,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum CbzMediaType {
    Modern,
    Legacy,
    Compatible,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum KoreaderSyncChecksumMethod {
    Binary,
    Filename,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum KoreaderSyncLegacyStrategy {
    Prompt,
    Silent,
    Send,
    Receive,
    Disabled,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum KoreaderSyncConflictStrategy {
    Prompt,
    KeepLocal,
    KeepRemote,
    Disabled,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum WebUIChannel {
    Bundled,
    Stable,
    Preview,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum WebUIFlavor {
    Webui,
    Vui,
    Custom,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum WebUIInterface {
    Browser,
    Electron,
}

/// Mirrors `SettingsDownloadConversionType`.
#[derive(SimpleObject, Clone)]
pub struct SettingsDownloadConversionType {
    pub call_timeout: DurationScalar,
    pub compression_level: Option<f64>,
    pub connect_timeout: DurationScalar,
    pub headers: Vec<SettingsDownloadConversionHeaderType>,
    pub mime_type: String,
    pub target: String,
}

/// Mirrors `SettingsDownloadConversionHeaderType`.
#[derive(SimpleObject, Clone)]
pub struct SettingsDownloadConversionHeaderType {
    pub key: String,
    pub value: String,
}

/// Mirrors `SettingsType` — 96 fields generated from ServerConfig in Kotlin.
#[derive(SimpleObject, Clone)]
#[graphql(rename_fields = "camelCase")]
pub struct SettingsType {
    pub auth_mode: AuthMode,
    pub auth_password: String,
    pub auth_username: String,
    pub auto_backup_include_categories: bool,
    pub auto_backup_include_chapters: bool,
    pub auto_backup_include_client_data: bool,
    pub auto_backup_include_history: bool,
    pub auto_backup_include_manga: bool,
    pub auto_backup_include_server_settings: bool,
    pub auto_backup_include_tracking: bool,
    #[graphql(deprecation = "Replaced with autoDownloadNewChaptersLimit")]
    pub auto_download_ahead_limit: i32,
    pub auto_download_ignore_re_uploads: bool,
    pub auto_download_new_chapters: bool,
    pub auto_download_new_chapters_limit: i32,
    pub backup_interval: i32,
    pub backup_path: String,
    pub backup_ttl: i32,
    pub backup_time: String,
    #[graphql(deprecation = "Removed - prefer authMode")]
    pub basic_auth_enabled: bool,
    #[graphql(deprecation = "Removed - prefer authPassword")]
    pub basic_auth_password: String,
    #[graphql(deprecation = "Removed - prefer authUsername")]
    pub basic_auth_username: String,
    pub database_password: String,
    pub database_type: GraphqlDatabaseType,
    pub database_url: String,
    pub database_username: String,
    pub debug_logs_enabled: bool,
    pub download_as_cbz: bool,
    pub download_conversions: Vec<SettingsDownloadConversionType>,
    pub downloads_path: String,
    pub electron_path: String,
    pub exclude_completed: bool,
    pub exclude_entry_with_unread_chapters: bool,
    pub exclude_not_started: bool,
    pub exclude_unread_chapters: bool,
    #[graphql(deprecation = "Replaced with addExtensionStore and removeExtensionStore mutations")]
    pub extension_repos: Vec<String>,
    pub flare_solverr_as_response_fallback: bool,
    pub flare_solverr_enabled: bool,
    pub flare_solverr_session_name: String,
    pub flare_solverr_session_ttl: i32,
    pub flare_solverr_timeout: i32,
    pub flare_solverr_url: String,
    pub global_update_interval: f64,
    #[graphql(deprecation = "Removed - does not do anything")]
    pub gql_debug_logs_enabled: bool,
    pub initial_open_in_browser_enabled: bool,
    pub ip: String,
    pub jwt_audience: String,
    pub jwt_refresh_expiry: DurationScalar,
    pub jwt_token_expiry: DurationScalar,
    pub kcef_enabled: bool,
    pub koreader_sync_checksum_method: KoreaderSyncChecksumMethod,
    #[graphql(deprecation = "Moved to preference store")]
    pub koreader_sync_device_id: String,
    pub koreader_sync_percentage_tolerance: f64,
    #[graphql(deprecation = "Moved to preference store")]
    pub koreader_sync_server_url: String,
    #[graphql(deprecation = "Replaced with koreaderSyncStrategyForward and koreaderSyncStrategyBackward")]
    pub koreader_sync_strategy: KoreaderSyncLegacyStrategy,
    pub koreader_sync_strategy_backward: KoreaderSyncConflictStrategy,
    pub koreader_sync_strategy_forward: KoreaderSyncConflictStrategy,
    #[graphql(deprecation = "Moved to preference store")]
    pub koreader_sync_userkey: String,
    #[graphql(deprecation = "Moved to preference store")]
    pub koreader_sync_username: String,
    pub local_source_path: String,
    pub max_log_file_size: String,
    pub max_log_files: i32,
    pub max_log_folder_size: String,
    pub max_sources_in_parallel: i32,
    pub opds_cbz_mimetype: CbzMediaType,
    pub opds_chapter_sort_order: crate::query::SortOrder,
    pub opds_enable_page_read_progress: bool,
    pub opds_items_per_page: i32,
    pub opds_mark_as_read_on_download: bool,
    pub opds_show_only_downloaded_chapters: bool,
    pub opds_show_only_unread_chapters: bool,
    pub opds_skip_chapter_metadata_feed: bool,
    pub opds_use_binary_file_sizes: bool,
    pub port: i32,
    pub serve_conversions: Vec<SettingsDownloadConversionType>,
    pub socks_proxy_enabled: bool,
    pub socks_proxy_host: String,
    pub socks_proxy_password: String,
    pub socks_proxy_port: String,
    pub socks_proxy_username: String,
    pub socks_proxy_version: i32,
    pub sync_data_categories: bool,
    pub sync_data_chapters: bool,
    pub sync_data_history: bool,
    pub sync_data_manga: bool,
    pub sync_data_tracking: bool,
    pub sync_interval: DurationScalar,
    pub sync_yomi_api_key: String,
    pub sync_yomi_enabled: bool,
    pub sync_yomi_host: String,
    pub system_tray_enabled: bool,
    pub update_mangas: bool,
    pub use_hikari_connection_pool: bool,
    #[graphql(name = "webUIChannel")]
    pub webui_channel: WebUIChannel,
    #[graphql(name = "webUIFlavor")]
    pub webui_flavor: WebUIFlavor,
    #[graphql(name = "webUIInterface")]
    pub webui_interface: WebUIInterface,
    #[graphql(name = "webUIUpdateCheckInterval")]
    pub webui_update_check_interval: f64,
}

impl SettingsType {
    /// Builds settings from `ServerConfig`; fields not yet backed by the
    /// config registry return Kotlin-compatible defaults (Phase 6 wires the
    /// full settings subsystem).
    pub fn from_config(c: &ServerConfig) -> Self {
        let basic_auth = c.auth_mode.to_uppercase().as_str() == "BASIC_AUTH";
        Self {
            auth_mode: AuthMode::from_mode(&c.auth_mode),
            auth_password: c.auth_password.clone(),
            auth_username: c.auth_username.clone(),
            auto_backup_include_categories: false,
            auto_backup_include_chapters: false,
            auto_backup_include_client_data: false,
            auto_backup_include_history: false,
            auto_backup_include_manga: false,
            auto_backup_include_server_settings: false,
            auto_backup_include_tracking: false,
            auto_download_ahead_limit: 3,
            auto_download_ignore_re_uploads: false,
            auto_download_new_chapters: false,
            auto_download_new_chapters_limit: 20,
            backup_interval: 0,
            backup_path: String::new(),
            backup_ttl: 0,
            backup_time: String::new(),
            basic_auth_enabled: basic_auth,
            basic_auth_password: c.auth_password.clone(),
            basic_auth_username: c.auth_username.clone(),
            database_password: c.database_password.clone(),
            database_type: c.database_type.into(),
            database_url: c.database_url.clone(),
            database_username: c.database_username.clone(),
            debug_logs_enabled: false,
            download_as_cbz: false,
            download_conversions: vec![],
            downloads_path: String::new(),
            electron_path: String::new(),
            exclude_completed: false,
            exclude_entry_with_unread_chapters: false,
            exclude_not_started: false,
            exclude_unread_chapters: false,
            extension_repos: vec![],
            flare_solverr_as_response_fallback: false,
            flare_solverr_enabled: false,
            flare_solverr_session_name: String::new(),
            flare_solverr_session_ttl: 0,
            flare_solverr_timeout: 0,
            flare_solverr_url: String::new(),
            global_update_interval: 0.0,
            gql_debug_logs_enabled: false,
            initial_open_in_browser_enabled: c.initial_open_in_browser_enabled,
            ip: c.ip.clone(),
            jwt_audience: String::new(),
            jwt_refresh_expiry: DurationScalar(std::time::Duration::ZERO),
            jwt_token_expiry: DurationScalar(std::time::Duration::ZERO),
            kcef_enabled: false, // R3: CEF removed (Tauri shell instead)
            koreader_sync_checksum_method: KoreaderSyncChecksumMethod::Binary,
            koreader_sync_device_id: String::new(),
            koreader_sync_percentage_tolerance: 0.0,
            koreader_sync_server_url: String::new(),
            koreader_sync_strategy: KoreaderSyncLegacyStrategy::Disabled,
            koreader_sync_strategy_backward: KoreaderSyncConflictStrategy::Disabled,
            koreader_sync_strategy_forward: KoreaderSyncConflictStrategy::Disabled,
            koreader_sync_userkey: String::new(),
            koreader_sync_username: String::new(),
            local_source_path: String::new(),
            max_log_file_size: String::new(),
            max_log_files: 0,
            max_log_folder_size: String::new(),
            max_sources_in_parallel: 0,
            opds_cbz_mimetype: CbzMediaType::Modern,
            opds_chapter_sort_order: crate::query::SortOrder::Asc,
            opds_enable_page_read_progress: true,
            opds_items_per_page: 30,
            opds_mark_as_read_on_download: false,
            opds_show_only_downloaded_chapters: false,
            opds_show_only_unread_chapters: false,
            opds_skip_chapter_metadata_feed: false,
            opds_use_binary_file_sizes: false,
            port: c.port,
            serve_conversions: vec![],
            socks_proxy_enabled: false,
            socks_proxy_host: String::new(),
            socks_proxy_password: String::new(),
            socks_proxy_port: String::new(),
            socks_proxy_username: String::new(),
            socks_proxy_version: 0,
            sync_data_categories: false,
            sync_data_chapters: false,
            sync_data_history: false,
            sync_data_manga: false,
            sync_data_tracking: false,
            sync_interval: DurationScalar(std::time::Duration::ZERO),
            sync_yomi_api_key: String::new(),
            sync_yomi_enabled: false,
            sync_yomi_host: String::new(),
            system_tray_enabled: false,
            update_mangas: false,
            use_hikari_connection_pool: c.use_hikari_connection_pool,
            webui_channel: WebUIChannel::Stable,
            webui_flavor: WebUIFlavor::Webui,
            webui_interface: WebUIInterface::Browser,
            webui_update_check_interval: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// AboutServer payload (InfoQuery)
// ---------------------------------------------------------------------------

#[derive(SimpleObject, Clone)]
pub struct JvmInfo {
    pub java_version: String,
    pub vm_name: String,
    pub vm_vendor: String,
    pub vm_version: String,
}

#[derive(SimpleObject, Clone)]
#[graphql(name = "OSInfo")]
pub struct OSInfo {
    pub build: Option<String>,
    pub name: String,
    pub version: String,
}

#[derive(SimpleObject, Clone)]
pub struct PlatformInfo {
    pub arch: String,
    pub headless: bool,
    pub jvm: JvmInfo,
    pub os: OSInfo,
}

/// Mirrors `AboutServerPayload` — full field set.
#[derive(SimpleObject, Clone)]
#[graphql(rename_fields = "camelCase")]
pub struct AboutServerPayload {
    pub build_time: LongString,
    pub build_type: String,
    pub discord: String,
    pub github: String,
    pub name: String,
    pub platform_info: PlatformInfo,
    #[graphql(deprecation = "The version includes the revision as the patch number")]
    pub revision: String,
    pub version: String,
}

impl AboutServerPayload {
    pub fn current() -> Self {
        let os_name = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();
        Self {
            build_time: LongString(0),
            build_type: "release".into(),
            discord: "https://qm.qq.com/q/aq1PDjhjMc".into(),
            github: "https://github.com/576576/Suwayomi-next".into(),
            name: "Suwayomi (next)".into(),
            platform_info: PlatformInfo {
                arch,
                headless: true,
                jvm: JvmInfo {
                    java_version: "n/a (Rust)".into(),
                    vm_name: "n/a".into(),
                    vm_vendor: "n/a".into(),
                    vm_version: "n/a".into(),
                },
                os: OSInfo { build: None, name: os_name, version: "n/a".into() },
            },
            revision: "".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}
