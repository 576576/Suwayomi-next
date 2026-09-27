//! REST API v1 — mirrors `suwayomi.manga.controller.*` +
//! `MangaAPI.kt` + `GlobalAPI.kt` on axum.
//!
//! 本 crate 只是**协议适配层**：把 `/api/v1/**` 的请求翻成 `suwayomi-domain`
//! 的服务调用。应用状态（`AppState`）与全站认证中间件已上移到 `suwayomi-api`
//! ——它们不只服务 REST，留在这里会让 OPDS 为了取一个类型而依赖 REST。

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

pub mod error;
pub mod routes;
