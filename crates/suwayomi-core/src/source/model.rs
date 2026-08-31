//! Extension-facing data abstractions — mirrors
//! `eu/kanade/tachiyomi/source/model/*.kt` (SManga, SChapter, Page, MangasPage).
//!
//! These structs are the wire format shared with the JVM sandbox (Phase 5)
//! and the local source implementation. Field names are snake_case to match
//! the Kotlin properties exactly.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::manga::UpdateStrategy;

/// Mirrors `interface SManga`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SManga {
    pub url: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub artist: Option<String>,
    pub author: Option<String>,
    pub status: i32,
    pub description: Option<String>,
    pub genre: Option<String>,
    /// Alternative titles (other-language titles from archive metadata) —
    /// shown on the manga details page as "Alternative title: …".
    #[serde(default)]
    pub alt_titles: Vec<String>,
    pub update_strategy: UpdateStrategy,
    pub initialized: bool,
    /// Extra source-specific metadata (JSON), namespaced (`mihon.*`, …)
    #[serde(default)]
    pub memo: Value,
}

impl SManga {
    pub const UNKNOWN: i32 = 0;
    pub const ONGOING: i32 = 1;
    pub const COMPLETED: i32 = 2;
    pub const LICENSED: i32 = 3;
    pub const PUBLISHING_FINISHED: i32 = 4;
    pub const CANCELLED: i32 = 5;
    pub const ON_HIATUS: i32 = 6;

    /// Mirrors `getGenres()`: split on `", "`, trim, drop blanks, dedupe.
    pub fn genres(&self) -> Option<Vec<String>> {
        let genre = self.genre.as_deref()?;
        if genre.trim().is_empty() {
            return None;
        }
        let mut out: Vec<String> = Vec::new();
        for g in genre.split(", ") {
            let g = g.trim();
            if g.is_empty() || out.iter().any(|x| x == g) {
                continue;
            }
            out.push(g.to_string());
        }
        Some(out)
    }
}

/// Mirrors `interface SChapter`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SChapter {
    pub url: String,
    pub name: String,
    pub chapter_number: f32,
    pub scanlator: Option<String>,
    pub date_upload: i64,
    /// Extra source-specific metadata (JSON), namespaced (`mihon.*`, …)
    #[serde(default)]
    pub memo: Value,
}

/// Mirrors `open class Page(index, url, imageUrl, uri)` + progress handling.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePage {
    pub index: i32,
    pub url: String,
    pub image_url: Option<String>,
    /// android.net.Uri — kept as an opaque string for the JVM sandbox
    #[serde(skip)]
    pub uri: Option<String>,
}

impl SourcePage {
    pub fn new(index: i32, url: String, image_url: Option<String>) -> Self {
        Self { index, url, image_url, uri: None }
    }
}

/// Mirrors `class MangasPage(mangas, hasNextPage)`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MangasPage {
    pub mangas: Vec<SManga>,
    pub has_next_page: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smanga_genres_matches_kotlin() {
        let mut m = SManga { genre: Some("Action, Adventure,  Comedy , Action".into()), ..Default::default() };
        assert_eq!(m.genres(), Some(vec!["Action".to_string(), "Adventure".to_string(), "Comedy".to_string()]));
        m.genre = Some("   ".into());
        assert_eq!(m.genres(), None);
        m.genre = None;
        assert_eq!(m.genres(), None);
    }

    #[test]
    fn wire_format_is_snake_case() {
        let ch = SChapter { date_upload: 5, ..Default::default() };
        let v = serde_json::to_value(&ch).unwrap();
        assert!(v.as_object().unwrap().contains_key("date_upload"));
        assert!(v.as_object().unwrap().contains_key("chapter_number"));
    }
}
