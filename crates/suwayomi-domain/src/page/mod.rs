//! Page service — mirrors `suwayomi.manga.impl.Page`
//! (DB-backed parts; image streaming lands with the source layer, Phase 5/6).

use suwayomi_core::db::Db;
use suwayomi_core::models::PageDataClass;
use suwayomi_core::schema::PageRow;

use crate::error::{DomainError, Result};
use crate::sql::bind_placeholders;

#[derive(Clone)]
pub struct PageService {
    pub db: Db,
}

impl PageService {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Page list for a chapter (by manga + source_order).
    pub async fn get_page_list_by_index(&self, manga_id: i32, chapter_index: i32) -> Result<Vec<PageDataClass>> {
        let sql = bind_placeholders(
            "SELECT p.* FROM page p INNER JOIN chapter c ON c.id = p.chapter WHERE c.manga = ? AND c.source_order = ? ORDER BY p.index ASC");
        let rows =
            { sqlx::query_as::<_, PageRow>(&sql).bind(manga_id).bind(chapter_index).fetch_all(self.db.pool()).await? };
        Ok(rows
            .iter()
            .map(|r| PageDataClass { index: r.index, image_url: r.image_url.clone().unwrap_or_default() })
            .collect())
    }

    /// Page list for a chapter by chapter id.
    pub async fn get_page_list(&self, chapter_id: i32) -> Result<Vec<PageDataClass>> {
        let sql = bind_placeholders("SELECT * FROM page WHERE chapter = ? ORDER BY index ASC");
        let rows = sqlx::query_as::<_, PageRow>(&sql).bind(chapter_id).fetch_all(self.db.pool()).await?;
        Ok(rows
            .iter()
            .map(|r| PageDataClass { index: r.index, image_url: r.image_url.clone().unwrap_or_default() })
            .collect())
    }

    /// Mirrors the page-count heuristic used by the front-end ("pageCount").
    pub async fn get_page_count(&self, chapter_id: i32) -> Result<i32> {
        let sql = bind_placeholders("SELECT count(*) FROM page WHERE chapter = ?");
        let count = sqlx::query_scalar::<_, i64>(&sql).bind(chapter_id).fetch_one(self.db.pool()).await?;
        Ok(count as i32)
    }

    /// Look up a single page row (used by image endpoints).
    pub async fn get_page(&self, chapter_id: i32, index: i32) -> Result<PageRow> {
        let sql = bind_placeholders("SELECT * FROM page WHERE chapter = ? AND index = ?");
        sqlx::query_as::<_, PageRow>(&sql)
            .bind(chapter_id)
            .bind(index)
            .fetch_optional(self.db.pool())
            .await?
            .ok_or_else(|| DomainError::not_found("page not found"))
    }
}
