//! Mihon/Suwayomi protobuf 备份（backup.proto）。消息结构为手写 prost derive
//! （无需 protoc），字段号对齐 kotlinx-protobuf @ProtoNumber。create_backup 由
//! 当前数据库构建 Backup 消息，返回 gzip 压缩的 protobuf 字节（.tachibk 载荷）。

use std::collections::HashMap;

use prost::Message;
use suwayomi_db::Db;

use crate::schema::{CategoryRow, ChapterRow, MangaRow, TrackRecordRow};

/// 备份里认得的追踪器 id —— 与 `suwayomi_domain::tracker` 的常量一致
/// （1 MAL / 2 AniList / 3 Kitsu / 4 Shikimori / 5 Bangumi / 7 MangaUpdates）。
///
/// 恢复时用来丢弃备份里本仓库不支持的追踪器记录（上游 `BackupMangaHandler`
/// 也是这么做的），否则会留下一条没有对应追踪器的 `track_record`。
/// `suwayomi-domain` 的测试会断言两份清单相等，防止单边漂移。
pub const SUPPORTED_TRACKER_IDS: [i32; 6] = [1, 2, 3, 4, 5, 7];

/// 备份内容开关（对应上游 `BackupFlags`）。
///
/// 默认全开，与上游 `BackupFlags.DEFAULT` 一致。`include_history` /
/// `include_client_data` / `include_server_settings` 目前是空操作：本仓库的导出
/// 还没有把 `BackupHistory`、manga/chapter meta、`BackupServerSettings` 填进去。
#[derive(Debug, Clone, Copy)]
pub struct BackupFlags {
    pub include_manga: bool,
    pub include_categories: bool,
    pub include_chapters: bool,
    pub include_tracking: bool,
    pub include_history: bool,
    pub include_client_data: bool,
    pub include_server_settings: bool,
}

impl Default for BackupFlags {
    fn default() -> Self {
        Self {
            include_manga: true,
            include_categories: true,
            include_chapters: true,
            include_tracking: true,
            include_history: true,
            include_client_data: true,
            include_server_settings: true,
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
    pub include_client_data: Option<bool>,
    pub include_server_settings: Option<bool>,
}

impl BackupFlags {
    /// 未指定的键沿用默认值（上游 `BackupFlags.fromPartial`）。
    pub fn from_partial(p: &PartialBackupFlags) -> Self {
        let d = Self::default();
        Self {
            include_manga: p.include_manga.unwrap_or(d.include_manga),
            include_categories: p.include_categories.unwrap_or(d.include_categories),
            include_chapters: p.include_chapters.unwrap_or(d.include_chapters),
            include_tracking: p.include_tracking.unwrap_or(d.include_tracking),
            include_history: p.include_history.unwrap_or(d.include_history),
            include_client_data: p.include_client_data.unwrap_or(d.include_client_data),
            include_server_settings: p.include_server_settings.unwrap_or(d.include_server_settings),
        }
    }
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
    #[prost(map = "string, string", tag = "9000")]
    pub meta: HashMap<String, String>,
    #[prost(message, optional, tag = "9001")]
    pub server_settings: Option<BackupServerSettings>,
    /// 追踪器凭据。上游把凭据放在客户端 SharedPreferences 里，备份格式没有这一节；
    /// 本仓库凭据落库（`tracker_credential`），用 Suwayomi 自留号段（9000+）带走。
    /// 其它客户端按 proto3 规则忽略未知字段。
    #[prost(message, repeated, tag = "9002")]
    pub tracker_credentials: Vec<BackupTrackerCredential>,
}

/// `tracker_credential` 表的一行。
#[derive(Clone, PartialEq, ::prost::Message)]
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

#[derive(Clone, PartialEq, ::prost::Message)]
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

#[derive(Clone, PartialEq, ::prost::Message)]
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

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BackupHistory {
    #[prost(string, tag = "1")]
    pub url: String,
    #[prost(int64, tag = "2")]
    pub last_read: i64,
    #[prost(int64, tag = "3")]
    pub read_at: i64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
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

// ---------------------------------------------------------------------------
// export
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// import / validate
// ---------------------------------------------------------------------------

/// Summary of a backup restore/validate run.
#[derive(Debug, Clone, Default)]
pub struct RestoreSummary {
    pub restored_manga: usize,
    pub restored_categories: usize,
    pub restored_chapters: usize,
    pub missing_sources: Vec<String>,
    pub mangas_missing_sources: Vec<String>,
    pub errors: Vec<String>,
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
    let name_of = |sid: i64| backup.backup_sources.iter().find(|s| s.source_id == sid).map(|s| s.name.clone()).unwrap_or_else(|| sid.to_string());
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
pub async fn restore_backup_proto(pool: &Db, backup: &Backup, flags: BackupFlags) -> Result<RestoreSummary, BackupError> {
    let mut summary = validate_backup_inner(backup);
    let source_names: HashMap<i64, String> = backup.backup_sources.iter().map(|s| (s.source_id, s.name.clone())).collect();

    // 1) categories: order -> id (reuse existing by name; mirrors Kotlin's
    //    `BackupCategory.order`-keyed mapping used by BackupManga.categories)
    let mut category_mapping: HashMap<i32, i32> = HashMap::new(); // category order -> db id
    for (idx, c) in backup.backup_categories.iter().enumerate() {
        if !flags.include_categories {
            break;
        }
        let existing: Option<i32> = suwayomi_db::query_scalar("SELECT id FROM category WHERE name = $1").bind(&c.name).fetch_optional(pool).await?;
        let id = match existing {
            Some(id) => id,
            None => {
                // Use a fresh, collision-free sort_order: keying membership by
                // numeric order is fine within one backup file, but reusing a
                // value already taken in the DB (e.g. several categories at
                // order 0) would make a later restore of our own export map
                // memberships onto the wrong category.
                let next_order: i32 =
                    suwayomi_db::query_scalar("SELECT COALESCE(MAX(sort_order), 0) + 1 FROM category").fetch_one(pool).await?;
                let id: i32 = suwayomi_db::query_scalar("INSERT INTO category (name, sort_order) VALUES ($1, $2) RETURNING id")
                    .bind(&c.name)
                    .bind(next_order)
                    .fetch_one(pool)
                    .await?;
                summary.restored_categories += 1;
                id
            }
        };
        let _ = idx;
        category_mapping.insert(c.order, id);
    }

    // 追踪器凭据整表覆盖写回（导出带的那一节）。没有凭据节就什么都不做，
    // 不会把本机已登录的追踪器登出。
    if flags.include_tracking {
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
    let ext_id: i32 = match suwayomi_db::query_scalar::<i32>("SELECT id FROM extension ORDER BY id LIMIT 1").fetch_optional(pool).await? {
        Some(id) => id,
        None => suwayomi_db::query_scalar(
            "INSERT INTO extension (name, pkg_name, version_name, version_code, lang, content_warning) \
             VALUES ('restored', 'org.suwayomi.restored', '0.0.0', 0, 'en', 0) RETURNING id",
        )
        .fetch_one(pool)
        .await?,
    };

    // 3) restore each manga
    let now_secs = chrono::Utc::now().timestamp();
    let manga_to_restore: &[BackupManga] = if flags.include_manga { &backup.backup_manga } else { &[] };
    for m in manga_to_restore {
        // ensure source exists
        let source_exists: bool = suwayomi_db::query_scalar::<bool>("SELECT EXISTS(SELECT 1 FROM source WHERE id = $1)")
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

        // Every manga in a Tachiyomi/Mihon/Suwayomi backup is a library
        // entry. `BackupManga.favorite` is NOT "in library" (on modern Mihon
        // it is a separate per-library bookmark), so the restore must mark
        // rows as in_library regardless — otherwise the restored library
        // stays empty and a follow-up export only contains the pre-existing
        // rows.
        let added_secs = if m.date_added > 0 { m.date_added / 1000 } else { now_secs };
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
        let manga_id = match existing {
            Some((id, cur_artist, cur_author, cur_desc, cur_genre, cur_status, cur_thumb, cur_strategy, cur_added, cur_init, cur_inlib)) => {
                let dirty = m.artist.as_deref().is_some_and(|v| cur_artist.as_deref() != Some(v))
                    || m.author.as_deref().is_some_and(|v| cur_author.as_deref() != Some(v))
                    || m.description.as_deref().is_some_and(|v| cur_desc.as_deref() != Some(v))
                    || (!genre_new.is_empty() && cur_genre.as_deref() != Some(genre_new.as_str()))
                    || m.status != cur_status
                    || m.thumbnail_url.as_deref().is_some_and(|v| cur_thumb.as_deref() != Some(v))
                    || cur_strategy != strategy_new
                    || !cur_inlib
                    || cur_added != Some(added_secs)
                    || (m.description.is_some() && !cur_init);
                if dirty {
                    suwayomi_db::query(
                        "UPDATE manga SET artist = COALESCE($1, artist), author = COALESCE($2, author), \
                         description = COALESCE($3, description), genre = COALESCE(NULLIF($4, ''), genre), \
                         status = $5, thumbnail_url = COALESCE($6, thumbnail_url), update_strategy = $7, \
                         in_library = TRUE, in_library_at = $8, \
                         initialized = initialized OR $9 WHERE id = $10",
                    )
                    .bind(&m.artist)
                    .bind(&m.author)
                    .bind(&m.description)
                    .bind(&genre_new)
                    .bind(m.status)
                    .bind(&m.thumbnail_url)
                    .bind(&strategy_new)
                    .bind(added_secs)
                    .bind(m.description.is_some())
                    .bind(id)
                    .execute(pool)
                    .await?;
                }
                id
            }
            None => {
                let id: i32 = suwayomi_db::query_scalar(
                    "INSERT INTO manga (url, title, artist, author, description, genre, status, thumbnail_url, \
                     update_strategy, source, initialized, in_library, in_library_at, last_modified_at, version) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, TRUE, $12, $13, $14) RETURNING id",
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
                .bind(added_secs)
                .bind(m.last_modified_at)
                .bind(m.version)
                .fetch_one(pool)
                .await?;
                summary.restored_manga += 1;
                id
            }
        };

        // chapters (upsert on (url, manga))
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
            match existing_ch {
                Some((cid, cur_name, cur_scan, cur_read, cur_book, cur_lpr, cur_upload, cur_number, cur_order)) => {
                    // source_order is 1-based here (Mihon/phone backups are
                    // 0-based): the reader indexes chapters by
                    // `len - sourceOrder` on a DESC-sorted list, so a 0-based
                    // chapter (or 0) can never be opened.
                    let new_order = ch.source_order + 1;
                    let dirty = cur_name != ch.name
                        || cur_scan.as_deref() != ch.scanlator.as_deref()
                        || cur_read != ch.read
                        || cur_book != ch.bookmark
                        || cur_lpr != ch.last_page_read
                        || cur_upload != ch.date_upload
                        || cur_number != ch.chapter_number
                        || cur_order != new_order;
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
                        .bind(new_order)
                        .bind(cid)
                        .execute(pool)
                        .await?;
                    }
                    chapter_ids.push(cid);
                }
                None => {
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
                    .bind(ch.source_order + 1)
                    .bind(manga_id)
                    .fetch_one(pool)
                    .await?;
                    summary.restored_chapters += 1;
                    chapter_ids.push(cid);
                }
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
            let _ = suwayomi_db::query("UPDATE chapter SET last_page_read = $1, last_read_at = $2 WHERE url = $3 AND manga = $4")
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

        let _ = chapter_ids;
    }

    Ok(summary)
}

/// 对应上游 `BackupMangaHandler.restoreMangaTrackerData`。
///
/// 只在「本地没有该追踪器的记录」时新增；已有记录时按上游的做法只并进
/// `remote_id` / `library_id` 与取大的 `last_chapter_read`，其余字段保留本机值。
/// 备份里本仓库不支持的追踪器（例如旧版备份里的 id）直接丢弃。
async fn restore_manga_tracker_data(pool: &Db, manga_id: i32, tracks: &[BackupTracking]) -> Result<(), BackupError> {
    if tracks.is_empty() {
        return Ok(());
    }
    let existing: Vec<TrackRecordRow> = suwayomi_db::query_as("SELECT * FROM track_record WHERE manga_id = $1")
        .bind(manga_id)
        .fetch_all(pool)
        .await?;
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

/// Builds the `Backup` protobuf message from the current database (no encoding).
pub async fn create_backup_proto(pool: &Db, flags: BackupFlags) -> Result<Backup, BackupError> {
    build_backup(pool, flags).await
}

async fn build_backup(pool: &Db, flags: BackupFlags) -> Result<Backup, BackupError> {
    let category_rows: Vec<CategoryRow> = suwayomi_db::query_as("SELECT * FROM category ORDER BY sort_order, id")
        .fetch_all(pool)
        .await?;
    let backup_categories: Vec<BackupCategory> = category_rows
        .iter()
        .map(|c| BackupCategory {
            name: c.name.clone(),
            order: c.sort_order,
            flags: c.include_in_update,
            version: c.version,
            uid: c.uid,
            last_modified_at: c.last_modified_at,
            meta: HashMap::new(),
        })
        .collect();

    let manga_rows: Vec<MangaRow> = if flags.include_manga {
        suwayomi_db::query_as("SELECT * FROM manga WHERE in_library = TRUE ORDER BY id").fetch_all(pool).await?
    } else {
        Vec::new()
    };
    let mut backup_mangas: Vec<BackupManga> = Vec::with_capacity(manga_rows.len());
    let mut source_ids: Vec<i64> = Vec::new();
    for m in &manga_rows {
        let chapters: Vec<ChapterRow> = if flags.include_chapters {
            suwayomi_db::query_as("SELECT * FROM chapter WHERE manga = $1 ORDER BY source_order").bind(m.id).fetch_all(pool).await?
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
                // File format is the Mihon one: source_order is 0-based in a
                // `.tachibk`. The server stores it 1-based internally (the
                // WebUI reader indexes `len - sourceOrder`), so export
                // subtracts 1 and restore adds it back — a phone backup and a
                // re-export of a restored library stay numerically identical,
                // and export→import round-trips without drifting.
                source_order: c.source_order.saturating_sub(1),
                last_modified_at: c.last_modified_at,
                version: c.version,
                memo: c.memo.clone().into_bytes(),
                meta: HashMap::new(),
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
            meta: HashMap::new(),
        });
        if !source_ids.contains(&m.source) {
            source_ids.push(m.source);
        }
    }

    let mut backup_sources: Vec<BackupSource> = Vec::with_capacity(source_ids.len());
    for sid in &source_ids {
        let name: Option<String> = suwayomi_db::query_scalar("SELECT name FROM source WHERE id = $1").bind(sid).fetch_optional(pool).await?;
        if let Some(name) = name {
            backup_sources.push(BackupSource { name, source_id: *sid, meta: HashMap::new() });
        }
    }

    let tracker_credentials: Vec<BackupTrackerCredential> = if flags.include_tracking {
        suwayomi_db::query_as::<(i32, String, String, String, bool, String, String)>(
            "SELECT tracker_id, username, password, token, token_expired, score_type, pkce_verifier \
             FROM tracker_credential ORDER BY tracker_id",
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|(tracker_id, username, password, token, token_expired, score_type, pkce_verifier)| BackupTrackerCredential {
            tracker_id,
            username,
            password,
            token,
            token_expired,
            score_type,
            pkce_verifier,
        })
        .collect()
    } else {
        Vec::new()
    };

    Ok(Backup {
        backup_manga: backup_mangas,
        backup_categories,
        backup_sources,
        meta: HashMap::new(),
        server_settings: None,
        tracker_credentials,
    })
}

/// `track_record` 行 → `BackupTracking`。
fn backup_tracking_of(r: &TrackRecordRow) -> BackupTracking {
    BackupTracking {
        sync_id: r.sync_id,
        // 上游强制给 0 而不是 null：1.x 的字段是非空 long。
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
pub async fn create_backup(pool: &Db, flags: BackupFlags) -> Result<Vec<u8>, BackupError> {
    let backup = create_backup_proto(pool, flags).await?;
    let bytes = backup.encode_to_vec();
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&bytes)?;
    let gz = encoder.finish()?;
    Ok(gz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    async fn seed() -> Db {
        let db = Db::sqlite_in_memory().await.expect("connect");
        db.migrate().await.expect("migrate");
        let pool = db.pool();
        suwayomi_db::query("INSERT INTO extension (name, pkg_name, version_name, version_code, lang, content_warning) VALUES ('E','p','1',1,'en',0)")
            .execute(pool)
            .await
            .expect("ext");
        suwayomi_db::query("INSERT INTO source (name, lang, extension) VALUES ('MangaDex','en',1)").execute(pool).await.expect("src");
        suwayomi_db::query(
            "INSERT INTO manga (url, title, author, genre, status, thumbnail_url, in_library, source, initialized) \
             VALUES ('/m/1','Backup Manga','Author','Action, Drama',1,'https://t.jpg',TRUE,1,TRUE)",
        )
        .execute(pool)
        .await
        .expect("manga");
        suwayomi_db::query(
            "INSERT INTO chapter (url, name, chapter_number, source_order, read, last_page_read, manga) \
             VALUES ('/m/1/c/1','Ch 1',1.0,0,TRUE,3,1)",
        )
        .execute(pool)
        .await
        .expect("chapter");
        suwayomi_db::query("INSERT INTO category (name, sort_order) VALUES ('Cat',1)").execute(pool).await.expect("category");
        suwayomi_db::query("INSERT INTO category_manga (category, manga) VALUES (1,1)").execute(pool).await.expect("cm");
        db
    }

    #[tokio::test]
    async fn backup_roundtrip_preserves_manga() {
        let db = seed().await;
        let gz = create_backup(db.pool(), BackupFlags::default()).await.expect("create backup");

        use std::io::Read;
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
        assert_eq!(manga.chapters.len(), 1);
        assert_eq!(manga.chapters[0].name, "Ch 1");
        assert!(manga.chapters[0].read);
        assert_eq!(manga.chapters[0].last_page_read, 3);
        assert_eq!(backup.backup_categories.len(), 1);
        assert_eq!(backup.backup_categories[0].name, "Cat");
        assert_eq!(backup.backup_sources.len(), 1);
        assert_eq!(backup.backup_sources[0].name, "MangaDex");
    }

    #[tokio::test]
    async fn backup_empty_library_is_valid() {
        let db = Db::sqlite_in_memory().await.expect("connect");
        db.migrate().await.expect("migrate");
        let gz = create_backup(db.pool(), BackupFlags::default()).await.expect("create backup");
        use std::io::Read;
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
        let gz = create_backup(db.pool(), BackupFlags::default()).await.expect("create backup");

        // restore into a fresh embedded database
        let fresh = Db::sqlite_in_memory().await.expect("connect fresh");
        fresh.migrate().await.expect("migrate fresh");
        let summary = restore_backup(fresh.pool(), &gz, BackupFlags::default()).await.expect("restore");

        assert_eq!(summary.restored_manga, 1);
        assert_eq!(summary.restored_chapters, 1);
        assert!(summary.missing_sources.is_empty(), "sources included in backup");

        // verify content
        let n: i64 = suwayomi_db::query_scalar("SELECT COUNT(*) FROM manga").fetch_one(fresh.pool()).await.expect("count manga");
        assert_eq!(n, 1);
        let title: String = suwayomi_db::query_scalar("SELECT title FROM manga WHERE id = 1").fetch_one(fresh.pool()).await.expect("title");
        assert_eq!(title, "Backup Manga");
        let in_lib: bool = suwayomi_db::query_scalar("SELECT in_library FROM manga WHERE id = 1").fetch_one(fresh.pool()).await.expect("in_library");
        assert!(in_lib, "favorite manga restored as in-library");
        let ch: i64 = suwayomi_db::query_scalar("SELECT COUNT(*) FROM chapter WHERE manga = 1").fetch_one(fresh.pool()).await.expect("count chapters");
        assert_eq!(ch, 1);
        let cm: i64 = suwayomi_db::query_scalar("SELECT COUNT(*) FROM category_manga WHERE manga = 1").fetch_one(fresh.pool()).await.expect("count cm");
        assert_eq!(cm, 1, "category membership restored");
        let cat: String = suwayomi_db::query_scalar("SELECT name FROM category WHERE id = 1").fetch_one(fresh.pool()).await.expect("category");
        assert_eq!(cat, "Cat");
        let src: String = suwayomi_db::query_scalar("SELECT name FROM source WHERE id = 1").fetch_one(fresh.pool()).await.expect("source");
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
        suwayomi_db::query("INSERT INTO tracker_credential (tracker_id, username, password, token) VALUES (2, 'user', 'tok', '{}')")
            .execute(pool)
            .await
            .expect("credential");

        let gz = create_backup(pool, BackupFlags::default()).await.expect("create backup");
        let backup = decode_gz_backup(&gz).expect("decode");
        assert_eq!(backup.backup_manga[0].tracking.len(), 2, "导出带上全部 track_record");
        assert_eq!(backup.backup_manga[0].tracking[0].media_id, 12345);
        assert_eq!(backup.backup_manga[0].tracking[0].last_chapter_read, 3.5);
        assert_eq!(backup.tracker_credentials.len(), 1);

        // 关掉 tracking 之后两份数据都不带走
        let no_tracking =
            create_backup(pool, BackupFlags { include_tracking: false, ..Default::default() }).await.expect("create backup");
        let backup = decode_gz_backup(&no_tracking).expect("decode");
        assert!(backup.backup_manga[0].tracking.is_empty());
        assert!(backup.tracker_credentials.is_empty());

        let fresh = Db::sqlite_in_memory().await.expect("connect fresh");
        fresh.migrate().await.expect("migrate fresh");
        restore_backup(fresh.pool(), &gz, BackupFlags::default()).await.expect("restore");

        let (remote_id, last_chapter_read, private): (i64, f64, bool) =
            suwayomi_db::query_as("SELECT remote_id, last_chapter_read, private FROM track_record WHERE manga_id = 1 AND sync_id = 2")
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
        let backup = Backup {
            backup_manga: vec![manga],
            ..Default::default()
        };
        let raw = backup.encode_to_vec();
        use std::io::Write;
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
        let src_name: Option<String> = suwayomi_db::query_scalar("SELECT name FROM source WHERE id = 999").fetch_one(db.pool()).await.expect("src");
        assert_eq!(src_name.as_deref(), Some("source-999"));
    }
}
