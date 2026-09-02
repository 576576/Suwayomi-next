//! Mihon/Suwayomi protobuf backup (`backup.proto`) — mirrors
//! `manga/impl/backup/proto/models/*.kt` + `ProtoBackupExport.kt`.
//!
//! The message structs are hand-written `prost` derives (no `protoc` needed);
//! field numbers match the kotlinx-protobuf `@ProtoNumber` annotations.
//! `create_backup` builds the `Backup` message from the current database and
//! returns the gzip-compressed protobuf bytes (the wire format the Kotlin
//! `protobufExport` endpoint streams, and the payload inside a `.tachibk`).

use std::collections::HashMap;

use prost::Message;
use sqlx::PgPool;

use crate::schema::{CategoryRow, ChapterRow, MangaRow};

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
pub async fn restore_backup(pool: &PgPool, gz: &[u8]) -> Result<RestoreSummary, BackupError> {
    let backup = decode_gz_backup(gz)?;
    restore_backup_proto(pool, &backup).await
}

/// Restores from an already-decoded `Backup` message (idempotent upserts).
pub async fn restore_backup_proto(pool: &PgPool, backup: &Backup) -> Result<RestoreSummary, BackupError> {
    let mut summary = validate_backup_inner(backup);
    let source_names: HashMap<i64, String> = backup.backup_sources.iter().map(|s| (s.source_id, s.name.clone())).collect();

    // 1) categories: order -> id (reuse existing by name; mirrors Kotlin's
    //    `BackupCategory.order`-keyed mapping used by BackupManga.categories)
    let mut category_mapping: HashMap<i32, i32> = HashMap::new(); // category order -> db id
    for (idx, c) in backup.backup_categories.iter().enumerate() {
        let existing: Option<i32> = sqlx::query_scalar("SELECT id FROM category WHERE name = $1").bind(&c.name).fetch_optional(pool).await?;
        let id = match existing {
            Some(id) => id,
            None => {
                let id: i32 = sqlx::query_scalar("INSERT INTO category (name, sort_order) VALUES ($1, $2) RETURNING id")
                    .bind(&c.name)
                    .bind(c.order)
                    .fetch_one(pool)
                    .await?;
                summary.restored_categories += 1;
                id
            }
        };
        let _ = idx;
        category_mapping.insert(c.order, id);
    }

    // 2) ensure an extension row exists (source.extension FK — a violation
    //    would terminate the embedded session)
    let ext_id: i32 = match sqlx::query_scalar::<_, i32>("SELECT id FROM extension ORDER BY id LIMIT 1").fetch_optional(pool).await? {
        Some(id) => id,
        None => sqlx::query_scalar(
            "INSERT INTO extension (name, pkg_name, version_name, version_code, lang, content_warning) \
             VALUES ('restored', 'org.suwayomi.restored', '0.0.0', 0, 'en', 0) RETURNING id",
        )
        .fetch_one(pool)
        .await?,
    };

    // 3) restore each manga
    let now_secs = chrono::Utc::now().timestamp();
    for m in &backup.backup_manga {
        // ensure source exists
        let source_exists: bool = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM source WHERE id = $1)")
            .bind(m.source)
            .fetch_one(pool)
            .await?;
        if !source_exists {
            let name = source_names.get(&m.source).cloned().unwrap_or_else(|| format!("source-{}", m.source));
            sqlx::query("INSERT INTO source (id, name, lang, extension) VALUES ($1, $2, 'en', $3)")
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

        // find-or-insert manga by (url, source)
        let existing: Option<i32> = sqlx::query_scalar("SELECT id FROM manga WHERE url = $1 AND source = $2")
            .bind(&m.url)
            .bind(m.source)
            .fetch_optional(pool)
            .await?;
        let manga_id = match existing {
            Some(id) => {
                sqlx::query(
                    "UPDATE manga SET artist = COALESCE($1, artist), author = COALESCE($2, author), \
                     description = COALESCE($3, description), genre = COALESCE(NULLIF($4, ''), genre), \
                     status = $5, thumbnail_url = COALESCE($6, thumbnail_url), update_strategy = $7, \
                     in_library = TRUE, in_library_at = $8, last_modified_at = $9, \
                     version = $10, initialized = initialized OR $11 WHERE id = $12",
                )
                .bind(&m.artist)
                .bind(&m.author)
                .bind(&m.description)
                .bind(m.genre.join(", "))
                .bind(m.status)
                .bind(&m.thumbnail_url)
                .bind(update_strategy_name(m.update_strategy))
                .bind(added_secs)
                .bind(m.last_modified_at)
                .bind(m.version)
                .bind(m.description.is_some())
                .bind(id)
                .execute(pool)
                .await?;
                id
            }
            None => {
                let id: i32 = sqlx::query_scalar(
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
        for ch in &m.chapters {
            let existing_ch: Option<i32> = sqlx::query_scalar("SELECT id FROM chapter WHERE url = $1 AND manga = $2")
                .bind(&ch.url)
                .bind(manga_id)
                .fetch_optional(pool)
                .await?;
            match existing_ch {
                Some(cid) => {
                    sqlx::query(
                        "UPDATE chapter SET name = $1, scanlator = $2, read = $3, bookmark = $4, last_page_read = $5, \
                         date_upload = $6, chapter_number = $7, source_order = $8, last_modified_at = $9, version = $10 WHERE id = $11",
                    )
                    .bind(&ch.name)
                    .bind(&ch.scanlator)
                    .bind(ch.read)
                    .bind(ch.bookmark)
                    .bind(ch.last_page_read)
                    .bind(ch.date_upload)
                    .bind(ch.chapter_number)
                    .bind(ch.source_order)
                    .bind(ch.last_modified_at)
                    .bind(ch.version)
                    .bind(cid)
                    .execute(pool)
                    .await?;
                    chapter_ids.push(cid);
                }
                None => {
                    let cid: i32 = sqlx::query_scalar(
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
                    chapter_ids.push(cid);
                }
            }
        }

        // category membership (backup index -> db id via mapping)
        for cidx in &m.categories {
            if let Some(db_cat) = category_mapping.get(cidx) {
                let _ = sqlx::query("INSERT INTO category_manga (category, manga) VALUES ($1, $2) ON CONFLICT (manga, category) DO NOTHING")
                    .bind(db_cat)
                    .bind(manga_id)
                    .execute(pool)
                    .await;
            }
        }

        // history: match chapter by url, apply last_page_read / last_read_at
        for h in &m.history {
            let _ = sqlx::query("UPDATE chapter SET last_page_read = $1, last_read_at = $2 WHERE url = $3 AND manga = $4")
                .bind(h.last_read as i32)
                .bind(h.read_at / 1000)
                .bind(&h.url)
                .bind(manga_id)
                .execute(pool)
                .await;
        }

        let _ = chapter_ids;
    }

    Ok(summary)
}

/// Maps the 0.x update-strategy ordinal back to the DB enum name.
fn update_strategy_name(ordinal: i32) -> &'static str {
    match ordinal {
        1 => "ALWAYS_FETCH",
        _ => "ALWAYS_UPDATE",
    }
}

/// Builds the `Backup` protobuf message from the current database (no encoding).
pub async fn create_backup_proto(pool: &PgPool) -> Result<Backup, BackupError> {
    build_backup(pool).await
}

async fn build_backup(pool: &PgPool) -> Result<Backup, BackupError> {
    let category_rows: Vec<CategoryRow> = sqlx::query_as("SELECT * FROM category ORDER BY sort_order, id")
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

    let manga_rows: Vec<MangaRow> = sqlx::query_as("SELECT * FROM manga WHERE in_library = TRUE ORDER BY id")
        .fetch_all(pool)
        .await?;
    let mut backup_mangas: Vec<BackupManga> = Vec::with_capacity(manga_rows.len());
    let mut source_ids: Vec<i64> = Vec::new();
    for m in &manga_rows {
        let chapters: Vec<ChapterRow> =
            sqlx::query_as("SELECT * FROM chapter WHERE manga = $1 ORDER BY source_order").bind(m.id).fetch_all(pool).await?;
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
                source_order: c.source_order,
                last_modified_at: c.last_modified_at,
                version: c.version,
                memo: c.memo.clone().into_bytes(),
                meta: HashMap::new(),
            })
            .collect();
        // BackupManga.categories stores the category ORDER (not id) —
        // mirrors Kotlin: `categoryMapping[it]` keys on `BackupCategory.order`.
        let category_orders: Vec<i32> = sqlx::query_scalar(
            "SELECT c.sort_order FROM category_manga cm JOIN category c ON c.id = cm.category WHERE cm.manga = $1",
        )
        .bind(m.id)
        .fetch_all(pool)
        .await?;
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
            tracking: vec![],
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
        let name: Option<String> = sqlx::query_scalar("SELECT name FROM source WHERE id = $1").bind(sid).fetch_optional(pool).await?;
        if let Some(name) = name {
            backup_sources.push(BackupSource { name, source_id: *sid, meta: HashMap::new() });
        }
    }

    Ok(Backup {
        backup_manga: backup_mangas,
        backup_categories,
        backup_sources,
        meta: HashMap::new(),
        server_settings: None,
    })
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
    Sqlx(#[from] sqlx::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("protobuf decode error: {0}")]
    Decode(String),
}

/// Serializes the current database into a gzipped `Backup` protobuf payload.
pub async fn create_backup(pool: &PgPool) -> Result<Vec<u8>, BackupError> {
    let backup = create_backup_proto(pool).await?;
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
        let db = Db::connect_embedded(None).await.expect("connect");
        db.migrate().await.expect("migrate");
        let pool = db.pool();
        sqlx::query("INSERT INTO extension (name, pkg_name, version_name, version_code, lang, content_warning) VALUES ('E','p','1',1,'en',0)")
            .execute(pool)
            .await
            .expect("ext");
        sqlx::query("INSERT INTO source (name, lang, extension) VALUES ('MangaDex','en',1)").execute(pool).await.expect("src");
        sqlx::query(
            "INSERT INTO manga (url, title, author, genre, status, thumbnail_url, in_library, source, initialized) \
             VALUES ('/m/1','Backup Manga','Author','Action, Drama',1,'https://t.jpg',TRUE,1,TRUE)",
        )
        .execute(pool)
        .await
        .expect("manga");
        sqlx::query(
            "INSERT INTO chapter (url, name, chapter_number, source_order, read, last_page_read, manga) \
             VALUES ('/m/1/c/1','Ch 1',1.0,0,TRUE,3,1)",
        )
        .execute(pool)
        .await
        .expect("chapter");
        sqlx::query("INSERT INTO category (name, sort_order) VALUES ('Cat',1)").execute(pool).await.expect("category");
        sqlx::query("INSERT INTO category_manga (category, manga) VALUES (1,1)").execute(pool).await.expect("cm");
        db
    }

    #[tokio::test]
    async fn backup_roundtrip_preserves_manga() {
        let db = seed().await;
        let gz = create_backup(db.pool()).await.expect("create backup");

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
        let db = Db::connect_embedded(None).await.expect("connect");
        db.migrate().await.expect("migrate");
        let gz = create_backup(db.pool()).await.expect("create backup");
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
        let gz = create_backup(db.pool()).await.expect("create backup");

        // restore into a fresh embedded database
        let fresh = Db::connect_embedded(None).await.expect("connect fresh");
        fresh.migrate().await.expect("migrate fresh");
        let summary = restore_backup(fresh.pool(), &gz).await.expect("restore");

        assert_eq!(summary.restored_manga, 1);
        assert_eq!(summary.restored_chapters, 1);
        assert!(summary.missing_sources.is_empty(), "sources included in backup");

        // verify content
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga").fetch_one(fresh.pool()).await.expect("count manga");
        assert_eq!(n, 1);
        let title: String = sqlx::query_scalar("SELECT title FROM manga WHERE id = 1").fetch_one(fresh.pool()).await.expect("title");
        assert_eq!(title, "Backup Manga");
        let in_lib: bool = sqlx::query_scalar("SELECT in_library FROM manga WHERE id = 1").fetch_one(fresh.pool()).await.expect("in_library");
        assert!(in_lib, "favorite manga restored as in-library");
        let ch: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chapter WHERE manga = 1").fetch_one(fresh.pool()).await.expect("count chapters");
        assert_eq!(ch, 1);
        let cm: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM category_manga WHERE manga = 1").fetch_one(fresh.pool()).await.expect("count cm");
        assert_eq!(cm, 1, "category membership restored");
        let cat: String = sqlx::query_scalar("SELECT name FROM category WHERE id = 1").fetch_one(fresh.pool()).await.expect("category");
        assert_eq!(cat, "Cat");
        let src: String = sqlx::query_scalar("SELECT name FROM source WHERE id = 1").fetch_one(fresh.pool()).await.expect("source");
        assert_eq!(src, "MangaDex");
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
        let db = Db::connect_embedded(None).await.expect("connect");
        db.migrate().await.expect("migrate");
        let s = restore_backup(db.pool(), &gz).await.expect("restore");
        assert_eq!(s.restored_manga, 1);
        let src_name: Option<String> = sqlx::query_scalar("SELECT name FROM source WHERE id = 999").fetch_one(db.pool()).await.expect("src");
        assert_eq!(src_name.as_deref(), Some("source-999"));
    }
}
