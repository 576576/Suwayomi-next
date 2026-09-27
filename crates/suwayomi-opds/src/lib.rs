//! suwayomi-opds
//!
//! Mirrors `suwayomi.opds.*` — OPDS 1.2 feeds for e-reader clients
//! (KOReader, etc.). Feed set: root navigation, search, history, library
//! series with cross-filters/sort, explore sources, categories/genres/
//! statuses/languages navigation, library updates, series chapters, chapter
//! metadata, not-found.

// 测试代码允许 panic：unwrap / expect / panic! 在断言里是常规写法，
// 逐个改成 `?` 传播只会让失败信息更难读。生产代码不受这条影响
// （`cfg_attr(test, ...)`）。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unreachable, clippy::todo))]

pub mod constants;
pub mod feeds;
pub mod model;
pub mod repository;
pub mod router;
pub mod xml;
