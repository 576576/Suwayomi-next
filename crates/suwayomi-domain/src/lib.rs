//! Business logic layer — mirrors `suwayomi.manga.impl.*`.
//! Phase 2 scope: database-backed services (Manga / Chapter / Page / Library /
//! MangaList / Category / CategoryManga / Meta). Source-fetching paths are
//! behind the `SourceFetcher` trait and get a real implementation in Phase 5.

pub mod category;
pub mod download;
pub mod chapter;
pub mod error;
pub mod manga;
pub mod meta;
pub mod page;
pub mod source;
pub mod sql;
