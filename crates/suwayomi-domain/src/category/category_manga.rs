//! Category↔Manga relations — mirrors `suwayomi.tachidesk.manga.impl.CategoryManga`.

use sqlx::Row;
use suwayomi_core::db::Db;
use suwayomi_core::models::{CategoryDataClass, IncludeOrExclude, MangaDataClass};
use suwayomi_core::schema::{CategoryRow, MangaRow};

use crate::category::CategoryService;
use crate::error::Result;
use crate::manga::manga_row_to_data_class;
use crate::sql::bind_placeholders;

#[derive(Clone)]
pub struct CategoryMangaService {
    pub db: Db,
}

impl CategoryMangaService {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Mirrors `addMangasToCategories` — skips DEFAULT_CATEGORY_ID, insert-if-absent.
    pub async fn add_mangas_to_categories(&self, manga_ids: &[i32], category_ids: &[i32]) -> Result<()> {
        let filtered: Vec<i32> =
            category_ids.iter().copied().filter(|&c| c != CategoryService::DEFAULT_CATEGORY_ID).collect();
        for &manga_id in manga_ids {
            for &category_id in &filtered {
                let exists = self.exists(manga_id, category_id).await?;
                if !exists {
                    let sql = bind_placeholders("INSERT INTO category_manga (category, manga) VALUES (?, ?)");
                    {
                        sqlx::query(&sql).bind(category_id).bind(manga_id).execute(self.db.pool()).await?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Mirrors `removeMangaFromCategory` (default category removal is a no-op).
    pub async fn remove_manga_from_category(&self, manga_id: i32, category_id: i32) -> Result<()> {
        if category_id == CategoryService::DEFAULT_CATEGORY_ID {
            return Ok(());
        }
        let sql = bind_placeholders("DELETE FROM category_manga WHERE category = ? AND manga = ?");
        {
            sqlx::query(&sql).bind(category_id).bind(manga_id).execute(self.db.pool()).await?;
        }
        Ok(())
    }

    pub async fn remove_manga_from_all_categories(&self, manga_id: i32) -> Result<()> {
        let sql = bind_placeholders("DELETE FROM category_manga WHERE manga = ?");
        {
            sqlx::query(&sql).bind(manga_id).execute(self.db.pool()).await?;
        }
        Ok(())
    }

    /// Mirrors `getMangaCategories`.
    pub async fn get_manga_categories(&self, manga_id: i32) -> Result<Vec<CategoryDataClass>> {
        let sql = bind_placeholders(
            "SELECT c.* FROM category_manga cm INNER JOIN category c ON c.id = cm.category WHERE cm.manga = ? ORDER BY c.sort_order ASC");
        let rows = sqlx::query_as::<_, CategoryRow>(&sql).bind(manga_id).fetch_all(self.db.pool()).await?;
        Ok(rows
            .iter()
            .map(|r| CategoryDataClass {
                id: r.id,
                order: r.sort_order,
                name: r.name.clone(),
                default: r.is_default,
                include_in_update: IncludeOrExclude::from_i32(r.include_in_update),
                include_in_download: IncludeOrExclude::from_i32(r.include_in_download),
                version: r.version,
                uid: r.uid,
                last_modified_at: r.last_modified_at,
            })
            .collect())
    }

    /// Mirrors `getCategoryMangaList` — library manga in a category with
    /// chapter counts; DEFAULT_CATEGORY_ID means "no category".
    pub async fn get_category_manga_list(&self, category_id: i32) -> Result<Vec<MangaDataClass>> {
        let sql = if category_id == CategoryService::DEFAULT_CATEGORY_ID {
            "SELECT m.* FROM manga m LEFT JOIN category_manga cm ON cm.manga = m.id WHERE m.in_library = TRUE AND cm.manga IS NULL ORDER BY m.title ASC".to_string()
        } else {
            bind_placeholders(
                "SELECT m.* FROM manga m INNER JOIN category_manga cm ON cm.manga = m.id WHERE m.in_library = TRUE AND cm.category = ? ORDER BY m.title ASC")
        };
        let rows = {
            if category_id == CategoryService::DEFAULT_CATEGORY_ID {
                sqlx::query_as::<_, MangaRow>(&sql).fetch_all(self.db.pool()).await?
            } else {
                sqlx::query_as::<_, MangaRow>(&sql).bind(category_id).fetch_all(self.db.pool()).await?
            }
        };
        let mut out = Vec::new();
        for row in &rows {
            let mut dc = manga_row_to_data_class(row);
            let (unread, downloaded, chapter_count, last_read_at) = self.chapter_stats(row.id).await?;
            dc.unread_count = Some(unread);
            dc.download_count = Some(downloaded);
            dc.chapter_count = Some(chapter_count);
            dc.last_read_at = Some(last_read_at);
            out.push(dc);
        }
        Ok(out)
    }

    async fn chapter_stats(&self, manga_id: i32) -> Result<(i64, i64, i64, i64)> {
        let sql = bind_placeholders("SELECT * FROM chapter WHERE manga = ?");
        let mut unread = 0i64;
        let mut downloaded = 0i64;
        let mut total = 0i64;
        let mut last_read_at = 0i64;

        macro_rules! tally {
            ($rows:expr) => {{
                for row in $rows {
                    let read: bool = row.try_get("read").unwrap_or(false);
                    let is_downloaded: bool = row.try_get("is_downloaded").unwrap_or(false);
                    let lra: i64 = row.try_get("last_read_at").unwrap_or(0);
                    total += 1;
                    if !read {
                        unread += 1;
                    }
                    if is_downloaded {
                        downloaded += 1;
                    }
                    last_read_at = last_read_at.max(lra);
                }
            }};
        }

        {
            let rows = sqlx::query(&sql).bind(manga_id).fetch_all(self.db.pool()).await?;
            tally!(rows.iter());
        }
        Ok((unread, downloaded, total, last_read_at))
    }

    async fn exists(&self, manga_id: i32, category_id: i32) -> Result<bool> {
        let sql = bind_placeholders("SELECT count(*) FROM category_manga WHERE manga = ? AND category = ?");
        let n: i64 = sqlx::query_scalar(&sql).bind(manga_id).bind(category_id).fetch_one(self.db.pool()).await?;
        Ok(n > 0)
    }
}
