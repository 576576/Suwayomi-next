//! Server configuration — mirrors `suwayomi.server.ServerConfig`
//! and `server-config` module. Values here are the process-level defaults
//! loaded from the environment; user-edited settings persist in the
//! `global_meta` blob and are layered on top when read back.

use serde::{Deserialize, Serialize};

/// 缓存根的显式覆盖（进程级，只认第一次设置）。
static CACHE_ROOT_OVERRIDE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// 钉住缓存根，供没有环境变量可用的宿主（Android）调用。
pub fn set_cache_root(dir: std::path::PathBuf) {
    let _ = CACHE_ROOT_OVERRIDE.set(dir);
}

/// 统一缓存根：显式覆盖 > `SUWAYOMI_CACHE_DIR` > `<发布根>/cache` > `./cache`。
/// 内分子目录（extensions/icons、extensions/index、trackers 等）。发布布局
/// bin/suwayomi-server.exe 时根 = exe 的上级；否则退回当前工作目录。
pub fn cache_root() -> std::path::PathBuf {
    if let Some(dir) = CACHE_ROOT_OVERRIDE.get() {
        return dir.clone();
    }
    if let Ok(dir) = std::env::var("SUWAYOMI_CACHE_DIR")
        && !dir.is_empty()
    {
        return std::path::PathBuf::from(dir);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && dir.file_name().map(|n| n == "bin").unwrap_or(false)
        && let Some(base) = dir.parent()
    {
        return base.join("cache");
    }
    std::path::PathBuf::from("cache")
}

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

/// CBZ 下载与 OPDS 条目使用的 MIME 类型（`opdsCbzMimetype`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CbzMediaType {
    /// IANA 标准（2017）：`application/vnd.comicbook+zip`。
    Modern,
    /// 旧版 .cbz 专用类型：`application/x-cbz`。
    Legacy,
    /// 旧版「所有漫画归档」类型：`application/octet-stream`。
    Compatible,
}

impl CbzMediaType {
    /// 响应头 `Content-Type` 的值。
    pub fn media_type(&self) -> &'static str {
        match self {
            Self::Modern => "application/vnd.comicbook+zip",
            Self::Legacy => "application/x-cbz",
            Self::Compatible => "application/octet-stream",
        }
    }
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
    /// JWT `aud` 声明（UI_LOGIN 用）。空表示不校验。
    pub jwt_audience: String,
    /// access token 有效期（`5m` / `PT5M`）。
    pub jwt_token_expiry: String,
    /// refresh token 有效期（`60d` / `P60D`）。
    pub jwt_refresh_expiry: String,
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
    /// CBZ 下载与 OPDS 条目的 MIME 类型。
    pub opds_cbz_mimetype: CbzMediaType,
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
            jwt_audience: "suwayomi-server-api".into(),
            jwt_token_expiry: "5m".into(),
            jwt_refresh_expiry: "60d".into(),
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
            opds_cbz_mimetype: CbzMediaType::Modern,
        }
    }
}

impl ServerConfig {
    /// 把 `global_meta` 里持久化的 settings blob（`setSettings` 写的 camelCase
    /// JSON）覆盖到本实例上。只认本结构自己持有的字段，其余键忽略；键缺失或
    /// 类型不符时保留原值。
    ///
    /// `graphql::settings::SettingsType::apply_overrides` 是同一份 blob 的显示侧
    /// 映射：两边必须覆盖同一批键，否则设置页显示的值和运行时用的值会分叉。
    pub fn apply_settings_blob(&mut self, blob: &serde_json::Value) {
        fn text(blob: &serde_json::Value, key: &str) -> Option<String> {
            blob.get(key).and_then(|v| v.as_str()).map(str::to_string)
        }
        fn flag(blob: &serde_json::Value, key: &str) -> Option<bool> {
            blob.get(key).and_then(|v| v.as_bool())
        }

        if let Some(v) = text(blob, "ip") {
            self.ip = v;
        }
        if let Some(v) = blob.get("port").and_then(|v| v.as_i64()) {
            self.port = v as i32;
        }
        if let Some(v) = text(blob, "databaseType") {
            self.database_type = match v.as_str() {
                "H2" => DatabaseType::H2,
                "POSTGRESQL" => DatabaseType::Postgresql,
                _ => self.database_type,
            };
        }
        if let Some(v) = text(blob, "databaseUrl") {
            self.database_url = v;
        }
        if let Some(v) = text(blob, "databaseUsername") {
            self.database_username = v;
        }
        if let Some(v) = text(blob, "databasePassword") {
            self.database_password = v;
        }
        if let Some(v) = flag(blob, "useHikariConnectionPool") {
            self.use_hikari_connection_pool = v;
        }
        if let Some(v) = flag(blob, "initialOpenInBrowserEnabled") {
            self.initial_open_in_browser_enabled = v;
        }
        if let Some(v) = text(blob, "authMode") {
            self.auth_mode = v;
        }
        if let Some(v) = text(blob, "authUsername") {
            self.auth_username = v;
        }
        if let Some(v) = text(blob, "authPassword") {
            self.auth_password = v;
        }
        if let Some(v) = text(blob, "jwtAudience") {
            self.jwt_audience = v;
        }
        // jwt 时长两种写法（`5m` / `PT5M`）都能解析，原样存即可。
        if let Some(v) = text(blob, "jwtTokenExpiry") {
            self.jwt_token_expiry = v;
        }
        if let Some(v) = text(blob, "jwtRefreshExpiry") {
            self.jwt_refresh_expiry = v;
        }
        if let Some(v) = text(blob, "koreaderSyncChecksumMethod") {
            self.koreader_sync_checksum_method = match v.as_str() {
                "BINARY" => KoreaderSyncChecksumMethod::Binary,
                "FILENAME" => KoreaderSyncChecksumMethod::Filename,
                _ => self.koreader_sync_checksum_method,
            };
        }
        if let Some(v) = text(blob, "koreaderSyncStrategyForward") {
            self.koreader_sync_strategy_forward = conflict_strategy(&v, self.koreader_sync_strategy_forward);
        }
        if let Some(v) = text(blob, "koreaderSyncStrategyBackward") {
            self.koreader_sync_strategy_backward = conflict_strategy(&v, self.koreader_sync_strategy_backward);
        }
        if let Some(v) = blob.get("koreaderSyncPercentageTolerance").and_then(|v| v.as_f64()) {
            self.koreader_sync_percentage_tolerance = v as f32;
        }
        if let Some(v) = flag(blob, "syncYomiEnabled") {
            self.sync_yomi_enabled = v;
        }
        if let Some(v) = text(blob, "syncYomiHost") {
            self.sync_yomi_host = v;
        }
        if let Some(v) = text(blob, "syncYomiApiKey") {
            self.sync_yomi_api_key = v;
        }
        if let Some(v) = flag(blob, "syncDataManga") {
            self.sync_data_manga = v;
        }
        if let Some(v) = flag(blob, "syncDataChapters") {
            self.sync_data_chapters = v;
        }
        if let Some(v) = flag(blob, "syncDataTracking") {
            self.sync_data_tracking = v;
        }
        if let Some(v) = flag(blob, "syncDataHistory") {
            self.sync_data_history = v;
        }
        if let Some(v) = flag(blob, "syncDataCategories") {
            self.sync_data_categories = v;
        }
        if let Some(v) = text(blob, "syncInterval")
            && let Some(d) = crate::auth::parse_duration(&v)
        {
            self.sync_interval = d.as_secs() as i64;
        }
        if let Some(v) = text(blob, "opdsCbzMimetype") {
            self.opds_cbz_mimetype = match v.as_str() {
                "MODERN" => CbzMediaType::Modern,
                "LEGACY" => CbzMediaType::Legacy,
                "COMPATIBLE" => CbzMediaType::Compatible,
                _ => self.opds_cbz_mimetype,
            };
        }
    }
}

/// settings blob 里的冲突策略写法（`KEEP_REMOTE` 等）；无法识别时退回 `fallback`。
fn conflict_strategy(s: &str, fallback: KoreaderSyncConflictStrategy) -> KoreaderSyncConflictStrategy {
    match s {
        "PROMPT" => KoreaderSyncConflictStrategy::Prompt,
        "KEEP_REMOTE" => KoreaderSyncConflictStrategy::KeepRemote,
        "KEEP_LOCAL" => KoreaderSyncConflictStrategy::KeepLocal,
        "DISABLED" => KoreaderSyncConflictStrategy::Disabled,
        _ => fallback,
    }
}

/// 运行时配置句柄：多个服务共享一份可整体替换的 `ServerConfig`。
///
/// 设置写入 `global_meta` 的 blob 后由持有者重算并 `replace`；读取方每次取
/// `snapshot`。服务各自持有一份 `ServerConfig` 值拷贝的话，改设置只有重启才生效。
#[derive(Clone)]
pub struct RuntimeConfig(std::sync::Arc<std::sync::RwLock<ServerConfig>>);

impl RuntimeConfig {
    pub fn new(config: ServerConfig) -> Self {
        Self(std::sync::Arc::new(std::sync::RwLock::new(config)))
    }

    /// 当前配置快照。
    pub fn snapshot(&self) -> ServerConfig {
        self.0.read().expect("config lock poisoned").clone()
    }

    /// 换上一份新配置，之后的 `snapshot` 返回新值。
    pub fn replace(&self, config: ServerConfig) {
        *self.0.write().expect("config lock poisoned") = config;
    }
}

impl From<ServerConfig> for RuntimeConfig {
    fn from(config: ServerConfig) -> Self {
        Self::new(config)
    }
}
