//! Manga model + related enums.
//!
//! Mirrors `manga/model/dataclass/MangaDataClass.kt`,
//! `manga/model/table/MangaTable.kt` (MangaStatus) and
//! `eu/kanade/tachiyomi/source/model/UpdateStrategy.kt`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::chapter::ChapterDataClass;
use super::source::SourceDataClass;
use super::track::MangaTrackerDataClass;

/// Current epoch seconds, equivalent to Kotlin `Instant.now().epochSecond`.
pub fn now_epoch_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Mirrors `eu.kanade.tachiyomi.source.model.UpdateStrategy`.
/// Stored in DB as its name; serialized as its name (SCREAMING_SNAKE_CASE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UpdateStrategy {
    #[default]
    AlwaysUpdate,
    OnlyFetchOnce,
}

impl UpdateStrategy {
    pub fn from_db(s: &str) -> Self {
        match s {
            "ONLY_FETCH_ONCE" => Self::OnlyFetchOnce,
            _ => Self::AlwaysUpdate,
        }
    }

    pub fn to_db(&self) -> &'static str {
        match self {
            Self::AlwaysUpdate => "ALWAYS_UPDATE",
            Self::OnlyFetchOnce => "ONLY_FETCH_ONCE",
        }
    }
}

/// Mirrors `MangaStatus` in `manga/model/table/MangaTable.kt`.
/// DB stores the int value; JSON/GraphQL expose the enum name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MangaStatus {
    Unknown,
    Ongoing,
    Completed,
    Licensed,
    PublishingFinished,
    Cancelled,
    OnHiatus,
}

impl MangaStatus {
    pub const UNKNOWN: i32 = 0;
    pub const ONGOING: i32 = 1;
    pub const COMPLETED: i32 = 2;
    pub const LICENSED: i32 = 3;
    pub const PUBLISHING_FINISHED: i32 = 4;
    pub const CANCELLED: i32 = 5;
    pub const ON_HIATUS: i32 = 6;

    pub fn from_i32(value: i32) -> Self {
        match value {
            Self::ONGOING => Self::Ongoing,
            Self::COMPLETED => Self::Completed,
            Self::LICENSED => Self::Licensed,
            Self::PUBLISHING_FINISHED => Self::PublishingFinished,
            Self::CANCELLED => Self::Cancelled,
            Self::ON_HIATUS => Self::OnHiatus,
            _ => Self::Unknown,
        }
    }

    pub fn to_i32(&self) -> i32 {
        match self {
            Self::Unknown => Self::UNKNOWN,
            Self::Ongoing => Self::ONGOING,
            Self::Completed => Self::COMPLETED,
            Self::Licensed => Self::LICENSED,
            Self::PublishingFinished => Self::PUBLISHING_FINISHED,
            Self::Cancelled => Self::CANCELLED,
            Self::OnHiatus => Self::ON_HIATUS,
        }
    }
}

/// Mirrors `data class MangaDataClass`.
/// JSON field names match Kotlin jackson output (camelCase).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MangaDataClass {
    pub id: i32,
    pub source_id: String,
    pub url: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub thumbnail_url_last_fetched: i64,
    pub initialized: bool,
    pub artist: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub genre: Vec<String>,
    pub status: MangaStatus,
    pub in_library: bool,
    pub in_library_at: i64,
    pub source: Option<SourceDataClass>,
    pub real_url: Option<String>,
    pub last_fetched_at: Option<i64>,
    pub chapters_last_fetched_at: Option<i64>,
    pub update_strategy: UpdateStrategy,
    pub fresh_data: bool,
    pub unread_count: Option<i64>,
    pub download_count: Option<i64>,
    pub chapter_count: Option<i64>,
    pub last_read_at: Option<i64>,
    pub last_chapter_read: Option<ChapterDataClass>,
    pub age: Option<i64>,
    pub chapters_age: Option<i64>,
    pub trackers: Option<Vec<MangaTrackerDataClass>>,
    pub last_modified_at: i64,
    pub version: i64,
    #[serde(skip)]
    pub memo: Value,
}

impl MangaDataClass {
    /// Computes the `age`/`chaptersAge` defaults the same way Kotlin does:
    /// `age = if (lastFetchedAt == null) 0 else now - lastFetchedAt`
    /// `chaptersAge = if (chaptersLastFetchedAt == null) null else now - chaptersLastFetchedAt`
    pub fn with_computed_ages(mut self) -> Self {
        self.age = Some(match self.last_fetched_at {
            None => 0,
            Some(last) => now_epoch_secs() - last,
        });
        self.chapters_age = self.chapters_last_fetched_at.map(|last| now_epoch_secs() - last);
        self
    }
}

/// Mirrors `data class PagedMangaListDataClass`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedMangaListDataClass {
    pub manga_list: Vec<MangaDataClass>,
    pub has_next_page: bool,
}

/// Mirrors `data class MangaChapterDataClass` (used by recent-chapters feeds).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MangaChapterDataClass {
    pub manga: MangaDataClass,
    pub chapter: ChapterDataClass,
}

/// Mirrors `internal fun String?.toGenreList()`.
pub fn to_genre_list(genre: Option<&str>) -> Vec<String> {
    genre.map(|g| g.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
}
