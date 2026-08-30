//! Row structs mirroring the 15 Suwayomi tables (PostgreSQL + SQLite shared).
//!
//! `memo` columns are stored as JSON text; use `sqlx::types::Json` for
//! transparent encode/decode. All timestamps are epoch-seconds (i64).

use sqlx::FromRow;

/// `extension` — mirrors `ExtensionTable`
#[derive(Debug, Clone, FromRow)]
pub struct ExtensionRow {
    pub id: i32,
    pub apk_name: Option<String>,
    pub store_index_url: Option<String>,
    pub icon_url: String,
    pub name: String,
    pub pkg_name: String,
    pub apk_url: Option<String>,
    pub jar_url: Option<String>,
    pub extension_lib: Option<String>,
    pub version_name: String,
    pub version_code: i64,
    pub lang: String,
    pub content_warning: i32,
    pub is_installed: bool,
    pub has_update: bool,
    pub is_obsolete: bool,
    pub class_name: String,
}

/// `source` — mirrors `SourceTable` (long id)
#[derive(Debug, Clone, FromRow)]
pub struct SourceRow {
    pub id: i64,
    pub name: String,
    pub lang: String,
    pub extension: i32,
    pub content_warning: i32,
}

/// `manga` — mirrors `MangaTable`
#[derive(Debug, Clone, FromRow)]
pub struct MangaRow {
    pub id: i32,
    pub url: String,
    pub title: String,
    pub initialized: bool,
    pub artist: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub genre: Option<String>,
    pub status: i32,
    pub thumbnail_url: Option<String>,
    pub thumbnail_url_last_fetched: i64,
    pub in_library: bool,
    pub in_library_at: i64,
    pub source: i64,
    pub real_url: Option<String>,
    pub last_fetched_at: i64,
    pub chapters_last_fetched_at: i64,
    pub update_strategy: String,
    pub last_modified_at: i64,
    pub version: i64,
    pub is_syncing: bool,
    pub memo: String,
}

/// `chapter` — mirrors `ChapterTable`
#[derive(Debug, Clone, FromRow)]
pub struct ChapterRow {
    pub id: i32,
    pub url: String,
    pub name: String,
    pub date_upload: i64,
    pub chapter_number: f32,
    pub scanlator: Option<String>,
    pub read: bool,
    pub bookmark: bool,
    pub last_page_read: i32,
    pub last_read_at: i64,
    pub fetched_at: i64,
    pub source_order: i32,
    pub real_url: Option<String>,
    pub is_downloaded: bool,
    pub page_count: i32,
    pub manga: i32,
    pub koreader_hash: Option<String>,
    pub last_modified_at: i64,
    pub version: i64,
    pub is_syncing: bool,
    pub memo: String,
}

/// `page` — mirrors `PageTable`
#[derive(Debug, Clone, FromRow)]
pub struct PageRow {
    pub id: i32,
    pub index: i32,
    pub url: String,
    pub image_url: Option<String>,
    pub chapter: i32,
}

/// `category` — mirrors `CategoryTable`
#[derive(Debug, Clone, FromRow)]
pub struct CategoryRow {
    pub id: i32,
    pub name: String,
    pub sort_order: i32,
    pub is_default: bool,
    pub include_in_update: i32,
    pub include_in_download: i32,
    pub version: i64,
    pub uid: i64,
    pub last_modified_at: i64,
    pub is_syncing: bool,
}

/// `category_manga` — mirrors `CategoryMangaTable`
#[derive(Debug, Clone, FromRow)]
pub struct CategoryMangaRow {
    pub id: i32,
    pub category: i32,
    pub manga: i32,
}

/// `category_meta` — mirrors `CategoryMetaTable`
#[derive(Debug, Clone, FromRow)]
pub struct CategoryMetaRow {
    pub id: i32,
    pub meta_key: String,
    pub value: String,
    pub category_ref: i32,
}

/// `chapter_meta` — mirrors `ChapterMetaTable`
#[derive(Debug, Clone, FromRow)]
pub struct ChapterMetaRow {
    pub id: i32,
    pub meta_key: String,
    pub value: String,
    pub chapter_ref: i32,
}

/// `manga_meta` — mirrors `MangaMetaTable`
#[derive(Debug, Clone, FromRow)]
pub struct MangaMetaRow {
    pub id: i32,
    pub meta_key: String,
    pub value: String,
    pub manga_ref: i32,
}

/// `source_meta` — mirrors `SourceMetaTable` (long ref, no FK)
#[derive(Debug, Clone, FromRow)]
pub struct SourceMetaRow {
    pub id: i32,
    pub meta_key: String,
    pub value: String,
    pub source_ref: i64,
}

/// `global_meta` — mirrors `GlobalMetaTable`
#[derive(Debug, Clone, FromRow)]
pub struct GlobalMetaRow {
    pub id: i32,
    pub meta_key: String,
    pub value: String,
}

/// `extension_store` — mirrors `ExtensionStoreTable`
#[derive(Debug, Clone, FromRow)]
pub struct ExtensionStoreRow {
    pub id: i32,
    pub index_url: String,
    pub name: String,
    pub badge_label: String,
    pub signing_key: String,
    pub contact_website: String,
    pub contact_discord: Option<String>,
    pub is_legacy: bool,
    pub extension_list_url: Option<String>,
}

/// `track_record` — mirrors `TrackRecordTable`
#[derive(Debug, Clone, FromRow)]
pub struct TrackRecordRow {
    pub id: i32,
    pub manga_id: i32,
    pub sync_id: i32,
    pub remote_id: i64,
    pub library_id: Option<i64>,
    pub title: String,
    pub last_chapter_read: f64,
    pub total_chapters: i32,
    pub status: i32,
    pub score: f64,
    pub remote_url: String,
    pub start_date: i64,
    pub finish_date: i64,
    pub private: bool,
}

/// `track_search` — mirrors `TrackSearchTable`
#[derive(Debug, Clone, FromRow)]
pub struct TrackSearchRow {
    pub id: i32,
    pub tracker_id: i32,
    pub remote_id: i64,
    pub title: String,
    pub total_chapters: i32,
    pub tracking_url: String,
    pub cover_url: String,
    pub summary: String,
    pub publishing_status: String,
    pub publishing_type: String,
    pub start_date: String,
    pub library_id: Option<i64>,
    pub last_chapter_read: f64,
    pub status: i32,
    pub score: f64,
    pub started_reading_date: i64,
    pub finished_reading_date: i64,
    pub private: bool,
    pub authors: Option<String>,
    pub artists: Option<String>,
}
