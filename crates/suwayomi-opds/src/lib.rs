//! suwayomi-opds
//!
//! Mirrors `suwayomi.opds.*` — OPDS 1.2 feeds for e-reader clients
//! (KOReader, etc.). Feed set: root navigation, search, history, library
//! series with cross-filters/sort, explore sources, categories/genres/
//! statuses/languages navigation, library updates, series chapters, chapter
//! metadata, not-found.

pub mod constants;
pub mod feeds;
pub mod model;
pub mod repository;
pub mod router;
pub mod xml;
