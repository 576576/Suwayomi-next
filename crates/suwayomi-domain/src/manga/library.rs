//! Library service — mirrors `suwayomi.manga.impl.Library`.

use suwayomi_core::db::Db;

use crate::category::category_manga::CategoryMangaService;
use crate::category::CategoryService;
use crate::error::Result;
use crate::manga::MangaService;
use crate::sql::bind_placeholders;

#[derive(Clone)]
pub struct LibraryService {
    pub db: Db,
    pub manga: MangaService,
}

impl LibraryService {
    pub fn new(db: Db, manga: MangaService) -> Self {
        Self { db, manga }
    }

    /// Mirrors `addMangaToLibrary` — marks in_library and attaches default
    /// categories when the manga has no categories yet.
    pub async fn add_manga_to_library(&self, manga_id: i32) -> Result<()> {
        let dc = self.manga.get_manga(manga_id, false).await?;
        if dc.in_library {
            return Ok(());
        }

        let default_sql = bind_placeholders("SELECT * FROM category WHERE is_default = TRUE AND id != ?");
        let defaults = {
            sqlx::query_as::<_, suwayomi_core::schema::CategoryRow>(&default_sql)
                .bind(CategoryService::DEFAULT_CATEGORY_ID)
                .fetch_all(self.db.pool())
                .await?
        };

        let existing_sql = bind_placeholders("SELECT count(*) FROM category_manga WHERE manga = ?");
        let existing: i64 = sqlx::query_scalar(&existing_sql).bind(manga_id).fetch_one(self.db.pool()).await?;

        let now = suwayomi_core::models::now_epoch_secs();
        let update_sql = bind_placeholders("UPDATE manga SET in_library = TRUE, in_library_at = ? WHERE id = ?");
        {
            sqlx::query(&update_sql).bind(now).bind(manga_id).execute(self.db.pool()).await?;
        }

        if existing == 0 && !defaults.is_empty() {
            let ids: Vec<i32> = defaults.iter().map(|r| r.id).collect();
            CategoryMangaService::new(self.db.clone()).add_mangas_to_categories(&[manga_id], &ids).await?;
        }
        Ok(())
    }

    /// Mirrors `removeMangaFromLibrary`.
    pub async fn remove_manga_from_library(&self, manga_id: i32) -> Result<()> {
        let dc = self.manga.get_manga(manga_id, false).await?;
        if !dc.in_library {
            return Ok(());
        }
        let sql = bind_placeholders("UPDATE manga SET in_library = FALSE WHERE id = ?");
        {
            sqlx::query(&sql).bind(manga_id).execute(self.db.pool()).await?;
        }
        Ok(())
    }
}
