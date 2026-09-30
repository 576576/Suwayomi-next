//! Mihon/Suwayomi protobuf 备份（backup.proto）。消息结构为手写 prost derive
//! （无需 protoc），字段号对齐 kotlinx-protobuf @ProtoNumber。create_backup 由
//! 当前数据库构建 Backup 消息，返回 gzip 压缩的 protobuf 字节（.tachibk 载荷）。

use std::collections::HashMap;

use prost::Message;
use suwayomi_db::Db;

use crate::schema::{CategoryRow, ChapterRow, ExtensionStoreRow, MangaRow, TrackRecordRow};

/// 备份里认得的追踪器 id —— 与 `suwayomi_domain::tracker` 的常量一致
/// （1 MAL / 2 AniList / 3 Kitsu / 4 Shikimori / 5 Bangumi / 7 MangaUpdates）。
///
/// 恢复时用来丢弃备份里本仓库不支持的追踪器记录（参考实现 `BackupMangaHandler`
/// 也是这么做的），否则会留下一条没有对应追踪器的 `track_record`。
/// `suwayomi-domain` 的测试会断言两份清单相等，防止单边漂移。
pub const SUPPORTED_TRACKER_IDS: [i32; 6] = [1, 2, 3, 4, 5, 7];

/// 备份内容开关 —— 与 Mihon `BackupOptions` 的十项一一对应。
///
/// 默认全开，唯一的例外是 `include_private_settings`（对齐 Mihon 里唯一默认关闭的
/// `privateSettings`）。`include_history` 仍是空操作：本仓库没有 history 表，
/// 导出侧不填 `BackupHistory`。
#[derive(Debug, Clone, Copy)]
pub struct BackupFlags {
    pub include_manga: bool,
    pub include_categories: bool,
    pub include_chapters: bool,
    pub include_tracking: bool,
    pub include_history: bool,
    /// Mihon `readEntries`：除库内作品外，还带上「有已读章节但不在库」的作品。
    pub include_read_entries: bool,
    /// Mihon `appSettings`：服务端设置（9001）+ 各 meta 节（9000）。
    pub include_app_settings: bool,
    /// Mihon `extensionStores`：插件仓库（106）。
    pub include_extension_stores: bool,
    /// Mihon `sourceSettings`：扩展自己存的图源设置（105）。
    pub include_source_settings: bool,
    /// Mihon `privateSettings`：凭据与认证信息。默认关闭。
    pub include_private_settings: bool,
}

impl Default for BackupFlags {
    fn default() -> Self {
        Self {
            include_manga: true,
            include_categories: true,
            include_chapters: true,
            include_tracking: true,
            include_history: true,
            include_read_entries: true,
            include_app_settings: true,
            include_extension_stores: true,
            include_source_settings: true,
            include_private_settings: false,
        }
    }
}

/// 只给了部分键的开关（GraphQL `PartialBackupFlagsInput` 的对应物）。
#[derive(Debug, Clone, Copy, Default)]
pub struct PartialBackupFlags {
    pub include_manga: Option<bool>,
    pub include_categories: Option<bool>,
    pub include_chapters: Option<bool>,
    pub include_tracking: Option<bool>,
    pub include_history: Option<bool>,
    pub include_read_entries: Option<bool>,
    pub include_app_settings: Option<bool>,
    pub include_extension_stores: Option<bool>,
    pub include_source_settings: Option<bool>,
    pub include_private_settings: Option<bool>,
}

impl BackupFlags {
    /// 未指定的键沿用默认值（参考实现 `BackupFlags.fromPartial`）。
    pub fn from_partial(p: &PartialBackupFlags) -> Self {
        let d = Self::default();
        Self {
            include_manga: p.include_manga.unwrap_or(d.include_manga),
            include_categories: p.include_categories.unwrap_or(d.include_categories),
            include_chapters: p.include_chapters.unwrap_or(d.include_chapters),
            include_tracking: p.include_tracking.unwrap_or(d.include_tracking),
            include_history: p.include_history.unwrap_or(d.include_history),
            include_read_entries: p.include_read_entries.unwrap_or(d.include_read_entries),
            include_app_settings: p.include_app_settings.unwrap_or(d.include_app_settings),
            include_extension_stores: p.include_extension_stores.unwrap_or(d.include_extension_stores),
            include_source_settings: p.include_source_settings.unwrap_or(d.include_source_settings),
            include_private_settings: p.include_private_settings.unwrap_or(d.include_private_settings),
        }
    }

    /// 字段按 [`BACKUP_FLAG_QUERY_KEYS`] 的顺序展开。
    fn to_array(self) -> [bool; 10] {
        [
            self.include_manga,
            self.include_categories,
            self.include_chapters,
            self.include_tracking,
            self.include_history,
            self.include_read_entries,
            self.include_app_settings,
            self.include_extension_stores,
            self.include_source_settings,
            self.include_private_settings,
        ]
    }

    /// [`to_array`](Self::to_array) 的逆操作。
    fn from_array(values: [bool; 10]) -> Self {
        Self {
            include_manga: values[0],
            include_categories: values[1],
            include_chapters: values[2],
            include_tracking: values[3],
            include_history: values[4],
            include_read_entries: values[5],
            include_app_settings: values[6],
            include_extension_stores: values[7],
            include_source_settings: values[8],
            include_private_settings: values[9],
        }
    }

    /// 编成手动备份下载 URL 的 query 串（`GET /api/v1/backup/export/file?…`）。
    ///
    /// 开关只经 URL 传递，生成端（GraphQL `createBackup`）与消费端（REST 下载）
    /// 都走这里：两端各写一份键名，就会出现「URL 丢开关、导出内容恒为全选」。
    pub fn to_query_string(&self) -> String {
        BACKUP_FLAG_QUERY_KEYS
            .iter()
            .zip(self.to_array())
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&")
    }

    /// 解析下载 URL 的 query；缺键或值不是布尔时沿用默认值（全选）。
    pub fn from_query_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut values = Self::default().to_array();
        for (key, value) in pairs {
            let Ok(value) = value.parse::<bool>() else {
                continue;
            };
            let Some(index) = BACKUP_FLAG_QUERY_KEYS.iter().position(|known| *known == key) else {
                continue;
            };
            if let Some(slot) = values.get_mut(index) {
                *slot = value;
            }
        }
        Self::from_array(values)
    }
}

/// 手动备份下载 URL 里承载开关的键名（camelCase，与 GraphQL
/// `PartialBackupFlagsInput` 的字段同名），顺序与 [`BackupFlags::to_array`] 一致。
const BACKUP_FLAG_QUERY_KEYS: [&str; 10] = [
    "includeManga",
    "includeCategories",
    "includeChapters",
    "includeTracking",
    "includeHistory",
    "includeReadEntries",
    "includeAppSettings",
    "includeExtensionStores",
    "includeSourceSettings",
    "includePrivateSettings",
];

/// `global_meta` 里属于服务端自身状态、不进备份的键。
///
/// 这张表在本仓库是「客户端 meta」与「服务端状态」混住的：设置 blob、自动备份
/// 游标、KOReader 凭据、SyncYomi 位点都写在这里。放它们进备份会同时造成两件事——
/// 凭据随备份文件流出，以及恢复一份别人的备份会改掉本机设置与同步位点。
const GLOBAL_META_INTERNAL_KEYS: [&str; 3] = ["settings", "webui_migration", "last_auto_backup_at"];

/// 服务端自己写的 `global_meta` 键前缀（KOReader 凭据、SyncYomi 游标）。
const GLOBAL_META_INTERNAL_PREFIXES: [&str; 2] = ["koreader_sync_", "sync_yomi_"];

/// Mihon 用 `__PRIVATE_` 前缀标记敏感偏好、`__APP_STATE_` 标记永不入备份的态。
/// 图源设置从沙盒原样取回来，过滤在这里做。
pub const PRIVATE_PREFERENCE_PREFIX: &str = "__PRIVATE_";

/// 见 [`PRIVATE_PREFERENCE_PREFIX`]。
pub const APP_STATE_PREFERENCE_PREFIX: &str = "__APP_STATE_";

/// `key` 是否属于服务端自身状态（设置 blob / 同步位点 / 凭据）。
fn is_internal_global_meta_key(key: &str) -> bool {
    GLOBAL_META_INTERNAL_KEYS.contains(&key)
        || GLOBAL_META_INTERNAL_PREFIXES.iter().any(|prefix| key.starts_with(prefix))
}

// 恢复时「查找现有行」用的宽行类型：列多但只作一次性比对，抽别名避免 clippy
// `type_complexity` 噪音，也让 SELECT 与解构处的形状一目了然。
type ExistingMangaRow = (
    i32,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i32,
    Option<String>,
    String,
    Option<i64>,
    bool,
    bool,
);
type ExistingChapterRow = (i32, String, Option<String>, bool, bool, i32, i64, f32, i32);

// ---------------------------------------------------------------------------
// protobuf messages (0.x Suwayomi backup format)
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Backup {
    #[prost(message, repeated, tag = "1")]
    pub backup_manga: Vec<BackupManga>,
    #[prost(message, repeated, tag = "2")]
    pub backup_categories: Vec<BackupCategory>,
    #[prost(message, repeated, tag = "101")]
    pub backup_sources: Vec<BackupSource>,
    /// 图源设置（Mihon tag 105）。值是扩展自己存的，从沙盒取；本仓库没有服务端的
    /// 图源设置表。Mihon 的 104（客户端 SharedPreferences）这里不声明：服务端没有
    /// 那套键空间，prost 会跳过未知字段，Mihon 的备份照样解得开。
    #[prost(message, repeated, tag = "105")]
    pub backup_source_preferences: Vec<BackupSourcePreferences>,
    /// 插件商店仓库（Mihon tag 106），表 `extension_store`。
    #[prost(message, repeated, tag = "106")]
    pub backup_extension_stores: Vec<BackupExtensionStore>,
    #[prost(map = "string, string", tag = "9000")]
    pub meta: HashMap<String, String>,
    #[prost(message, optional, tag = "9001")]
    pub server_settings: Option<BackupServerSettings>,
    /// 追踪器凭据。参考实现把凭据放在客户端 SharedPreferences 里，备份格式没有这一节；
    /// 本仓库凭据落库（`tracker_credential`），用 Suwayomi 自留号段（9000+）带走。
    /// 其它客户端按 proto3 规则忽略未知字段。
    #[prost(message, repeated, tag = "9002")]
    pub tracker_credentials: Vec<BackupTrackerCredential>,
}

/// `tracker_credential` 表的一行。
#[derive(Clone, PartialEq, Eq, ::prost::Message)]
pub struct BackupTrackerCredential {
    #[prost(int32, tag = "1")]
    pub tracker_id: i32,
    #[prost(string, tag = "2")]
    pub username: String,
    /// OAuth 站点放 access token，MangaUpdates 放 session token。
    #[prost(string, tag = "3")]
    pub password: String,
    /// 整份 OAuth JSON（刷新用）。
    #[prost(string, tag = "4")]
    pub token: String,
    #[prost(bool, tag = "5")]
    pub token_expired: bool,
    #[prost(string, tag = "6")]
    pub score_type: String,
    #[prost(string, tag = "7")]
    pub pkce_verifier: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BackupManga {
    #[prost(int64, tag = "1")]
    pub source: i64,
    #[prost(string, tag = "2")]
    pub url: String,
    #[prost(string, tag = "3")]
    pub title: String,
    #[prost(string, optional, tag = "4")]
    pub artist: Option<String>,
    #[prost(string, optional, tag = "5")]
    pub author: Option<String>,
    #[prost(string, optional, tag = "6")]
    pub description: Option<String>,
    #[prost(string, repeated, tag = "7")]
    pub genre: Vec<String>,
    #[prost(int32, tag = "8")]
    pub status: i32,
    #[prost(string, optional, tag = "9")]
    pub thumbnail_url: Option<String>,
    #[prost(int64, tag = "13")]
    pub date_added: i64,
    #[prost(int32, tag = "14")]
    pub viewer: i32,
    #[prost(message, repeated, tag = "16")]
    pub chapters: Vec<BackupChapter>,
    #[prost(int32, repeated, tag = "17")]
    pub categories: Vec<i32>,
    #[prost(message, repeated, tag = "18")]
    pub tracking: Vec<BackupTracking>,
    #[prost(bool, tag = "100")]
    pub favorite: bool,
    #[prost(int32, tag = "101")]
    pub chapter_flags: i32,
    #[prost(int32, optional, tag = "103")]
    pub viewer_flags: Option<i32>,
    #[prost(message, repeated, tag = "104")]
    pub history: Vec<BackupHistory>,
    #[prost(int32, tag = "105")]
    pub update_strategy: i32,
    #[prost(int64, tag = "106")]
    pub last_modified_at: i64,
    #[prost(int64, tag = "109")]
    pub version: i64,
    #[prost(bool, tag = "111")]
    pub initialized: bool,
    #[prost(bytes = "vec", tag = "112")]
    pub memo: Vec<u8>,
    #[prost(map = "string, string", tag = "9000")]
    pub meta: HashMap<String, String>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BackupChapter {
    #[prost(string, tag = "1")]
    pub url: String,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(string, optional, tag = "3")]
    pub scanlator: Option<String>,
    #[prost(bool, tag = "4")]
    pub read: bool,
    #[prost(bool, tag = "5")]
    pub bookmark: bool,
    #[prost(int32, tag = "6")]
    pub last_page_read: i32,
    #[prost(int64, tag = "7")]
    pub date_fetch: i64,
    #[prost(int64, tag = "8")]
    pub date_upload: i64,
    #[prost(float, tag = "9")]
    pub chapter_number: f32,
    #[prost(int32, tag = "10")]
    pub source_order: i32,
    #[prost(int64, tag = "11")]
    pub last_modified_at: i64,
    #[prost(int64, tag = "12")]
    pub version: i64,
    #[prost(bytes = "vec", tag = "13")]
    pub memo: Vec<u8>,
    #[prost(map = "string, string", tag = "9000")]
    pub meta: HashMap<String, String>,
}

#[derive(Clone, PartialEq, Eq, ::prost::Message)]
pub struct BackupCategory {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(int32, tag = "2")]
    pub order: i32,
    #[prost(int32, tag = "100")]
    pub flags: i32,
    #[prost(int64, tag = "601")]
    pub version: i64,
    #[prost(int64, tag = "602")]
    pub uid: i64,
    #[prost(int64, tag = "603")]
    pub last_modified_at: i64,
    #[prost(map = "string, string", tag = "9000")]
    pub meta: HashMap<String, String>,
}

#[derive(Clone, PartialEq, Eq, ::prost::Message)]
pub struct BackupSource {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(int64, tag = "2")]
    pub source_id: i64,
    #[prost(map = "string, string", tag = "9000")]
    pub meta: HashMap<String, String>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BackupTracking {
    #[prost(int32, tag = "1")]
    pub sync_id: i32,
    #[prost(int64, tag = "2")]
    pub library_id: i64,
    #[prost(int32, tag = "3")]
    pub media_id_int: i32,
    #[prost(string, tag = "4")]
    pub tracking_url: String,
    #[prost(string, tag = "5")]
    pub title: String,
    #[prost(float, tag = "6")]
    pub last_chapter_read: f32,
    #[prost(int32, tag = "7")]
    pub total_chapters: i32,
    #[prost(float, tag = "8")]
    pub score: f32,
    #[prost(int32, tag = "9")]
    pub status: i32,
    #[prost(int64, tag = "10")]
    pub started_reading_date: i64,
    #[prost(int64, tag = "11")]
    pub finished_reading_date: i64,
    #[prost(bool, tag = "12")]
    pub private: bool,
    #[prost(int64, tag = "100")]
    pub media_id: i64,
}

#[derive(Clone, PartialEq, Eq, ::prost::Message)]
pub struct BackupHistory {
    #[prost(string, tag = "1")]
    pub url: String,
    #[prost(int64, tag = "2")]
    pub last_read: i64,
    #[prost(int64, tag = "3")]
    pub read_at: i64,
}

#[derive(Clone, PartialEq, Eq, ::prost::Message)]
pub struct BackupServerSettings {
    #[prost(string, tag = "1")]
    pub ip: String,
    #[prost(int32, tag = "2")]
    pub port: i32,
    #[prost(bool, tag = "3")]
    pub initial_open_in_browser_enabled: bool,
    #[prost(string, tag = "4")]
    pub auth_mode: String,
    #[prost(string, tag = "5")]
    pub auth_username: String,
    #[prost(string, tag = "6")]
    pub auth_password: String,
    #[prost(bool, tag = "7")]
    pub use_hikari_connection_pool: bool,
}

/// 一个源的扁平设置（Mihon `BackupSourcePreferences`，tag 105）。
///
/// `source_key` 是 `source_<id>`，与扩展自己的 `ConfigurableSource.preferenceKey()`
/// 一致 —— 扩展按这个键取它那份 `SharedPreferences`。
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BackupSourcePreferences {
    #[prost(string, tag = "1")]
    pub source_key: String,
    #[prost(message, repeated, tag = "2")]
    pub prefs: Vec<BackupPreference>,
}

/// 一条扁平偏好（Mihon `BackupPreference`）。
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BackupPreference {
    #[prost(string, tag = "1")]
    pub key: String,
    #[prost(message, optional, tag = "2")]
    pub value: Option<PreferenceValue>,
}

/// Mihon 把偏好值做成 sealed class，六个子类按声明顺序占 1..6 号字段，各自只有一个
/// `value`（tag 1）。顺序即协议：换错一位，整型会被读成字符串。
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PreferenceValue {
    #[prost(oneof = "preference_value::Value", tags = "1, 2, 3, 4, 5, 6")]
    pub value: Option<preference_value::Value>,
}

pub mod preference_value {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Value {
        #[prost(int32, tag = "1")]
        Int(i32),
        #[prost(int64, tag = "2")]
        Long(i64),
        #[prost(float, tag = "3")]
        Float(f32),
        #[prost(string, tag = "4")]
        Text(String),
        #[prost(bool, tag = "5")]
        Flag(bool),
        #[prost(message, tag = "6")]
        StringSet(StringSetValue),
    }

    /// `Set<String>`：一个只有 repeated 字段的消息，字段号 1。
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct StringSetValue {
        #[prost(string, repeated, tag = "1")]
        pub value: Vec<String>,
    }
}

/// 一个插件仓库（Mihon `BackupExtensionStore`，tag 106）。
///
/// 字段号按 Mihon 的声明顺序：3/4 与 5 在源码里是交错声明的，不要按书写顺序排。
#[derive(Clone, PartialEq, Eq, ::prost::Message)]
pub struct BackupExtensionStore {
    #[prost(string, tag = "1")]
    pub index_url: String,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(string, optional, tag = "3")]
    pub badge_label: Option<String>,
    #[prost(string, tag = "4")]
    pub contact_website: String,
    #[prost(string, tag = "5")]
    pub signing_key: String,
    #[prost(string, optional, tag = "6")]
    pub contact_discord: Option<String>,
    #[prost(bool, optional, tag = "7")]
    pub is_legacy: Option<bool>,
    #[prost(string, optional, tag = "8")]
    pub extension_list_url: Option<String>,
}

// ---------------------------------------------------------------------------
// export
// ---------------------------------------------------------------------------

/// 生成备份时需要、DB 之外的两样输入。
///
/// 服务端设置住 `ServerConfig`（不在库里），图源设置住沙盒（扩展自己的 JVM 存储，
/// core 够不到）—— 都由调用方取好喂进来，core 因此不依赖沙盒客户端。
#[derive(Debug, Default)]
pub struct BackupInputs<'a> {
    /// 写进 9001 号段的服务端设置；`None` 表示不填这一节。
    pub server_config: Option<&'a crate::config::ServerConfig>,
    /// 写进 105 号段的图源设置。调用方按 `include_source_settings` 决定要不要去取，
    /// 这里只负责按 `include_private_settings` 过滤。
    pub source_preferences: Vec<BackupSourcePreferences>,
}

/// 剔除敏感与「程序状态」偏好，并把过滤后为空的源整个丢掉。
///
/// Mihon 同样在导出侧过滤：`__PRIVATE_` 只在勾了敏感设置时带走，`__APP_STATE_`
/// 永远不带（那是运行态而不是设置）。
fn filter_source_preferences(
    mut groups: Vec<BackupSourcePreferences>,
    include_private: bool,
) -> Vec<BackupSourcePreferences> {
    for group in &mut groups {
        group.prefs.retain(|p| {
            !p.key.starts_with(APP_STATE_PREFERENCE_PREFIX)
                && (include_private || !p.key.starts_with(PRIVATE_PREFERENCE_PREFIX))
        });
    }
    groups.retain(|group| !group.prefs.is_empty());
    groups
}

// ---------------------------------------------------------------------------
// import / validate
// ---------------------------------------------------------------------------

/// Summary of a backup restore/validate run.
#[derive(Debug, Clone, Default)]
pub struct RestoreSummary {
    pub restored_manga: usize,
    pub restored_categories: usize,
    pub restored_chapters: usize,
    pub restored_extension_stores: usize,
    pub restored_preferences: usize,
    pub missing_sources: Vec<String>,
    pub mangas_missing_sources: Vec<String>,
    pub errors: Vec<String>,
    /// 备份里的图源设置（105）。core 写不进沙盒，由调用方拿到后写回扩展。
    pub source_preferences: Vec<BackupSourcePreferences>,
}

/// Decodes a gzipped `Backup` protobuf payload.
pub fn decode_gz_backup(gz: &[u8]) -> Result<Backup, BackupError> {
    use std::io::Read;
    let mut decoder = flate2::read::GzDecoder::new(gz);
    let mut raw = Vec::new();
    decoder.read_to_end(&mut raw)?;
    Backup::decode(raw.as_slice()).map_err(|e| BackupError::Decode(e.to_string()))
}

/// Validates a backup without touching the database (missing sources/trackers).
pub async fn validate_backup(gz: &[u8]) -> Result<RestoreSummary, BackupError> {
    let backup = decode_gz_backup(gz)?;
    Ok(validate_backup_inner(&backup))
}

fn validate_backup_inner(backup: &Backup) -> RestoreSummary {
    let available: std::collections::HashSet<i64> = backup.backup_sources.iter().map(|s| s.source_id).collect();
    let missing: Vec<i64> = backup
        .backup_manga
        .iter()
        .map(|m| m.source)
        .filter(|s| !available.contains(s))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let name_of = |sid: i64| {
        backup.backup_sources.iter().find(|s| s.source_id == sid).map_or_else(|| sid.to_string(), |s| s.name.clone())
    };
    let mangas_missing: Vec<String> = backup
        .backup_manga
        .iter()
        .filter(|m| missing.contains(&m.source))
        .map(|m| format!("{} [{}]", m.title, name_of(m.source)))
        .collect();
    RestoreSummary {
        missing_sources: missing.iter().map(|s| name_of(*s)).collect(),
        mangas_missing_sources: mangas_missing,
        ..Default::default()
    }
}

/// Restores a gzipped backup into the database.
///
/// Semantics mirror `ProtoBackupImport.performRestore` + `BackupMangaHandler`:
/// categories are matched/created by name, manga by (url, source) — existing
/// rows are merged, new rows inserted; chapters upsert on (url, manga).
pub async fn restore_backup(pool: &Db, gz: &[u8], flags: BackupFlags) -> Result<RestoreSummary, BackupError> {
    let backup = decode_gz_backup(gz)?;
    restore_backup_proto(pool, &backup, flags).await
}

/// Restores from an already-decoded `Backup` message (idempotent upserts).
pub async fn restore_backup_proto(
    pool: &Db,
    backup: &Backup,
    flags: BackupFlags,
) -> Result<RestoreSummary, BackupError> {
    let mut summary = validate_backup_inner(backup);
    let source_names: HashMap<i64, String> =
        backup.backup_sources.iter().map(|s| (s.source_id, s.name.clone())).collect();

    // 1) categories: order -> id (reuse existing by name; mirrors Kotlin's
    //    `BackupCategory.order`-keyed mapping used by BackupManga.categories)
    let mut category_mapping: HashMap<i32, i32> = HashMap::new(); // category order -> db id
    for (idx, c) in backup.backup_categories.iter().enumerate() {
        if !flags.include_categories {
            break;
        }
        let existing: Option<i32> = suwayomi_db::query_scalar("SELECT id FROM category WHERE name = $1")
            .bind(&c.name)
            .fetch_optional(pool)
            .await?;
        let id = if let Some(id) = existing {
            id
        } else {
            // Use a fresh, collision-free sort_order: keying membership by
            // numeric order is fine within one backup file, but reusing a
            // value already taken in the DB (e.g. several categories at
            // order 0) would make a later restore of our own export map
            // memberships onto the wrong category.
            let next_order: i32 = suwayomi_db::query_scalar("SELECT COALESCE(MAX(sort_order), 0) + 1 FROM category")
                .fetch_one(pool)
                .await?;
            let id: i32 =
                suwayomi_db::query_scalar("INSERT INTO category (name, sort_order) VALUES ($1, $2) RETURNING id")
                    .bind(&c.name)
                    .bind(next_order)
                    .fetch_one(pool)
                    .await?;
            summary.restored_categories += 1;
            id
        };
        let _ = idx;
        category_mapping.insert(c.order, id);
        if flags.include_app_settings && !c.meta.is_empty() {
            summary.restored_preferences +=
                upsert_meta(pool, "category_meta", "category_ref", i64::from(id), &c.meta).await?;
        }
    }

    // 追踪器凭据整表覆盖写回（导出带的那一节）。没有凭据节就什么都不做，
    // 不会把本机已登录的追踪器登出。凭据归「敏感设置」，与导出侧同一条件。
    if flags.include_tracking && flags.include_private_settings {
        for c in &backup.tracker_credentials {
            suwayomi_db::query(
                "INSERT INTO tracker_credential (tracker_id, username, password, token, token_expired, score_type, pkce_verifier) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7) \
                 ON CONFLICT (tracker_id) DO UPDATE SET username = $2, password = $3, token = $4, \
                 token_expired = $5, score_type = $6, pkce_verifier = $7",
            )
            .bind(c.tracker_id)
            .bind(&c.username)
            .bind(&c.password)
            .bind(&c.token)
            .bind(c.token_expired)
            .bind(&c.score_type)
            .bind(&c.pkce_verifier)
            .execute(pool)
            .await?;
        }
    }

    // 2) ensure an extension row exists (source.extension FK — a violation
    //    would terminate the embedded session)
    let ext_id: i32 = match suwayomi_db::query_scalar::<i32>("SELECT id FROM extension ORDER BY id LIMIT 1")
        .fetch_optional(pool)
        .await?
    {
        Some(id) => id,
        None => {
            suwayomi_db::query_scalar(
                "INSERT INTO extension (name, pkg_name, version_name, version_code, lang, content_warning) \
             VALUES ('restored', 'org.suwayomi.restored', '0.0.0', 0, 'en', 0) RETURNING id",
            )
            .fetch_one(pool)
            .await?
        }
    };

    // 3) restore each manga
    let now_secs = chrono::Utc::now().timestamp();
    let manga_to_restore: &[BackupManga] = if flags.include_manga { &backup.backup_manga } else { &[] };
    for m in manga_to_restore {
        // ensure source exists
        let source_exists: bool =
            suwayomi_db::query_scalar::<bool>("SELECT EXISTS(SELECT 1 FROM source WHERE id = $1)")
                .bind(m.source)
                .fetch_one(pool)
                .await?;
        if !source_exists {
            let name = source_names.get(&m.source).cloned().unwrap_or_else(|| format!("source-{}", m.source));
            suwayomi_db::query("INSERT INTO source (id, name, lang, extension) VALUES ($1, $2, 'en', $3)")
                .bind(m.source)
                .bind(name)
                .bind(ext_id)
                .execute(pool)
                .await?;
        }

        // `BackupManga.favorite` 就是「在不在书库」：Mihon 的 `Manga.favorite` 派生自
        // `favoriteAt != null`，恢复时 `favoriteAt` 又正是由这个字段决定
        // （`BackupManga.kt`）。写死 TRUE 会把 `readEntries` 带出来的非库已读作品
        // 全部塞进书库。
        let in_library = m.favorite;
        // 非库条目没有「入库时间」：Mihon 导出时 `favoriteAt` 为 null（文件里是 0）。
        let added_secs = if in_library { if m.date_added > 0 { m.date_added / 1000 } else { now_secs } } else { 0 };
        let genre_new = m.genre.join(", ");
        let strategy_new = update_strategy_name(m.update_strategy).to_string();

        // find-or-insert manga by (url, source). When the row already exists,
        // only UPDATE when something actually changes: manga/chapter tables
        // have BEFORE UPDATE triggers that stamp last_modified_at and bump
        // version, so re-importing an identical backup must skip them or the
        // exported file would change on every restore→export cycle.
        let existing: Option<ExistingMangaRow> = suwayomi_db::query_as(
            "SELECT id, artist, author, description, genre, status, thumbnail_url, update_strategy, in_library_at, initialized, in_library \
             FROM manga WHERE url = $1 AND source = $2",
        )
            .bind(&m.url)
            .bind(m.source)
            .fetch_optional(pool)
            .await?;
        let manga_id = if let Some((
            id,
            cur_artist,
            cur_author,
            cur_desc,
            cur_genre,
            cur_status,
            cur_thumb,
            cur_strategy,
            cur_added,
            cur_init,
            cur_inlib,
        )) = existing
        {
            let dirty = m.artist.as_deref().is_some_and(|v| cur_artist.as_deref() != Some(v))
                || m.author.as_deref().is_some_and(|v| cur_author.as_deref() != Some(v))
                || m.description.as_deref().is_some_and(|v| cur_desc.as_deref() != Some(v))
                || (!genre_new.is_empty() && cur_genre.as_deref() != Some(genre_new.as_str()))
                || m.status != cur_status
                || m.thumbnail_url.as_deref().is_some_and(|v| cur_thumb.as_deref() != Some(v))
                || cur_strategy != strategy_new
                || cur_inlib != in_library
                || cur_added != Some(added_secs)
                || (m.description.is_some() && !cur_init);
            if dirty {
                suwayomi_db::query(
                    "UPDATE manga SET artist = COALESCE($1, artist), author = COALESCE($2, author), \
                     description = COALESCE($3, description), genre = COALESCE(NULLIF($4, ''), genre), \
                     status = $5, thumbnail_url = COALESCE($6, thumbnail_url), update_strategy = $7, \
                     in_library = $8, in_library_at = $9, \
                     initialized = initialized OR $10 WHERE id = $11",
                )
                .bind(&m.artist)
                .bind(&m.author)
                .bind(&m.description)
                .bind(&genre_new)
                .bind(m.status)
                .bind(&m.thumbnail_url)
                .bind(&strategy_new)
                .bind(in_library)
                .bind(added_secs)
                .bind(m.description.is_some())
                .bind(id)
                .execute(pool)
                .await?;
            }
            id
        } else {
            let id: i32 = suwayomi_db::query_scalar(
                "INSERT INTO manga (url, title, artist, author, description, genre, status, thumbnail_url, \
                 update_strategy, source, initialized, in_library, in_library_at, last_modified_at, version) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15) RETURNING id",
            )
            .bind(&m.url)
            .bind(&m.title)
            .bind(&m.artist)
            .bind(&m.author)
            .bind(&m.description)
            .bind(m.genre.join(", "))
            .bind(m.status)
            .bind(&m.thumbnail_url)
            .bind(update_strategy_name(m.update_strategy))
            .bind(m.source)
            .bind(m.description.is_some())
            .bind(in_library)
            .bind(added_secs)
            .bind(m.last_modified_at)
            .bind(m.version)
            .fetch_one(pool)
            .await?;
            summary.restored_manga += 1;
            id
        };

        // chapters (upsert on (url, manga)). `source_order` needs no
        // conversion: the row and a `.tachibk` both use Mihon's 0-based
        // numbering.
        let mut chapter_ids: Vec<i32> = Vec::new();
        let chapters_to_restore: &[BackupChapter] = if flags.include_chapters { &m.chapters } else { &[] };
        for ch in chapters_to_restore {
            let existing_ch: Option<ExistingChapterRow> = suwayomi_db::query_as(
                "SELECT id, name, scanlator, read, bookmark, last_page_read, date_upload, chapter_number::float4, source_order \
                 FROM chapter WHERE url = $1 AND manga = $2",
            )
                .bind(&ch.url)
                .bind(manga_id)
                .fetch_optional(pool)
                .await?;
            let chapter_id = if let Some((
                cid,
                cur_name,
                cur_scan,
                cur_read,
                cur_book,
                cur_lpr,
                cur_upload,
                cur_number,
                cur_order,
            )) = existing_ch
            {
                let dirty = cur_name != ch.name
                    || cur_scan.as_deref() != ch.scanlator.as_deref()
                    || cur_read != ch.read
                    || cur_book != ch.bookmark
                    || cur_lpr != ch.last_page_read
                    || cur_upload != ch.date_upload
                    || cur_number != ch.chapter_number
                    || cur_order != ch.source_order;
                if dirty {
                    suwayomi_db::query(
                        "UPDATE chapter SET name = $1, scanlator = $2, read = $3, bookmark = $4, last_page_read = $5, \
                         date_upload = $6, chapter_number = $7, source_order = $8 WHERE id = $9",
                    )
                    .bind(&ch.name)
                    .bind(&ch.scanlator)
                    .bind(ch.read)
                    .bind(ch.bookmark)
                    .bind(ch.last_page_read)
                    .bind(ch.date_upload)
                    .bind(ch.chapter_number)
                    .bind(ch.source_order)
                    .bind(cid)
                    .execute(pool)
                    .await?;
                }
                cid
            } else {
                let cid: i32 = suwayomi_db::query_scalar(
                    "INSERT INTO chapter (url, name, scanlator, read, bookmark, last_page_read, date_upload, \
                     chapter_number, source_order, manga) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING id",
                )
                .bind(&ch.url)
                .bind(&ch.name)
                .bind(&ch.scanlator)
                .bind(ch.read)
                .bind(ch.bookmark)
                .bind(ch.last_page_read)
                .bind(ch.date_upload)
                .bind(ch.chapter_number)
                .bind(ch.source_order)
                .bind(manga_id)
                .fetch_one(pool)
                .await?;
                summary.restored_chapters += 1;
                cid
            };
            chapter_ids.push(chapter_id);
            if flags.include_app_settings && !ch.meta.is_empty() {
                summary.restored_preferences +=
                    upsert_meta(pool, "chapter_meta", "chapter_ref", i64::from(chapter_id), &ch.meta).await?;
            }
        }

        // category membership (backup index -> db id via mapping)
        let category_indexes: &[i32] = if flags.include_categories { &m.categories } else { &[] };
        for cidx in category_indexes {
            if let Some(db_cat) = category_mapping.get(cidx) {
                let _ = suwayomi_db::query("INSERT INTO category_manga (category, manga) VALUES ($1, $2) ON CONFLICT (manga, category) DO NOTHING")
                    .bind(db_cat)
                    .bind(manga_id)
                    .execute(pool)
                    .await;
            }
        }

        // history: match chapter by url, apply last_page_read / last_read_at
        let history_to_restore: &[BackupHistory] = if flags.include_history { &m.history } else { &[] };
        for h in history_to_restore {
            let _ = suwayomi_db::query(
                "UPDATE chapter SET last_page_read = $1, last_read_at = $2 WHERE url = $3 AND manga = $4",
            )
            .bind(h.last_read as i32)
            .bind(h.read_at / 1000)
            .bind(&h.url)
            .bind(manga_id)
            .execute(pool)
            .await;
        }

        if flags.include_tracking {
            restore_manga_tracker_data(pool, manga_id, &m.tracking).await?;
        }

        if flags.include_app_settings && !m.meta.is_empty() {
            summary.restored_preferences +=
                upsert_meta(pool, "manga_meta", "manga_ref", i64::from(manga_id), &m.meta).await?;
        }

        let _ = chapter_ids;
    }

    // 图源 meta（9000）挂在 `BackupSource` 上，与作品的 meta 同一开关。
    if flags.include_app_settings {
        for s in &backup.backup_sources {
            if !s.meta.is_empty() {
                summary.restored_preferences +=
                    upsert_meta(pool, "source_meta", "source_ref", s.source_id, &s.meta).await?;
            }
        }
    }

    // 插件仓库（106）：按 index_url upsert，不动本机已有的其它仓库行。
    if flags.include_extension_stores {
        for store in &backup.backup_extension_stores {
            let existing: Option<i32> =
                suwayomi_db::query_scalar("SELECT id FROM extension_store WHERE index_url = $1")
                    .bind(&store.index_url)
                    .fetch_optional(pool)
                    .await?;
            let badge_label = store.badge_label.clone().unwrap_or_default();
            let is_legacy = store.is_legacy.unwrap_or(false);
            if let Some(id) = existing {
                suwayomi_db::query(
                    "UPDATE extension_store SET name = $1, badge_label = $2, signing_key = $3, \
                     contact_website = $4, contact_discord = $5, is_legacy = $6, extension_list_url = $7 \
                     WHERE id = $8",
                )
                .bind(&store.name)
                .bind(&badge_label)
                .bind(&store.signing_key)
                .bind(&store.contact_website)
                .bind(&store.contact_discord)
                .bind(is_legacy)
                .bind(&store.extension_list_url)
                .bind(id)
                .execute(pool)
                .await?;
            } else {
                suwayomi_db::query(
                    "INSERT INTO extension_store (index_url, name, badge_label, signing_key, contact_website, \
                     contact_discord, is_legacy, extension_list_url) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                )
                .bind(&store.index_url)
                .bind(&store.name)
                .bind(&badge_label)
                .bind(&store.signing_key)
                .bind(&store.contact_website)
                .bind(&store.contact_discord)
                .bind(is_legacy)
                .bind(&store.extension_list_url)
                .execute(pool)
                .await?;
                summary.restored_extension_stores += 1;
            }
        }
    }

    // 客户端 meta（9000）：服务端自身状态（设置 blob / 同步位点 / 凭据）一律不写，
    // 否则导入一份别人的备份会顺手改掉本机设置。
    if flags.include_app_settings {
        summary.restored_preferences += upsert_global_meta(pool, &backup.meta).await?;
    }

    // 图源设置（105）写不进库也写不进沙盒，交给调用方（core 不依赖沙盒客户端）。
    if flags.include_source_settings {
        summary.source_preferences =
            filter_source_preferences(backup.backup_source_preferences.clone(), flags.include_private_settings);
    }

    Ok(summary)
}

/// 对应参考实现 `BackupMangaHandler.restoreMangaTrackerData`。
///
/// 只在「本地没有该追踪器的记录」时新增；已有记录时按参考实现的做法只并进
/// `remote_id` / `library_id` 与取大的 `last_chapter_read`，其余字段保留本机值。
/// 备份里本仓库不支持的追踪器（例如旧版备份里的 id）直接丢弃。
async fn restore_manga_tracker_data(pool: &Db, manga_id: i32, tracks: &[BackupTracking]) -> Result<(), BackupError> {
    if tracks.is_empty() {
        return Ok(());
    }
    let existing: Vec<TrackRecordRow> =
        suwayomi_db::query_as("SELECT * FROM track_record WHERE manga_id = $1").bind(manga_id).fetch_all(pool).await?;
    let existing_by_tracker: HashMap<i32, &TrackRecordRow> = existing.iter().map(|r| (r.sync_id, r)).collect();

    for t in tracks {
        if !SUPPORTED_TRACKER_IDS.contains(&t.sync_id) {
            continue;
        }
        match existing_by_tracker.get(&t.sync_id) {
            None => {
                suwayomi_db::query(
                    "INSERT INTO track_record (manga_id, sync_id, remote_id, library_id, title, last_chapter_read, \
                     total_chapters, status, score, remote_url, start_date, finish_date, private) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
                )
                .bind(manga_id)
                .bind(t.sync_id)
                .bind(t.media_id)
                .bind(if t.library_id == 0 { None } else { Some(t.library_id) })
                .bind(&t.title)
                .bind(t.last_chapter_read as f64)
                .bind(t.total_chapters)
                .bind(t.status)
                .bind(t.score as f64)
                .bind(&t.tracking_url)
                .bind(t.started_reading_date)
                .bind(t.finished_reading_date)
                .bind(t.private)
                .execute(pool)
                .await?;
            }
            Some(db) => {
                let remote_id = t.media_id;
                let library_id = if t.library_id == 0 { db.library_id } else { Some(t.library_id) };
                let last_chapter_read = db.last_chapter_read.max(t.last_chapter_read as f64);
                suwayomi_db::query(
                    "UPDATE track_record SET remote_id = $1, library_id = $2, last_chapter_read = $3 WHERE id = $4",
                )
                .bind(remote_id)
                .bind(library_id)
                .bind(last_chapter_read)
                .bind(db.id)
                .execute(pool)
                .await?;
            }
        }
    }
    Ok(())
}

/// Maps the 0.x update-strategy ordinal back to the DB enum name.
fn update_strategy_name(ordinal: i32) -> &'static str {
    match ordinal {
        1 => "ALWAYS_FETCH",
        _ => "ALWAYS_UPDATE",
    }
}

/// 一张 `*_meta` 表的全部行，按 ref 分组。
async fn load_ref_meta(
    pool: &Db,
    table: &str,
    ref_column: &str,
) -> Result<HashMap<i64, HashMap<String, String>>, BackupError> {
    let rows = suwayomi_db::query(&format!("SELECT {ref_column} AS ref_id, meta_key, value FROM {table}"))
        .fetch_all(pool)
        .await?;
    let mut out: HashMap<i64, HashMap<String, String>> = HashMap::new();
    for row in rows {
        out.entry(row.try_get("ref_id")?).or_default().insert(row.try_get("meta_key")?, row.try_get("value")?);
    }
    Ok(out)
}

/// `global_meta` 里属于客户端 meta 的部分：跳过服务端自身状态（见
/// [`is_internal_global_meta_key`]）。
async fn load_client_global_meta(pool: &Db) -> Result<HashMap<String, String>, BackupError> {
    let rows = suwayomi_db::query("SELECT meta_key, value FROM global_meta").fetch_all(pool).await?;
    let mut out = HashMap::new();
    for row in rows {
        let key: String = row.try_get("meta_key")?;
        if is_internal_global_meta_key(&key) {
            continue;
        }
        out.insert(key, row.try_get("value")?);
    }
    Ok(out)
}

/// 把一份 `key -> value` upsert 进带 ref 列的 `*_meta` 表，返回写入的键数。
async fn upsert_meta(
    pool: &Db,
    table: &str,
    ref_column: &str,
    ref_id: i64,
    meta: &HashMap<String, String>,
) -> Result<usize, BackupError> {
    for (key, value) in meta {
        let existing: Option<i32> =
            suwayomi_db::query_scalar(&format!("SELECT id FROM {table} WHERE {ref_column} = $1 AND meta_key = $2"))
                .bind(ref_id)
                .bind(key)
                .fetch_optional(pool)
                .await?;
        match existing {
            Some(id) => {
                suwayomi_db::query(&format!("UPDATE {table} SET value = $1 WHERE id = $2"))
                    .bind(value)
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
            None => {
                suwayomi_db::query(&format!("INSERT INTO {table} (meta_key, value, {ref_column}) VALUES ($1, $2, $3)"))
                    .bind(key)
                    .bind(value)
                    .bind(ref_id)
                    .execute(pool)
                    .await?;
            }
        }
    }
    Ok(meta.len())
}

/// [`upsert_meta`] 的无 ref 版本（`global_meta` 只有 `meta_key` / `value`）。
async fn upsert_global_meta(pool: &Db, meta: &HashMap<String, String>) -> Result<usize, BackupError> {
    let mut written = 0;
    for (key, value) in meta {
        if is_internal_global_meta_key(key) {
            continue;
        }
        let existing: Option<i32> = suwayomi_db::query_scalar("SELECT id FROM global_meta WHERE meta_key = $1")
            .bind(key)
            .fetch_optional(pool)
            .await?;
        match existing {
            Some(id) => {
                suwayomi_db::query("UPDATE global_meta SET value = $1 WHERE id = $2")
                    .bind(value)
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
            None => {
                suwayomi_db::query("INSERT INTO global_meta (meta_key, value) VALUES ($1, $2)")
                    .bind(key)
                    .bind(value)
                    .execute(pool)
                    .await?;
            }
        }
        written += 1;
    }
    Ok(written)
}

/// Builds the `Backup` protobuf message from the current database (no encoding).
pub async fn create_backup_proto(
    pool: &Db,
    flags: BackupFlags,
    inputs: BackupInputs<'_>,
) -> Result<Backup, BackupError> {
    build_backup(pool, flags, inputs).await
}

async fn build_backup(pool: &Db, flags: BackupFlags, inputs: BackupInputs<'_>) -> Result<Backup, BackupError> {
    let with_meta = flags.include_app_settings;
    let category_meta =
        if with_meta { load_ref_meta(pool, "category_meta", "category_ref").await? } else { HashMap::new() };

    let category_rows: Vec<CategoryRow> =
        suwayomi_db::query_as("SELECT * FROM category ORDER BY sort_order, id").fetch_all(pool).await?;
    let backup_categories: Vec<BackupCategory> = category_rows
        .iter()
        .map(|c| BackupCategory {
            name: c.name.clone(),
            order: c.sort_order,
            flags: c.include_in_update,
            version: c.version,
            uid: c.uid,
            last_modified_at: c.last_modified_at,
            meta: category_meta.get(&i64::from(c.id)).cloned().unwrap_or_default(),
        })
        .collect();

    let manga_meta = if with_meta { load_ref_meta(pool, "manga_meta", "manga_ref").await? } else { HashMap::new() };
    let chapter_meta =
        if with_meta { load_ref_meta(pool, "chapter_meta", "chapter_ref").await? } else { HashMap::new() };
    let source_meta = if with_meta { load_ref_meta(pool, "source_meta", "source_ref").await? } else { HashMap::new() };

    let manga_rows: Vec<MangaRow> = if flags.include_manga {
        if flags.include_read_entries {
            // 库内作品 + 「有已读章节但不在库」的作品（Mihon 的 `readEntries`）。
            // 库内的排前面，与 Mihon 的 `getFavorites() + getReadMangaNotInLibrary()`
            // 拼出来的顺序一致。
            suwayomi_db::query_as(
                "SELECT * FROM manga m WHERE m.in_library = TRUE \
                 OR EXISTS (SELECT 1 FROM chapter c WHERE c.manga = m.id AND c.read = TRUE) \
                 ORDER BY m.in_library DESC, m.id",
            )
            .fetch_all(pool)
            .await?
        } else {
            suwayomi_db::query_as("SELECT * FROM manga WHERE in_library = TRUE ORDER BY id").fetch_all(pool).await?
        }
    } else {
        Vec::new()
    };
    let mut backup_mangas: Vec<BackupManga> = Vec::with_capacity(manga_rows.len());
    let mut source_ids: Vec<i64> = Vec::new();
    for m in &manga_rows {
        let chapters: Vec<ChapterRow> = if flags.include_chapters {
            suwayomi_db::query_as("SELECT * FROM chapter WHERE manga = $1 ORDER BY source_order")
                .bind(m.id)
                .fetch_all(pool)
                .await?
        } else {
            Vec::new()
        };
        let backup_chapters = chapters
            .iter()
            .map(|c| BackupChapter {
                url: c.url.clone(),
                name: c.name.clone(),
                scanlator: c.scanlator.clone(),
                read: c.read,
                bookmark: c.bookmark,
                last_page_read: c.last_page_read,
                date_fetch: c.fetched_at,
                date_upload: c.date_upload,
                chapter_number: c.chapter_number,
                // Written as-is: the row already carries Mihon's 0-based
                // `sourceOrder`, which is what a `.tachibk` stores, so a phone
                // backup and a re-export of a restored library stay
                // numerically identical.
                source_order: c.source_order,
                last_modified_at: c.last_modified_at,
                version: c.version,
                memo: c.memo.clone().into_bytes(),
                meta: chapter_meta.get(&i64::from(c.id)).cloned().unwrap_or_default(),
            })
            .collect();
        // BackupManga.categories stores the category ORDER (not id) —
        // mirrors Kotlin: `categoryMapping[it]` keys on `BackupCategory.order`.
        let category_orders: Vec<i32> = if flags.include_categories {
            suwayomi_db::query_scalar(
                "SELECT c.sort_order FROM category_manga cm JOIN category c ON c.id = cm.category WHERE cm.manga = $1",
            )
            .bind(m.id)
            .fetch_all(pool)
            .await?
        } else {
            Vec::new()
        };
        let tracking: Vec<BackupTracking> = if flags.include_tracking {
            suwayomi_db::query_as::<TrackRecordRow>("SELECT * FROM track_record WHERE manga_id = $1 ORDER BY id")
                .bind(m.id)
                .fetch_all(pool)
                .await?
                .iter()
                .map(backup_tracking_of)
                .collect()
        } else {
            Vec::new()
        };
        let genres: Vec<String> = m
            .genre
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(|g| g.trim().to_string())
            .filter(|g| !g.is_empty())
            .collect();
        backup_mangas.push(BackupManga {
            source: m.source,
            url: m.url.clone(),
            title: m.title.clone(),
            artist: m.artist.clone(),
            author: m.author.clone(),
            description: m.description.clone(),
            genre: genres,
            status: m.status,
            thumbnail_url: m.thumbnail_url.clone(),
            date_added: m.in_library_at * 1000, // Kotlin exports epoch MILLISECONDS
            viewer: 0,
            chapters: backup_chapters,
            categories: category_orders,
            tracking,
            favorite: m.in_library,
            chapter_flags: 0,
            viewer_flags: None,
            history: vec![],
            update_strategy: update_strategy_ordinal(&m.update_strategy),
            last_modified_at: m.last_modified_at,
            version: m.version,
            initialized: m.initialized,
            memo: m.memo.clone().into_bytes(),
            meta: manga_meta.get(&i64::from(m.id)).cloned().unwrap_or_default(),
        });
        if !source_ids.contains(&m.source) {
            source_ids.push(m.source);
        }
    }

    let mut backup_sources: Vec<BackupSource> = Vec::with_capacity(source_ids.len());
    for sid in &source_ids {
        let name: Option<String> =
            suwayomi_db::query_scalar("SELECT name FROM source WHERE id = $1").bind(sid).fetch_optional(pool).await?;
        if let Some(name) = name {
            backup_sources.push(BackupSource {
                name,
                source_id: *sid,
                meta: source_meta.get(sid).cloned().unwrap_or_default(),
            });
        }
    }

    // 凭据跟着「敏感设置」而不是「追踪」走：勾了追踪但没勾敏感设置时，绑定关系
    // （Tracking 那一节）照样带走，用户名/令牌留下。
    let tracker_credentials: Vec<BackupTrackerCredential> = if flags.include_tracking && flags.include_private_settings
    {
        suwayomi_db::query_as::<(i32, String, String, String, bool, String, String)>(
            "SELECT tracker_id, username, password, token, token_expired, score_type, pkce_verifier \
                 FROM tracker_credential ORDER BY tracker_id",
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|(tracker_id, username, password, token, token_expired, score_type, pkce_verifier)| {
            BackupTrackerCredential { tracker_id, username, password, token, token_expired, score_type, pkce_verifier }
        })
        .collect()
    } else {
        Vec::new()
    };

    let extension_stores: Vec<BackupExtensionStore> = if flags.include_extension_stores {
        suwayomi_db::query_as::<ExtensionStoreRow>("SELECT * FROM extension_store ORDER BY id")
            .fetch_all(pool)
            .await?
            .iter()
            .map(|s| BackupExtensionStore {
                index_url: s.index_url.clone(),
                name: s.name.clone(),
                badge_label: Some(s.badge_label.clone()),
                contact_website: s.contact_website.clone(),
                signing_key: s.signing_key.clone(),
                contact_discord: s.contact_discord.clone(),
                is_legacy: Some(s.is_legacy),
                extension_list_url: s.extension_list_url.clone(),
            })
            .collect()
    } else {
        Vec::new()
    };

    let server_settings = if flags.include_app_settings {
        inputs.server_config.map(|c| BackupServerSettings {
            // ip / port 是机器局部的：恢复到另一台机器会直接改掉监听地址。
            ip: String::new(),
            port: 0,
            initial_open_in_browser_enabled: c.initial_open_in_browser_enabled,
            auth_mode: c.auth_mode.clone(),
            // 凭据归 include_private_settings，见 [`BackupFlags`]。
            auth_username: if flags.include_private_settings { c.auth_username.clone() } else { String::new() },
            auth_password: if flags.include_private_settings { c.auth_password.clone() } else { String::new() },
            use_hikari_connection_pool: c.use_hikari_connection_pool,
        })
    } else {
        None
    };

    let meta = if with_meta { load_client_global_meta(pool).await? } else { HashMap::new() };

    let source_preferences = if flags.include_source_settings {
        filter_source_preferences(inputs.source_preferences, flags.include_private_settings)
    } else {
        Vec::new()
    };

    Ok(Backup {
        backup_manga: backup_mangas,
        backup_categories,
        backup_sources,
        backup_source_preferences: source_preferences,
        backup_extension_stores: extension_stores,
        meta,
        server_settings,
        tracker_credentials,
    })
}

/// `track_record` 行 → `BackupTracking`。
fn backup_tracking_of(r: &TrackRecordRow) -> BackupTracking {
    BackupTracking {
        sync_id: r.sync_id,
        // 参考实现强制给 0 而不是 null：1.x 的字段是非空 long。
        library_id: r.library_id.unwrap_or(0),
        media_id_int: r.remote_id as i32,
        tracking_url: r.remote_url.clone(),
        title: r.title.clone(),
        last_chapter_read: r.last_chapter_read as f32,
        total_chapters: r.total_chapters,
        score: r.score as f32,
        status: r.status,
        started_reading_date: r.start_date,
        finished_reading_date: r.finish_date,
        private: r.private,
        media_id: r.remote_id,
    }
}

/// `UpdateStrategy` enum ordinal (ALWAYS_UPDATE = 0, ALWAYS_FETCH = 1),
/// matching kotlinx-protobuf enum encoding in the 0.x backup format.
fn update_strategy_ordinal(s: &str) -> i32 {
    match s {
        "ALWAYS_FETCH" => 1,
        _ => 0,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("sqlx error: {0}")]
    Sqlx(#[from] suwayomi_db::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("protobuf decode error: {0}")]
    Decode(String),
}

/// Serializes the current database into a gzipped `Backup` protobuf payload.
pub async fn create_backup(pool: &Db, flags: BackupFlags, inputs: BackupInputs<'_>) -> Result<Vec<u8>, BackupError> {
    use std::io::Write;

    let backup = create_backup_proto(pool, flags, inputs).await?;
    let bytes = backup.encode_to_vec();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&bytes)?;
    let gz = encoder.finish()?;
    Ok(gz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use std::io::{Read, Write};

    async fn seed() -> Db {
        let db = Db::sqlite_in_memory().await.expect("connect");
        db.migrate().await.expect("migrate");
        let pool = db.pool();
        suwayomi_db::query("INSERT INTO extension (name, pkg_name, version_name, version_code, lang, content_warning) VALUES ('E','p','1',1,'en',0)")
            .execute(pool)
            .await
            .expect("ext");
        suwayomi_db::query("INSERT INTO source (name, lang, extension) VALUES ('MangaDex','en',1)")
            .execute(pool)
            .await
            .expect("src");
        suwayomi_db::query(
            "INSERT INTO manga (url, title, author, genre, status, thumbnail_url, in_library, source, initialized) \
             VALUES ('/m/1','Backup Manga','Author','Action, Drama',1,'https://t.jpg',TRUE,1,TRUE)",
        )
        .execute(pool)
        .await
        .expect("manga");
        // `source_order` is 0-based (Mihon), the same numbering a `.tachibk`
        // stores — the file is written and read back verbatim.
        suwayomi_db::query(
            "INSERT INTO chapter (url, name, chapter_number, source_order, read, last_page_read, manga) \
             VALUES ('/m/1/c/1','Ch 1',1.0,0,TRUE,3,1)",
        )
        .execute(pool)
        .await
        .expect("chapter");
        suwayomi_db::query(
            "INSERT INTO chapter (url, name, chapter_number, source_order, read, last_page_read, manga) \
             VALUES ('/m/1/c/2','Ch 2',2.0,1,FALSE,0,1)",
        )
        .execute(pool)
        .await
        .expect("chapter 2");
        suwayomi_db::query("INSERT INTO category (name, sort_order) VALUES ('Cat',1)")
            .execute(pool)
            .await
            .expect("category");
        suwayomi_db::query("INSERT INTO category_manga (category, manga) VALUES (1,1)")
            .execute(pool)
            .await
            .expect("cm");
        db
    }

    #[tokio::test]
    async fn backup_roundtrip_preserves_manga() {
        let db = seed().await;
        let gz =
            create_backup(db.pool(), BackupFlags::default(), BackupInputs::default()).await.expect("create backup");
        let mut decoder = flate2::read::GzDecoder::new(gz.as_slice());
        let mut raw = Vec::new();
        decoder.read_to_end(&mut raw).expect("gunzip");
        let backup = Backup::decode(raw.as_slice()).expect("decode proto");

        assert_eq!(backup.backup_manga.len(), 1);
        let manga = &backup.backup_manga[0];
        assert_eq!(manga.title, "Backup Manga");
        assert_eq!(manga.source, 1);
        assert_eq!(manga.genre, vec!["Action".to_string(), "Drama".to_string()]);
        assert!(manga.favorite, "in-library manga is a favorite");
        assert_eq!(manga.categories, vec![1]);
        assert_eq!(manga.chapters.len(), 2);
        assert_eq!(manga.chapters[0].name, "Ch 1");
        assert!(manga.chapters[0].read);
        assert_eq!(manga.chapters[0].last_page_read, 3);
        // exported verbatim: no re-basing between the row and the file
        assert_eq!(manga.chapters[0].source_order, 0);
        assert_eq!(manga.chapters[1].source_order, 1);
        assert_eq!(backup.backup_categories.len(), 1);
        assert_eq!(backup.backup_categories[0].name, "Cat");
        assert_eq!(backup.backup_sources.len(), 1);
        assert_eq!(backup.backup_sources[0].name, "MangaDex");
    }

    #[tokio::test]
    async fn backup_empty_library_is_valid() {
        let db = Db::sqlite_in_memory().await.expect("connect");
        db.migrate().await.expect("migrate");
        let gz =
            create_backup(db.pool(), BackupFlags::default(), BackupInputs::default()).await.expect("create backup");
        let mut decoder = flate2::read::GzDecoder::new(gz.as_slice());
        let mut raw = Vec::new();
        decoder.read_to_end(&mut raw).expect("gunzip");
        let backup = Backup::decode(raw.as_slice()).expect("decode proto");
        assert!(backup.backup_manga.is_empty());
    }

    /// Export → wipe → restore on a fresh DB: data must round-trip.
    #[tokio::test]
    async fn export_restore_roundtrip() {
        let db = seed().await;
        let gz =
            create_backup(db.pool(), BackupFlags::default(), BackupInputs::default()).await.expect("create backup");

        // restore into a fresh embedded database
        let fresh = Db::sqlite_in_memory().await.expect("connect fresh");
        fresh.migrate().await.expect("migrate fresh");
        let summary = restore_backup(fresh.pool(), &gz, BackupFlags::default()).await.expect("restore");

        assert_eq!(summary.restored_manga, 1);
        assert_eq!(summary.restored_chapters, 2);
        assert!(summary.missing_sources.is_empty(), "sources included in backup");

        // verify content
        let n: i64 =
            suwayomi_db::query_scalar("SELECT COUNT(*) FROM manga").fetch_one(fresh.pool()).await.expect("count manga");
        assert_eq!(n, 1);
        let title: String = suwayomi_db::query_scalar("SELECT title FROM manga WHERE id = 1")
            .fetch_one(fresh.pool())
            .await
            .expect("title");
        assert_eq!(title, "Backup Manga");
        let in_lib: bool = suwayomi_db::query_scalar("SELECT in_library FROM manga WHERE id = 1")
            .fetch_one(fresh.pool())
            .await
            .expect("in_library");
        assert!(in_lib, "favorite manga restored as in-library");
        let ch: i64 = suwayomi_db::query_scalar("SELECT COUNT(*) FROM chapter WHERE manga = 1")
            .fetch_one(fresh.pool())
            .await
            .expect("count chapters");
        assert_eq!(ch, 2);
        // The file's 0-based `sourceOrder` lands in the row unchanged (and was
        // written from the row unchanged): assert the round-trip value, not
        // just the count — a re-basing shim on either side would shift these.
        let orders: Vec<i32> =
            suwayomi_db::query_scalar("SELECT source_order FROM chapter WHERE manga = 1 ORDER BY source_order")
                .fetch_all(fresh.pool())
                .await
                .expect("source_order");
        assert_eq!(orders, vec![0, 1], "source_order round-trips unchanged");
        let cm: i64 = suwayomi_db::query_scalar("SELECT COUNT(*) FROM category_manga WHERE manga = 1")
            .fetch_one(fresh.pool())
            .await
            .expect("count cm");
        assert_eq!(cm, 1, "category membership restored");
        let cat: String = suwayomi_db::query_scalar("SELECT name FROM category WHERE id = 1")
            .fetch_one(fresh.pool())
            .await
            .expect("category");
        assert_eq!(cat, "Cat");
        let src: String = suwayomi_db::query_scalar("SELECT name FROM source WHERE id = 1")
            .fetch_one(fresh.pool())
            .await
            .expect("source");
        assert_eq!(src, "MangaDex");
    }

    /// tracking 与凭据要跟着备份往返（`BackupTracking` 走 manga 节，凭据走
    /// Suwayomi 自留的 9002 节）；备份里本仓库不支持的追踪器不进导出也不恢复。
    #[tokio::test]
    async fn export_restore_roundtrip_tracking() {
        let db = seed().await;
        let pool = db.pool();
        suwayomi_db::query(
            "INSERT INTO track_record (manga_id, sync_id, remote_id, library_id, title, last_chapter_read, \
             total_chapters, status, score, remote_url, start_date, finish_date, private) \
             VALUES (1, 2, 12345, 99, 'Bound', 3.5, 10, 3, 8, 'https://anilist.co/manga/12345', 100, 0, TRUE)",
        )
        .execute(pool)
        .await
        .expect("track record");
        // id 6 不是本仓库支持的追踪器，应被丢弃。
        suwayomi_db::query(
            "INSERT INTO track_record (manga_id, sync_id, remote_id, title, last_chapter_read, total_chapters, \
             status, score, remote_url, start_date, finish_date, private) \
             VALUES (1, 6, 7, 'Unsupported', 0, 0, 0, 0, '', 0, 0, FALSE)",
        )
        .execute(pool)
        .await
        .expect("unsupported track record");
        suwayomi_db::query(
            "INSERT INTO tracker_credential (tracker_id, username, password, token) VALUES (2, 'user', 'tok', '{}')",
        )
        .execute(pool)
        .await
        .expect("credential");

        // 凭据归「敏感设置」，默认关闭；这里显式打开才带走。
        let with_private = BackupFlags { include_private_settings: true, ..Default::default() };
        let gz = create_backup(pool, with_private, BackupInputs::default()).await.expect("create backup");
        let backup = decode_gz_backup(&gz).expect("decode");
        assert_eq!(backup.backup_manga[0].tracking.len(), 2, "导出带上全部 track_record");
        assert_eq!(backup.backup_manga[0].tracking[0].media_id, 12345);
        assert_eq!(backup.backup_manga[0].tracking[0].last_chapter_read, 3.5);
        assert_eq!(backup.tracker_credentials.len(), 1);

        // 只勾追踪、不勾敏感设置：绑定关系带走，凭据留下。
        let public_only =
            create_backup(pool, BackupFlags::default(), BackupInputs::default()).await.expect("create backup");
        let backup = decode_gz_backup(&public_only).expect("decode");
        assert_eq!(backup.backup_manga[0].tracking.len(), 2);
        assert!(backup.tracker_credentials.is_empty(), "敏感设置关闭时不导出凭据");

        // 关掉 tracking 之后两份数据都不带走
        let no_tracking = create_backup(
            pool,
            BackupFlags { include_tracking: false, include_private_settings: true, ..Default::default() },
            BackupInputs::default(),
        )
        .await
        .expect("create backup");
        let backup = decode_gz_backup(&no_tracking).expect("decode");
        assert!(backup.backup_manga[0].tracking.is_empty());
        assert!(backup.tracker_credentials.is_empty());

        let fresh = Db::sqlite_in_memory().await.expect("connect fresh");
        fresh.migrate().await.expect("migrate fresh");
        restore_backup(fresh.pool(), &gz, with_private).await.expect("restore");

        let (remote_id, last_chapter_read, private): (i64, f64, bool) = suwayomi_db::query_as(
            "SELECT remote_id, last_chapter_read, private FROM track_record WHERE manga_id = 1 AND sync_id = 2",
        )
        .fetch_one(fresh.pool())
        .await
        .expect("track record restored");
        assert_eq!(remote_id, 12345);
        assert_eq!(last_chapter_read, 3.5);
        assert!(private);
        let unsupported: i64 = suwayomi_db::query_scalar("SELECT COUNT(*) FROM track_record WHERE sync_id = 6")
            .fetch_one(fresh.pool())
            .await
            .expect("count");
        assert_eq!(unsupported, 0, "不支持的追踪器不进库");
        let (username, password): (String, String) =
            suwayomi_db::query_as("SELECT username, password FROM tracker_credential WHERE tracker_id = 2")
                .fetch_one(fresh.pool())
                .await
                .expect("credential restored");
        assert_eq!((username.as_str(), password.as_str()), ("user", "tok"));
    }

    /// A backup whose source is missing must be reported, not crash.
    #[tokio::test]
    async fn restore_reports_missing_source() {
        let mut manga = BackupManga {
            source: 999,
            url: "/m/x".into(),
            title: "Orphan".into(),
            favorite: true,
            ..Default::default()
        };
        manga.update_strategy = 0;
        let backup = Backup { backup_manga: vec![manga], ..Default::default() };
        let raw = backup.encode_to_vec();
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&raw).expect("write");
        let gz = enc.finish().expect("gz");

        let summary = validate_backup(&gz).await.expect("validate");
        assert_eq!(summary.missing_sources, vec!["999".to_string()]);
        assert_eq!(summary.mangas_missing_sources.len(), 1);

        // restore still works: source gets auto-created as a placeholder
        let db = Db::sqlite_in_memory().await.expect("connect");
        db.migrate().await.expect("migrate");
        let s = restore_backup(db.pool(), &gz, BackupFlags::default()).await.expect("restore");
        assert_eq!(s.restored_manga, 1);
        let src_name: Option<String> = suwayomi_db::query_scalar("SELECT name FROM source WHERE id = 999")
            .fetch_one(db.pool())
            .await
            .expect("src");
        assert_eq!(src_name.as_deref(), Some("source-999"));
    }

    /// 开关与 query 键必须一一对应：错位会让某个勾选静默失效（导出内容仍按默认值）。
    /// 断言用 true/false 交替的取值，任何两个键调换都会让字面量对不上。
    #[test]
    fn backup_flags_query_round_trips() {
        let flags = BackupFlags {
            include_manga: false,
            include_categories: true,
            include_chapters: false,
            include_tracking: true,
            include_history: false,
            include_read_entries: true,
            include_app_settings: false,
            include_extension_stores: true,
            include_source_settings: false,
            include_private_settings: true,
        };
        let query = flags.to_query_string();

        assert_eq!(
            query,
            "includeManga=false&includeCategories=true&includeChapters=false&includeTracking=true\
             &includeHistory=false&includeReadEntries=true&includeAppSettings=false\
             &includeExtensionStores=true&includeSourceSettings=false&includePrivateSettings=true"
        );

        let parsed =
            BackupFlags::from_query_pairs(query.split('&').map(|pair| pair.split_once('=').expect("key=value")));
        assert_eq!(parsed.to_array(), flags.to_array());
    }

    /// 手敲 URL、老客户端不带 query 时，缺键的开关沿用默认值（全选，敏感设置除外）。
    #[test]
    fn backup_flags_query_missing_keys_default() {
        let parsed = BackupFlags::from_query_pairs([("includeManga", "false"), ("nope", "false")]);
        assert!(!parsed.include_manga);
        assert!(parsed.include_categories);
        assert!(parsed.include_extension_stores);
        assert!(!parsed.include_private_settings, "缺键时敏感设置保持默认关闭");

        let invalid = BackupFlags::from_query_pairs([("includeManga", "1")]);
        assert!(invalid.include_manga, "非布尔值不改变默认");
    }

    /// 敏感与「程序状态」偏好按前缀过滤；一个源的设置被过滤空之后整个源不进备份
    /// —— Mihon 的 `.filter { it.prefs.isNotEmpty() }` 也是这个效果。
    #[test]
    fn source_preferences_drop_private_and_app_state_keys() {
        let groups = vec![BackupSourcePreferences {
            source_key: "source_1".to_string(),
            prefs: vec![
                pref("user", preference_text("alice")),
                pref("__PRIVATE_token", preference_text("secret")),
                pref("__APP_STATE_last", preference_text("cursor")),
                pref("pageCount", PreferenceValue { value: Some(preference_value::Value::Int(3)) }),
            ],
        }];
        let kept = filter_source_preferences(groups.clone(), false);
        assert_eq!(
            kept[0].prefs.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(),
            vec!["user", "pageCount"],
            "默认不带敏感项，程序状态键永远不带"
        );

        let kept = filter_source_preferences(groups, true);
        assert_eq!(kept[0].prefs.len(), 3, "勾了敏感设置就带上 __PRIVATE_，但仍不含 __APP_STATE_");

        let only_state = vec![BackupSourcePreferences {
            source_key: "source_2".to_string(),
            prefs: vec![pref("__APP_STATE_x", preference_text("y"))],
        }];
        assert!(filter_source_preferences(only_state, true).is_empty(), "过滤后为空的源不进备份");
    }

    /// 105 / 106 的字段号必须与 Mihon 的 `@ProtoNumber` 一致：差一位就是另一个字段，
    /// 解出来的偏好类型会整体错位。
    #[test]
    fn source_preference_and_extension_store_wire_tags_match_mihon() {
        let pref = BackupPreference {
            key: "k".to_string(),
            value: Some(PreferenceValue { value: Some(preference_value::Value::Text("v".to_string())) }),
        };
        assert_eq!(
            pref.encode_to_vec(),
            vec![0x0a, 0x01, b'k', 0x12, 0x03, 0x22, 0x01, b'v'],
            "key 在 1 号字段，value 在 2 号字段，字符串值在 oneof 的 4 号"
        );

        let group = BackupSourcePreferences { source_key: "source_7".to_string(), prefs: vec![pref] };
        let mut encoded = Vec::new();
        group.encode(&mut encoded).expect("encode");
        assert_eq!(encoded[0], 0x0a, "sourceKey 是 1 号字段");
        assert!(encoded.windows(2).any(|w| w == [0x12, 8]), "prefs 是 2 号字段");

        let store = BackupExtensionStore {
            index_url: "https://repo".to_string(),
            name: "name".to_string(),
            badge_label: Some("18+".to_string()),
            contact_website: "https://site".to_string(),
            signing_key: "key".to_string(),
            contact_discord: Some("discord".to_string()),
            is_legacy: Some(true),
            extension_list_url: Some("https://list".to_string()),
        };
        let mut encoded = Vec::new();
        store.encode(&mut encoded).expect("encode");
        // 4 号是 contactWebsite、5 号是 signingKey —— 源码里它们是交错声明的。
        assert!(encoded.windows(2).any(|w| w == [0x22, 0x0c]), "contactWebsite 在 4 号");
        assert!(encoded.windows(2).any(|w| w == [0x2a, 0x03]), "signingKey 在 5 号");
    }

    /// 恢复时 `in_library` 跟随 `favorite`：把 `readEntries` 带出来的非库已读作品
    /// 塞进书库，会让往返不自洽（导出 1 条非库，再导出变成 1 条在库）。
    #[tokio::test]
    async fn restore_keeps_non_library_entries_out_of_the_library() {
        let db = seed().await;
        let pool = db.pool();
        // 有已读章节但不在库。
        suwayomi_db::query(
            "INSERT INTO manga (url, title, status, in_library, source, initialized) VALUES ('/m/2','Read Only',1,FALSE,1,TRUE)",
        )
        .execute(pool)
        .await
        .expect("manga");
        suwayomi_db::query(
            "INSERT INTO chapter (url, name, chapter_number, source_order, read, manga) VALUES ('/m/2/c/1','Ch 1',1.0,0,TRUE,2)",
        )
        .execute(pool)
        .await
        .expect("chapter");

        let gz = create_backup(pool, BackupFlags::default(), BackupInputs::default()).await.expect("create backup");
        let backup = decode_gz_backup(&gz).expect("decode");
        assert_eq!(backup.backup_manga.len(), 2, "readEntries 默认开启，非库已读作品也进备份");
        assert!(backup.backup_manga[0].favorite, "库内作品在前且 favorite=true");
        assert!(!backup.backup_manga[1].favorite, "非库已读作品 favorite=false");

        // 关掉 readEntries 之后只剩库内作品。
        let library_only = create_backup(
            pool,
            BackupFlags { include_read_entries: false, ..Default::default() },
            BackupInputs::default(),
        )
        .await
        .expect("create backup");
        assert_eq!(decode_gz_backup(&library_only).expect("decode").backup_manga.len(), 1);

        let fresh = Db::sqlite_in_memory().await.expect("connect fresh");
        fresh.migrate().await.expect("migrate fresh");
        let summary = restore_backup(fresh.pool(), &gz, BackupFlags::default()).await.expect("restore");
        assert_eq!(summary.restored_manga, 2);
        let in_lib: bool = suwayomi_db::query_scalar("SELECT in_library FROM manga WHERE url = '/m/2'")
            .fetch_one(fresh.pool())
            .await
            .expect("in_library");
        assert!(!in_lib, "非库已读作品恢复后仍在库外");
        let in_lib: bool = suwayomi_db::query_scalar("SELECT in_library FROM manga WHERE url = '/m/1'")
            .fetch_one(fresh.pool())
            .await
            .expect("in_library");
        assert!(in_lib, "库内作品恢复后仍在库");
    }

    /// 106 与 meta 各节往返；`global_meta` 里的服务端自身状态不随备份出去、也不被
    /// 备份写回。
    #[tokio::test]
    async fn extension_stores_and_meta_round_trip_without_server_state() {
        let db = seed().await;
        let pool = db.pool();
        suwayomi_db::query(
            "INSERT INTO extension_store (index_url, name, badge_label, signing_key, contact_website, is_legacy) \
             VALUES ('https://repo.example/index.min.json', 'Repo', '18+', 'k', 'https://repo.example', TRUE)",
        )
        .execute(pool)
        .await
        .expect("store");
        suwayomi_db::query("INSERT INTO manga_meta (meta_key, value, manga_ref) VALUES ('note', 'hello', 1)")
            .execute(pool)
            .await
            .expect("manga meta");
        suwayomi_db::query("INSERT INTO global_meta (meta_key, value) VALUES ('viewer_theme', 'dark')")
            .execute(pool)
            .await
            .expect("global meta");
        suwayomi_db::query("INSERT INTO global_meta (meta_key, value) VALUES ('settings', '{\"port\":1}')")
            .execute(pool)
            .await
            .expect("settings blob");
        suwayomi_db::query("INSERT INTO global_meta (meta_key, value) VALUES ('last_auto_backup_at', '123')")
            .execute(pool)
            .await
            .expect("autobackup cursor");

        let gz = create_backup(pool, BackupFlags::default(), BackupInputs::default()).await.expect("create backup");
        let backup = decode_gz_backup(&gz).expect("decode");
        assert_eq!(backup.backup_extension_stores.len(), 1);
        assert_eq!(backup.backup_extension_stores[0].index_url, "https://repo.example/index.min.json");
        assert_eq!(backup.backup_extension_stores[0].is_legacy, Some(true));
        assert_eq!(backup.backup_manga[0].meta.get("note").map(String::as_str), Some("hello"));
        assert_eq!(backup.meta.get("viewer_theme").map(String::as_str), Some("dark"));
        assert!(!backup.meta.contains_key("settings"), "设置 blob 不随备份出去");
        assert!(!backup.meta.contains_key("last_auto_backup_at"), "自动备份游标不随备份出去");

        let fresh = Db::sqlite_in_memory().await.expect("connect fresh");
        fresh.migrate().await.expect("migrate fresh");
        // 目标库先有一份本机设置，导入不能把它冲掉。
        suwayomi_db::query("INSERT INTO global_meta (meta_key, value) VALUES ('settings', '{\"port\":4567}')")
            .execute(fresh.pool())
            .await
            .expect("local settings");
        let summary = restore_backup(fresh.pool(), &gz, BackupFlags::default()).await.expect("restore");
        assert_eq!(summary.restored_extension_stores, 1);
        assert!(summary.restored_preferences >= 2);

        let note: String =
            suwayomi_db::query_scalar("SELECT value FROM manga_meta WHERE meta_key = 'note' AND manga_ref = 1")
                .fetch_one(fresh.pool())
                .await
                .expect("manga meta");
        assert_eq!(note, "hello");
        let theme: String = suwayomi_db::query_scalar("SELECT value FROM global_meta WHERE meta_key = 'viewer_theme'")
            .fetch_one(fresh.pool())
            .await
            .expect("global meta");
        assert_eq!(theme, "dark");
        let settings: String = suwayomi_db::query_scalar("SELECT value FROM global_meta WHERE meta_key = 'settings'")
            .fetch_one(fresh.pool())
            .await
            .expect("settings");
        assert_eq!(settings, "{\"port\":4567}", "备份不覆盖本机设置 blob");

        let store: String = suwayomi_db::query_scalar("SELECT name FROM extension_store WHERE is_legacy = TRUE")
            .fetch_one(fresh.pool())
            .await
            .expect("store");
        assert_eq!(store, "Repo");
    }

    /// 图源设置随 summary 交回调用方（core 够不到沙盒）。
    #[tokio::test]
    async fn restore_hands_source_preferences_to_the_caller() {
        let db = Db::sqlite_in_memory().await.expect("connect");
        db.migrate().await.expect("migrate");
        let backup = Backup {
            backup_source_preferences: vec![BackupSourcePreferences {
                source_key: "source_1".to_string(),
                prefs: vec![pref("user", preference_text("alice"))],
            }],
            ..Default::default()
        };
        let raw = backup.encode_to_vec();
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&raw).expect("write");
        let gz = enc.finish().expect("gz");

        let summary = restore_backup(db.pool(), &gz, BackupFlags::default()).await.expect("restore");
        assert_eq!(summary.source_preferences.len(), 1);
        assert_eq!(summary.source_preferences[0].source_key, "source_1");

        let off = restore_backup(db.pool(), &gz, BackupFlags { include_source_settings: false, ..Default::default() })
            .await
            .expect("restore");
        assert!(off.source_preferences.is_empty(), "关掉图源设置就不交回");
    }

    fn pref(key: &str, value: PreferenceValue) -> BackupPreference {
        BackupPreference { key: key.to_string(), value: Some(value) }
    }

    fn preference_text(s: &str) -> PreferenceValue {
        PreferenceValue { value: Some(preference_value::Value::Text(s.to_string())) }
    }
}
