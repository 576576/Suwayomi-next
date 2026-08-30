//! Chapter model — mirrors `manga/model/dataclass/ChapterDataClass.kt`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Mirrors `data class ChapterDataClass`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterDataClass {
    pub id: i32,
    pub url: String,
    pub name: String,
    pub upload_date: i64,
    pub chapter_number: f32,
    pub scanlator: Option<String>,
    pub manga_id: i32,
    pub read: bool,
    pub bookmarked: bool,
    /// last read page, zero means not read/no data
    pub last_page_read: i32,
    /// last read at (epoch secs), zero means not read/no data
    pub last_read_at: i64,
    /// chapter index, starts with 1
    pub index: i32,
    /// date we first saw this chapter
    pub fetched_at: i64,
    /// website url of this chapter
    pub real_url: Option<String>,
    /// is chapter downloaded
    pub downloaded: bool,
    /// used to construct pages in the front-end
    pub page_count: i32,
    pub last_modified_at: i64,
    pub version: i64,
    #[serde(skip)]
    pub memo: Value,
}
