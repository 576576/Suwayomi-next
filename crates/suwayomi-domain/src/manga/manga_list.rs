//! Manga list service — mirrors `suwayomi.manga.impl.MangaList`
//! (DB-backed insert-or-update of browsed source manga).

use std::sync::Arc;

use suwayomi_core::db::Db;
use suwayomi_core::models::PagedMangaListDataClass;
use suwayomi_core::schema::MangaRow;
use suwayomi_core::source::{MangasPage, SManga};

use crate::error::{DomainError, Result};
use crate::manga::manga_row_to_data_class;
use crate::source::{SourceFetcher, LOCAL_SOURCE_ID};
use crate::sql::bind_placeholders;

#[derive(Clone)]
pub struct MangaListService {
    pub db: Db,
    pub fetcher: Arc<dyn SourceFetcher>,
}

impl MangaListService {
    pub fn new(db: Db, fetcher: Arc<dyn SourceFetcher>) -> Self {
        Self { db, fetcher }
    }

    /// Mirrors `MangasPage.insertOrUpdate(sourceId)`.
    /// Inserts new manga rows, updates existing non-library rows, returns ids in input order.
    pub async fn insert_or_update(&self, source_id: i64, mangas: &[SManga]) -> Result<Vec<i32>> {
        let mut existing_by_url: std::collections::HashMap<String, MangaRow> = std::collections::HashMap::new();
        for s in mangas {
            let q = bind_placeholders("SELECT * FROM manga WHERE source = ? AND url = ?");
            let row = {
                sqlx::query_as::<_, MangaRow>(&q).bind(source_id).bind(&s.url).fetch_optional(self.db.pool()).await?
            };
            if let Some(r) = row {
                existing_by_url.insert(r.url.clone(), r);
            }
        }

        let mut ids: Vec<i32> = Vec::with_capacity(mangas.len());
        for s in mangas {
            match existing_by_url.get(&s.url) {
                None => {
                    let id = self.insert_manga(source_id, s).await?;
                    ids.push(id);
                }
                Some(existing) => {
                    // Kotlin: skip updating manga that are in the library (except local source)
                    if !existing.in_library || existing.source == LOCAL_SOURCE_ID {
                        self.update_manga(existing, s).await?;
                    }
                    ids.push(existing.id);
                }
            }
        }
        Ok(ids)
    }

    async fn insert_manga(&self, source_id: i64, s: &SManga) -> Result<i32> {
        let sql = bind_placeholders(
            "INSERT INTO manga (url, title, artist, author, description, genre, status, thumbnail_url, update_strategy, memo, source, initialized) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, TRUE)");
        let memo = s.memo.to_string();
        let last_id = {
            let sql = format!("{sql} RETURNING id");
            let row: (i32,) = sqlx::query_as(&sql)
                .bind(&s.url)
                .bind(&s.title)
                .bind(&s.artist)
                .bind(&s.author)
                .bind(&s.description)
                .bind(&s.genre)
                .bind(s.status)
                .bind(&s.thumbnail_url)
                .bind(s.update_strategy.to_db())
                .bind(memo)
                .bind(source_id)
                .fetch_one(self.db.pool())
                .await?;
            row.0
        };
        Ok(last_id)
    }

    async fn update_manga(&self, existing: &MangaRow, s: &SManga) -> Result<()> {
        let sql = bind_placeholders(
            "UPDATE manga SET title = ?, artist = COALESCE(?, artist), author = COALESCE(?, author), description = COALESCE(?, description), genre = COALESCE(?, genre), status = ?, thumbnail_url = COALESCE(?, thumbnail_url), update_strategy = ?, memo = ?, thumbnail_url_last_fetched = ? WHERE id = ?");
        let memo = s.memo.to_string();
        let thumbnail_changed =
            !s.thumbnail_url.as_deref().unwrap_or("").is_empty() && existing.thumbnail_url != s.thumbnail_url;
        let last_fetched =
            if thumbnail_changed { crate::manga::now_epoch_secs() } else { existing.thumbnail_url_last_fetched };
        {
            sqlx::query(&sql)
                .bind(&s.title)
                .bind(&s.artist)
                .bind(&s.author)
                .bind(&s.description)
                .bind(&s.genre)
                .bind(s.status)
                .bind(&s.thumbnail_url)
                .bind(s.update_strategy.to_db())
                .bind(memo)
                .bind(last_fetched)
                .bind(existing.id)
                .execute(self.db.pool())
                .await?;
        }
        Ok(())
    }

    /// Mirrors `MangasPage.processEntries(sourceId)`.
    pub async fn process_entries(&self, source_id: i64, page: &MangasPage) -> Result<PagedMangaListDataClass> {
        let ids = self.insert_or_update(source_id, &page.mangas).await?;
        let mut manga_list = Vec::with_capacity(ids.len());
        for id in ids {
            let sql = bind_placeholders("SELECT * FROM manga WHERE id = ?");
            let row = sqlx::query_as::<_, MangaRow>(&sql)
                .bind(id)
                .fetch_optional(self.db.pool())
                .await?
                .ok_or_else(|| DomainError::not_found(format!("manga {id} missing after insert")))?;
            manga_list.push(manga_row_to_data_class(&row));
        }
        Ok(PagedMangaListDataClass { manga_list, has_next_page: page.has_next_page })
    }

    /// Mirrors `getMangaList(sourceId, pageNum, popular)` — DB side is a no-op;
    /// fetching is delegated to the `SourceFetcher`.
    pub async fn get_manga_list(
        &self,
        source_id: i64,
        page_num: u32,
        popular: bool,
    ) -> Result<PagedMangaListDataClass> {
        if page_num == 0 {
            return Err(DomainError::invalid("pageNum = 0 is not in valid range"));
        }
        let page = if popular {
            self.fetcher.get_popular_manga(source_id, page_num).await?
        } else if self.fetcher.supports_latest(source_id) {
            self.fetcher.get_latest_updates(source_id, page_num).await?
        } else {
            return Err(DomainError::invalid(format!("Source {source_id} doesn't support latest")));
        };
        self.process_entries(source_id, &page).await
    }
}
