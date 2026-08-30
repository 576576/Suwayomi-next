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

/// Serializes the current database into a gzipped `Backup` protobuf payload.
pub async fn create_backup(pool: &PgPool) -> Result<Vec<u8>, BackupError> {
    let backup = build_backup(pool).await?;
    let bytes = backup.encode_to_vec();

    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&bytes)?;
    let gz = encoder.finish()?;
    Ok(gz)
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
        let category_ids: Vec<i32> =
            sqlx::query_scalar("SELECT category FROM category_manga WHERE manga = $1").bind(m.id).fetch_all(pool).await?;
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
            date_added: m.in_library_at,
            viewer: 0,
            chapters: backup_chapters,
            categories: category_ids,
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
}
