//! Settings & about-server types — mirrors `SettingsType.kt` / `InfoType.kt`
//! (which are code-generated from ServerConfig in Kotlin; here hand-written
//! against `docs/graphql/schema-baseline.graphql`).

use async_graphql::{Enum, SimpleObject};
use suwayomi_core::config::{DatabaseType as CoreDatabaseType, ServerConfig};

use crate::scalars::{parse_iso8601_duration, DurationScalar, LongString};

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
    pub name: String,
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
    /// Auto backup cadence in minutes (0 = disabled). UI slider offers
    /// off / 1-12 hours / 1-6 days / weekly; default 43200 (12 hours).
    #[graphql(name = "autoBackupFrequency")]
    pub auto_backup_frequency: i32,
    pub backup_interval: i32,
    pub backup_path: String,
    #[graphql(name = "backupTTL")]
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
            auto_backup_frequency: 43200,
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
            max_sources_in_parallel: 6,
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

    /// Applies persisted overrides (the `settings` global_meta JSON blob
    /// written by `setSettings`) on top of the env-derived defaults, so saved
    /// values are reflected by the `settings` query.
    pub fn apply_overrides(&mut self, o: &serde_json::Value) {
        use serde_json::Value;
        self.auth_mode = match o.get("authMode").and_then(Value::as_str) {
            Some("BASIC_AUTH") => AuthMode::BasicAuth,
            Some("SIMPLE_LOGIN") => AuthMode::SimpleLogin,
            Some("UI_LOGIN") => AuthMode::UiLogin,
            Some("NONE") => AuthMode::None,
            _ => self.auth_mode,
        };
        self.auth_password = ov_str(o, "authPassword", self.auth_password.clone());
        self.auth_username = ov_str(o, "authUsername", self.auth_username.clone());
        self.auto_backup_include_categories = ov_bool(o, "autoBackupIncludeCategories", self.auto_backup_include_categories);
        self.auto_backup_include_chapters = ov_bool(o, "autoBackupIncludeChapters", self.auto_backup_include_chapters);
        self.auto_backup_include_client_data = ov_bool(o, "autoBackupIncludeClientData", self.auto_backup_include_client_data);
        self.auto_backup_include_history = ov_bool(o, "autoBackupIncludeHistory", self.auto_backup_include_history);
        self.auto_backup_include_manga = ov_bool(o, "autoBackupIncludeManga", self.auto_backup_include_manga);
        self.auto_backup_include_server_settings =
            ov_bool(o, "autoBackupIncludeServerSettings", self.auto_backup_include_server_settings);
        self.auto_backup_include_tracking = ov_bool(o, "autoBackupIncludeTracking", self.auto_backup_include_tracking);
        self.auto_download_ignore_re_uploads = ov_bool(o, "autoDownloadIgnoreReUploads", self.auto_download_ignore_re_uploads);
        self.auto_download_new_chapters = ov_bool(o, "autoDownloadNewChapters", self.auto_download_new_chapters);
        self.auto_download_new_chapters_limit = ov_i32(o, "autoDownloadNewChaptersLimit", self.auto_download_new_chapters_limit);
        self.auto_backup_frequency = ov_i32(o, "autoBackupFrequency", self.auto_backup_frequency);
        self.backup_interval = ov_i32(o, "backupInterval", self.backup_interval);
        self.backup_path = ov_str(o, "backupPath", self.backup_path.clone());
        self.backup_ttl = ov_i32(o, "backupTTL", self.backup_ttl);
        self.backup_time = ov_str(o, "backupTime", self.backup_time.clone());
        self.database_password = ov_str(o, "databasePassword", self.database_password.clone());
        self.database_type = match o.get("databaseType").and_then(Value::as_str) {
            Some("H2") => GraphqlDatabaseType::H2,
            Some("POSTGRESQL") => GraphqlDatabaseType::Postgresql,
            _ => self.database_type,
        };
        self.database_url = ov_str(o, "databaseUrl", self.database_url.clone());
        self.database_username = ov_str(o, "databaseUsername", self.database_username.clone());
        self.debug_logs_enabled = ov_bool(o, "debugLogsEnabled", self.debug_logs_enabled);
        self.download_as_cbz = ov_bool(o, "downloadAsCbz", self.download_as_cbz);
        self.download_conversions = ov_conversions(o, "downloadConversions");
        self.downloads_path = ov_str(o, "downloadsPath", self.downloads_path.clone());
        self.electron_path = ov_str(o, "electronPath", self.electron_path.clone());
        self.exclude_completed = ov_bool(o, "excludeCompleted", self.exclude_completed);
        self.exclude_entry_with_unread_chapters =
            ov_bool(o, "excludeEntryWithUnreadChapters", self.exclude_entry_with_unread_chapters);
        self.exclude_not_started = ov_bool(o, "excludeNotStarted", self.exclude_not_started);
        self.exclude_unread_chapters = ov_bool(o, "excludeUnreadChapters", self.exclude_unread_chapters);
        self.flare_solverr_as_response_fallback =
            ov_bool(o, "flareSolverrAsResponseFallback", self.flare_solverr_as_response_fallback);
        self.flare_solverr_enabled = ov_bool(o, "flareSolverrEnabled", self.flare_solverr_enabled);
        self.flare_solverr_session_name = ov_str(o, "flareSolverrSessionName", self.flare_solverr_session_name.clone());
        self.flare_solverr_session_ttl = ov_i32(o, "flareSolverrSessionTtl", self.flare_solverr_session_ttl);
        self.flare_solverr_timeout = ov_i32(o, "flareSolverrTimeout", self.flare_solverr_timeout);
        self.flare_solverr_url = ov_str(o, "flareSolverrUrl", self.flare_solverr_url.clone());
        self.global_update_interval = ov_f64(o, "globalUpdateInterval", self.global_update_interval);
        self.initial_open_in_browser_enabled =
            ov_bool(o, "initialOpenInBrowserEnabled", self.initial_open_in_browser_enabled);
        self.ip = ov_str(o, "ip", self.ip.clone());
        self.jwt_audience = ov_str(o, "jwtAudience", self.jwt_audience.clone());
        self.jwt_refresh_expiry = ov_dur(o, "jwtRefreshExpiry", self.jwt_refresh_expiry);
        self.jwt_token_expiry = ov_dur(o, "jwtTokenExpiry", self.jwt_token_expiry);
        self.kcef_enabled = ov_bool(o, "kcefEnabled", self.kcef_enabled);
        self.koreader_sync_checksum_method = match o.get("koreaderSyncChecksumMethod").and_then(Value::as_str) {
            Some("BINARY") => KoreaderSyncChecksumMethod::Binary,
            Some("FILENAME") => KoreaderSyncChecksumMethod::Filename,
            _ => self.koreader_sync_checksum_method,
        };
        self.koreader_sync_percentage_tolerance =
            ov_f64(o, "koreaderSyncPercentageTolerance", self.koreader_sync_percentage_tolerance);
        self.koreader_sync_strategy_backward = ov_conflict(o, "koreaderSyncStrategyBackward", self.koreader_sync_strategy_backward);
        self.koreader_sync_strategy_forward = ov_conflict(o, "koreaderSyncStrategyForward", self.koreader_sync_strategy_forward);
        self.local_source_path = ov_str(o, "localSourcePath", self.local_source_path.clone());
        self.max_log_file_size = ov_str(o, "maxLogFileSize", self.max_log_file_size.clone());
        self.max_log_files = ov_i32(o, "maxLogFiles", self.max_log_files);
        self.max_log_folder_size = ov_str(o, "maxLogFolderSize", self.max_log_folder_size.clone());
        self.max_sources_in_parallel = ov_i32(o, "maxSourcesInParallel", self.max_sources_in_parallel);
        self.opds_cbz_mimetype = match o.get("opdsCbzMimetype").and_then(Value::as_str) {
            Some("MODERN") => CbzMediaType::Modern,
            Some("LEGACY") => CbzMediaType::Legacy,
            Some("COMPATIBLE") => CbzMediaType::Compatible,
            _ => self.opds_cbz_mimetype,
        };
        self.opds_chapter_sort_order = match o.get("opdsChapterSortOrder").and_then(Value::as_str) {
            Some("ASC") => crate::query::SortOrder::Asc,
            Some("DESC") => crate::query::SortOrder::Desc,
            _ => self.opds_chapter_sort_order,
        };
        self.opds_enable_page_read_progress = ov_bool(o, "opdsEnablePageReadProgress", self.opds_enable_page_read_progress);
        self.opds_items_per_page = ov_i32(o, "opdsItemsPerPage", self.opds_items_per_page);
        self.opds_mark_as_read_on_download = ov_bool(o, "opdsMarkAsReadOnDownload", self.opds_mark_as_read_on_download);
        self.opds_show_only_downloaded_chapters =
            ov_bool(o, "opdsShowOnlyDownloadedChapters", self.opds_show_only_downloaded_chapters);
        self.opds_show_only_unread_chapters = ov_bool(o, "opdsShowOnlyUnreadChapters", self.opds_show_only_unread_chapters);
        self.opds_skip_chapter_metadata_feed = ov_bool(o, "opdsSkipChapterMetadataFeed", self.opds_skip_chapter_metadata_feed);
        self.opds_use_binary_file_sizes = ov_bool(o, "opdsUseBinaryFileSizes", self.opds_use_binary_file_sizes);
        self.port = ov_i32(o, "port", self.port);
        self.serve_conversions = ov_conversions(o, "serveConversions");
        self.socks_proxy_enabled = ov_bool(o, "socksProxyEnabled", self.socks_proxy_enabled);
        self.socks_proxy_host = ov_str(o, "socksProxyHost", self.socks_proxy_host.clone());
        self.socks_proxy_password = ov_str(o, "socksProxyPassword", self.socks_proxy_password.clone());
        self.socks_proxy_port = ov_str(o, "socksProxyPort", self.socks_proxy_port.clone());
        self.socks_proxy_username = ov_str(o, "socksProxyUsername", self.socks_proxy_username.clone());
        self.socks_proxy_version = ov_i32(o, "socksProxyVersion", self.socks_proxy_version);
        self.sync_data_categories = ov_bool(o, "syncDataCategories", self.sync_data_categories);
        self.sync_data_chapters = ov_bool(o, "syncDataChapters", self.sync_data_chapters);
        self.sync_data_history = ov_bool(o, "syncDataHistory", self.sync_data_history);
        self.sync_data_manga = ov_bool(o, "syncDataManga", self.sync_data_manga);
        self.sync_data_tracking = ov_bool(o, "syncDataTracking", self.sync_data_tracking);
        self.sync_interval = ov_dur(o, "syncInterval", self.sync_interval);
        self.sync_yomi_api_key = ov_str(o, "syncYomiApiKey", self.sync_yomi_api_key.clone());
        self.sync_yomi_enabled = ov_bool(o, "syncYomiEnabled", self.sync_yomi_enabled);
        self.sync_yomi_host = ov_str(o, "syncYomiHost", self.sync_yomi_host.clone());
        self.system_tray_enabled = ov_bool(o, "systemTrayEnabled", self.system_tray_enabled);
        self.update_mangas = ov_bool(o, "updateMangas", self.update_mangas);
        self.use_hikari_connection_pool = ov_bool(o, "useHikariConnectionPool", self.use_hikari_connection_pool);
        self.webui_channel = match o.get("webUIChannel").and_then(Value::as_str) {
            Some("BUNDLED") => WebUIChannel::Bundled,
            Some("STABLE") => WebUIChannel::Stable,
            Some("PREVIEW") => WebUIChannel::Preview,
            _ => self.webui_channel,
        };
        self.webui_flavor = match o.get("webUIFlavor").and_then(Value::as_str) {
            Some("WEBUI") => WebUIFlavor::Webui,
            Some("VUI") => WebUIFlavor::Vui,
            Some("CUSTOM") => WebUIFlavor::Custom,
            _ => self.webui_flavor,
        };
        self.webui_interface = match o.get("webUIInterface").and_then(Value::as_str) {
            Some("BROWSER") => WebUIInterface::Browser,
            Some("ELECTRON") => WebUIInterface::Electron,
            _ => self.webui_interface,
        };
        self.webui_update_check_interval = ov_f64(o, "webUIUpdateCheckInterval", self.webui_update_check_interval);
    }
}

fn ov_str(o: &serde_json::Value, k: &str, d: String) -> String {
    o.get(k).and_then(serde_json::Value::as_str).map(ToOwned::to_owned).unwrap_or(d)
}

fn ov_bool(o: &serde_json::Value, k: &str, d: bool) -> bool {
    o.get(k).and_then(serde_json::Value::as_bool).unwrap_or(d)
}

fn ov_i32(o: &serde_json::Value, k: &str, d: i32) -> i32 {
    o.get(k).and_then(serde_json::Value::as_i64).map(|v| v as i32).unwrap_or(d)
}

fn ov_f64(o: &serde_json::Value, k: &str, d: f64) -> f64 {
    o.get(k).and_then(serde_json::Value::as_f64).unwrap_or(d)
}

fn ov_dur(o: &serde_json::Value, k: &str, d: DurationScalar) -> DurationScalar {
    o.get(k)
        .and_then(serde_json::Value::as_str)
        .and_then(parse_iso8601_duration)
        .map(DurationScalar)
        .unwrap_or(d)
}

fn ov_conflict(o: &serde_json::Value, k: &str, d: KoreaderSyncConflictStrategy) -> KoreaderSyncConflictStrategy {
    match o.get(k).and_then(serde_json::Value::as_str) {
        Some("PROMPT") => KoreaderSyncConflictStrategy::Prompt,
        Some("KEEP_LOCAL") => KoreaderSyncConflictStrategy::KeepLocal,
        Some("KEEP_REMOTE") => KoreaderSyncConflictStrategy::KeepRemote,
        Some("DISABLED") => KoreaderSyncConflictStrategy::Disabled,
        _ => d,
    }
}

fn ov_conversions(o: &serde_json::Value, k: &str) -> Vec<SettingsDownloadConversionType> {
    let Some(arr) = o.get(k).and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    arr.iter().filter_map(conversion_from_json).collect()
}

fn conversion_from_json(v: &serde_json::Value) -> Option<SettingsDownloadConversionType> {
    use serde_json::Value;
    let obj = v.as_object()?;
    let mime_type = obj.get("mimeType")?.as_str()?.to_string();
    let target = obj.get("target")?.as_str()?.to_string();
    let header = |x: &Value| -> Option<SettingsDownloadConversionHeaderType> {
        let h = x.as_object()?;
        Some(SettingsDownloadConversionHeaderType {
            name: h.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            value: h.get("value").and_then(Value::as_str).unwrap_or("").to_string(),
        })
    };
    Some(SettingsDownloadConversionType {
        call_timeout: obj
            .get("callTimeout")
            .and_then(Value::as_str)
            .and_then(parse_iso8601_duration)
            .map(DurationScalar)
            .unwrap_or_default(),
        compression_level: obj.get("compressionLevel").and_then(Value::as_f64),
        connect_timeout: obj
            .get("connectTimeout")
            .and_then(Value::as_str)
            .and_then(parse_iso8601_duration)
            .map(DurationScalar)
            .unwrap_or_default(),
        headers: obj
            .get("headers")
            .and_then(Value::as_array)
            .map(|hs| hs.iter().filter_map(header).collect())
            .unwrap_or_default(),
        mime_type,
        target,
    })
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
    /// User data root (backups/downloads/local source live under it) —
    /// displayed by the WebUI "Data & Storage" settings page.
    pub data_dir: String,
}

impl AboutServerPayload {
    pub fn current(data_dir: &str, jvm: JvmInfo) -> Self {
        let os_name = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();
        // 真实构建类型：编译期常量（core/build.rs 由 CI 注入的
        // SUWAYOMI_BUILD_TYPE 决定，缺省 release）。
        // NB: 这里不能用 std::env::var —— 那是读运行时环境，用户机器上没有 CI 的变量，
        // 会导致 alpha / beta 包都报成 release。
        let build_type = suwayomi_core::version::BUILD_TYPE.to_string();
        Self {
            // 真实构建时间与版本（由 suwayomi-core/build.rs 注入）——
            // 之前的 0 / 0.1.0 让 WebUI 关于页显示 1970-01-01 与占位版本。
            build_time: LongString(suwayomi_core::version::BUILD_TIME_EPOCH_SECS.parse().unwrap_or(0)),
            build_type,
            discord: "https://qm.qq.com/q/aq1PDjhjMc".into(),
            github: "https://github.com/576576/Suwayomi-next".into(),
            name: "Suwayomi (next)".into(),
            platform_info: PlatformInfo {
                arch,
                headless: true,
                // 真实 JVM 信息由 jvm-sandbox 上报（/jvm），未连接时兜底 n/a。
                jvm,
                os: OSInfo { build: None, name: os_name, version: "n/a".into() },
            },
            revision: suwayomi_core::version::VERSION_CODE.into(),
            version: suwayomi_core::version::VERSION.into(),
            data_dir: data_dir.to_string(),
        }
    }
}
