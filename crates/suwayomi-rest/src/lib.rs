//! REST API v1 — mirrors `suwayomi.manga.controller.*` +
//! `MangaAPI.kt` + `GlobalAPI.kt` on axum.

pub mod auth;
pub mod error;
pub mod routes;
pub mod state;

pub use state::AppState;
