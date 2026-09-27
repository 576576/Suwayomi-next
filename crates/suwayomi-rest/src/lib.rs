//! REST API v1 — mirrors `suwayomi.manga.controller.*` +
//! `MangaAPI.kt` + `GlobalAPI.kt` on axum.

// 测试代码允许 panic：unwrap / expect / panic! 在断言里是常规写法，
// 逐个改成 `?` 传播只会让失败信息更难读。生产代码不受这条影响
// （`cfg_attr(test, ...)`）。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::todo,
        clippy::indexing_slicing
    )
)]

pub mod auth;
pub mod error;
pub mod routes;
pub mod state;

pub use state::AppState;
