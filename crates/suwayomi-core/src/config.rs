//! Server configuration — mirrors `suwayomi.server.ServerConfig`
//! and `server-config` module. Values here are the process-level defaults
//! loaded from the environment; user-edited settings persist in the
//! `global_meta` blob and are layered on top when read back.

use serde::{Deserialize, Serialize};

mod paths;

/// 进程内各根目录的显式句柄（数据 / appdata / 下载 / 本地图源）。
///
/// 取代原先的三处进程级单例：路径在启动时解析一次，之后经构造参数注入到
/// `GraphQLState` / `AppState` / `DownloadManager` / `ExtensionStoreService`，
/// 不再有 `set_*_root()` 这种隐式全局写入口。
pub use paths::AppPaths;
/// appdata 根之下的两个子目录，供**在 `AppPaths` 构造之前**就要用到的场景：
/// 打开数据库之前先要知道库落在哪，拉起沙盒之前先要知道日志写哪。
pub use paths::{appdata_db, appdata_logs};

/// `SUWAYOMI_APPDATA_DIR` 环境变量名：appdata 根，**唯一**的目录级覆盖入口。
///
/// 缓存 / 数据库 / 设置 / 扩展四项都从它派生（见 [`AppPaths`]），各自都没有环境
/// 变量 —— 把可写根整个外指只需要设这一个。
pub const APPDATA_DIR_ENV: &str = "SUWAYOMI_APPDATA_DIR";

/// appdata 根的解析：`SUWAYOMI_APPDATA_DIR` > `<发布根>/appdata` > `./appdata`。
///
/// 发布布局（bin/suwayomi-server.exe）时根 = exe 的上级；否则退回相对路径，与
/// 数据目录同形。最终值由调用方装进 [`AppPaths::new`] 后注入 —— Android 宿主
/// 没有环境变量，直接把应用私有目录传进去即可。
pub fn resolve_appdata_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var(APPDATA_DIR_ENV)
        && !dir.trim().is_empty()
    {
        return std::path::PathBuf::from(dir);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && dir.file_name().is_some_and(|n| n == "bin")
        && let Some(base) = dir.parent()
    {
        return base.join("appdata");
    }
    std::path::PathBuf::from("appdata")
}

/// 设置里可用的目录占位符（大小写不敏感，只能出现在开头）：
/// `%APPDIR%` = 发布根（见 [`app_root`]），`%DATADIR%` = 数据目录。
pub const APP_DIR_TOKEN: &str = "%APPDIR%";
pub const DATA_DIR_TOKEN: &str = "%DATADIR%";

/// 发布根 —— 发布布局里 exe 在 `bin/` 下，取它的上级；否则取 exe 所在目录；
/// 拿不到 exe 就退回当前工作目录。
///
/// Android 上宿主不走这条（没有发布布局），`%APPDIR%` 在那里没有意义。
pub fn app_root() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        if dir.file_name().is_some_and(|n| n == "bin")
            && let Some(base) = dir.parent()
        {
            return base.to_path_buf();
        }
        return dir.to_path_buf();
    }
    std::env::current_dir().unwrap_or_default()
}

/// 把设置里填的目录串解析成实际路径。
///
/// 支持 `%APPDIR%` / `%DATADIR%` 两个前缀占位符（`data_dir` 由调用方给出 —— 同一份
/// 设置在不同进程里解析到哪儿，取决于那个进程的数据目录）。占位符之后的分段两种
/// 斜杠都认，所以 Windows 上 `%DATADIR%\downloads` 与 `%DATADIR%/downloads` 等价。
///
/// **不是占位符开头的一律原样返回**：绝对路径照用，相对路径仍按调用进程的当前工作
/// 目录解析（跟这之前的行为一致）—— 想让相对位置有确定含义就用占位符。
pub fn resolve_setting_path(value: &str, data_dir: &std::path::Path) -> std::path::PathBuf {
    let value = value.trim();
    for (token, base) in [(APP_DIR_TOKEN, app_root()), (DATA_DIR_TOKEN, data_dir.to_path_buf())] {
        // get(..) 而不是切片：value 开头是多字节字符时，按字节切会 panic
        if let Some(head) = value.get(..token.len())
            && head.eq_ignore_ascii_case(token)
        {
            let mut path = base;
            for segment in value[token.len()..].split(['\\', '/']) {
                if !segment.is_empty() {
                    path.push(segment);
                }
            }
            return path;
        }
    }
    std::path::PathBuf::from(value)
}

/// 数据库后端（GraphQL 的 `DatabaseType`）。
///
/// 参考实现的枚举里第一项是 `H2`（JVM 专有文件格式，本仓读不了），这里换成实际
/// 存在的 `RUSQLITE`：留着 `H2` 就等于在设置页给一个点了必然报错的选项。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatabaseType {
    Rusqlite,
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
            // 与参考实现一致。该值落在 Windows 的 Hyper-V 动态保留区（4501-4900，
            // bind 报 10013）里：桌面由托盘启动前嗅探并顺延（见 Suwayomi-tray），
            // 独立运行由下面监听循环的自顺延兜底。
            port: 4567,
            database_type: DatabaseType::Rusqlite,
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

/// 服务端**自连**用的基址（下载页图片走同源代理，URL 由服务自己拼）。
///
/// 端口取**实际绑定**的那个，而不是配置里的 `port`：端口被占用或落在 Windows
/// 保留段时监听会自顺延（`start()` 的 4501-4900 / +1 回退），回填配置值会把代理
/// 请求打到没人监听的端口上——每一次下载都整章失败。
///
/// host 用地址本身的回环形式：通配地址（`0.0.0.0` / `::`）不是可达的目的地址，
/// 换成 `127.0.0.1` / `[::1]`；绑定在具体 IP 上时该 IP 本身就是本机地址，原样用
/// （此时回环可能因不监听它而连不上）。
pub fn server_base_url(addr: std::net::SocketAddr) -> String {
    match addr.ip() {
        std::net::IpAddr::V4(ip) if ip.is_unspecified() => format!("http://127.0.0.1:{}", addr.port()),
        std::net::IpAddr::V6(ip) if ip.is_unspecified() => format!("http://[::1]:{}", addr.port()),
        std::net::IpAddr::V6(ip) => format!("http://[{ip}]:{}", addr.port()),
        std::net::IpAddr::V4(ip) => format!("http://{ip}:{}", addr.port()),
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
            blob.get(key).and_then(serde_json::Value::as_bool)
        }

        if let Some(v) = text(blob, "ip") {
            self.ip = v;
        }
        if let Some(v) = blob.get("port").and_then(serde_json::Value::as_i64) {
            self.port = v as i32;
        }
        if let Some(v) = text(blob, "databaseType") {
            self.database_type = match v.as_str() {
                // 认不出的值（含旧表单里已删掉的 H2）保持原样：它只驱动显示，
                // 真实后端由 `SUWAYOMI_DB_BACKEND` 决定
                "RUSQLITE" => DatabaseType::Rusqlite,
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
        if let Some(v) = blob.get("koreaderSyncPercentageTolerance").and_then(serde_json::Value::as_f64) {
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
    ///
    /// 锁中毒只说明"某个持有写锁的线程 panic 过"，配置值本身仍然完整；
    /// 这种情况继续读旧值重启不了设置页，直接 panic 反而会把一次读配置变成进程退出。
    pub fn snapshot(&self) -> ServerConfig {
        self.0.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// 换上一份新配置，之后的 `snapshot` 返回新值。
    pub fn replace(&self, config: ServerConfig) {
        *self.0.write().unwrap_or_else(std::sync::PoisonError::into_inner) = config;
    }
}

impl From<ServerConfig> for RuntimeConfig {
    fn from(config: ServerConfig) -> Self {
        Self::new(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_base_url_uses_the_actually_bound_port() {
        fn addr(s: &str) -> std::net::SocketAddr {
            s.parse().expect("socket addr")
        }
        // 配置的端口不可用、监听自顺延到 4568：基址必须跟着走
        assert_eq!(server_base_url(addr("0.0.0.0:4568")), "http://127.0.0.1:4568");
        // 通配 v6 回落到回环 v6；IPv6 字面量要加方括号，否则会被当成端口分隔
        assert_eq!(server_base_url(addr("[::]:4901")), "http://[::1]:4901");
        // 绑定在具体地址上时原样用它——此刻该地址才是可达的，回环反而可能没监听
        assert_eq!(server_base_url(addr("192.168.1.5:18090")), "http://192.168.1.5:18090");
        assert_eq!(server_base_url(addr("[fe80::1]:4567")), "http://[fe80::1]:4567");
    }

    #[test]
    fn setting_path_expands_tokens() {
        let data = std::path::Path::new("E:/data");
        let app = app_root();

        assert_eq!(resolve_setting_path("%DATADIR%/downloads", data), data.join("downloads"));
        assert_eq!(resolve_setting_path("%DATADIR%\\downloads", data), data.join("downloads"));
        assert_eq!(resolve_setting_path("%DATADIR%", data), data);
        assert_eq!(resolve_setting_path("%datadir%/a/b", data), data.join("a").join("b"));
        assert_eq!(resolve_setting_path("  %DATADIR%/a  ", data), data.join("a"));
        assert_eq!(resolve_setting_path("%APPDIR%/data", data), app.join("data"));

        // 不是占位符开头 → 原样（含绝对路径与普通相对路径）
        assert_eq!(resolve_setting_path("E:/x/y", data), std::path::PathBuf::from("E:/x/y"));
        assert_eq!(resolve_setting_path("rel/dir", data), std::path::PathBuf::from("rel/dir"));
        assert_eq!(resolve_setting_path("", data), std::path::PathBuf::new());
        // 占位符在中间/后面都不算
        assert_eq!(resolve_setting_path("data/%DATADIR%", data), std::path::PathBuf::from("data/%DATADIR%"));
        // 多字节开头的值不会因为按字节切片而 panic
        assert_eq!(resolve_setting_path("数据目录/%DATADIR%", data), std::path::PathBuf::from("数据目录/%DATADIR%"));
    }
}
