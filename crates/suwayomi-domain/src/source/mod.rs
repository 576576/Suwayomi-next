//! Source abstraction for the domain layer.
//!
//! Mirrors `eu.kanade.tachiyomi.source.Source` + the `GetSource` helpers.
//! `SourceFetcher` is the seam where the JVM sandbox (Phase 5) plugs in.
//! `LocalSource` semantics (ID, file handling) are reserved here.

pub mod sandbox;

use async_trait::async_trait;
use suwayomi_core::source::{MangasPage, SChapter, SManga};

/// The Tachiyomi local source id (a negative long). Matches `LocalSource.ID`.
pub const LOCAL_SOURCE_ID: i64 = -1;

/// Mirrors `Source` / `HttpSource` capabilities needed by the domain layer.
#[async_trait]
pub trait SourceFetcher: Send + Sync {
    /// fetchMangaAndChapters: fetch details (and/or chapters) from the source
    async fn fetch_manga_update(
        &self,
        source_id: i64,
        manga: &SManga,
        chapters: &[SChapter],
        fetch_details: bool,
        fetch_chapters: bool,
    ) -> crate::error::Result<(SManga, Vec<SChapter>)>;

    /// Popular / latest pagination (used by MangaList & Search)
    async fn get_popular_manga(&self, source_id: i64, page: u32) -> crate::error::Result<MangasPage>;
    async fn get_latest_updates(&self, source_id: i64, page: u32) -> crate::error::Result<MangasPage>;
    async fn search_manga(&self, source_id: i64, query: &str, page: u32) -> crate::error::Result<MangasPage>;

    /// Page list for a chapter (used by the download manager). Defaults to
    /// "not implemented" until the JVM sandbox exposes `getChapterPages`.
    async fn fetch_pages(
        &self,
        _source_id: i64,
        _manga_url: &str,
        _chapter_url: &str,
    ) -> crate::error::Result<Vec<suwayomi_core::source::SourcePage>> {
        Err(crate::error::DomainError::Source("fetch_pages not implemented (Phase 6)".into()))
    }

    /// Whether the source provides a latest listing
    fn supports_latest(&self, source_id: i64) -> bool;

    /// Search filters for a source (Phase 5). Defaults to an empty list;
    /// the JVM sandbox backend forwards `/source/{id}/filters`.
    async fn get_filters(&self, _source_id: i64) -> crate::error::Result<serde_json::Value> {
        Ok(serde_json::json!([]))
    }
}

/// Default stub used before the JVM sandbox is wired (Phase 5).
/// Matches `StubSource` behavior: fetching fails with a descriptive error.
#[derive(Default)]
pub struct StubFetcher;

#[async_trait]
impl SourceFetcher for StubFetcher {
    async fn fetch_manga_update(
        &self,
        _source_id: i64,
        _manga: &SManga,
        _chapters: &[SChapter],
        _fetch_details: bool,
        _fetch_chapters: bool,
    ) -> crate::error::Result<(SManga, Vec<SChapter>)> {
        Err(crate::error::DomainError::Source("source unavailable: extension sandbox not connected (Phase 5)".into()))
    }

    async fn get_popular_manga(&self, _source_id: i64, _page: u32) -> crate::error::Result<MangasPage> {
        Err(crate::error::DomainError::Source("source unavailable: extension sandbox not connected (Phase 5)".into()))
    }

    async fn get_latest_updates(&self, _source_id: i64, _page: u32) -> crate::error::Result<MangasPage> {
        Err(crate::error::DomainError::Source("source unavailable: extension sandbox not connected (Phase 5)".into()))
    }

    async fn search_manga(&self, _source_id: i64, _query: &str, _page: u32) -> crate::error::Result<MangasPage> {
        Err(crate::error::DomainError::Source("source unavailable: extension sandbox not connected (Phase 5)".into()))
    }

    fn supports_latest(&self, _source_id: i64) -> bool {
        false
    }
}
