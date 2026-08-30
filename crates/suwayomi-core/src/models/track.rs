//! Tracker models — mirror `manga/model/dataclass/TrackRecordDataClass.kt`,
//! `TrackSearchDataClass.kt` and `MangaTrackerDataClass.kt`.

use serde::{Deserialize, Serialize};

/// Mirrors `data class TrackRecordDataClass`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRecordDataClass {
    pub id: i32,
    pub manga_id: i32,
    pub tracker_id: i32,
    pub remote_id: i64,
    pub library_id: Option<i64>,
    pub title: String,
    pub last_chapter_read: f64,
    pub total_chapters: i32,
    pub status: i32,
    pub score: f64,
    pub score_string: Option<String>,
    pub remote_url: String,
    pub start_date: i64,
    pub finish_date: i64,
    pub private: bool,
}

/// Mirrors `@Serializable data class TrackSearchDataClass`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackSearchDataClass {
    pub id: i32,
    pub tracker_id: i32,
    pub remote_id: i64,
    pub library_id: Option<i64>,
    pub title: String,
    pub last_chapter_read: f64,
    pub total_chapters: i32,
    pub tracking_url: String,
    pub cover_url: String,
    pub summary: String,
    pub publishing_status: String,
    pub publishing_type: String,
    pub start_date: String,
    pub status: i32,
    pub score: f64,
    pub score_string: Option<String>,
    pub started_reading_date: i64,
    pub finished_reading_date: i64,
    pub private: bool,
}

/// Mirrors `data class MangaTrackerDataClass`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MangaTrackerDataClass {
    pub id: i32,
    pub name: String,
    pub icon: String,
    pub status_list: Vec<i32>,
    pub status_text_map: std::collections::BTreeMap<i32, String>,
    pub score_list: Vec<String>,
    pub record: Option<TrackRecordDataClass>,
}
