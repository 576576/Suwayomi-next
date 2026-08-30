//! Manga service — mirrors `suwayomi.tachidesk.manga.impl.Manga` (DB-backed parts).

pub mod library;
pub mod manga_list;

use std::collections::HashMap;
use std::sync::Arc;

use suwayomi_core::db::Db;
use suwayomi_core::models::{
    now_epoch_secs, to_genre_list, ChapterDataClass, MangaDataClass, MangaStatus, UpdateStrategy,
};
use suwayomi_core::schema::{ChapterRow, MangaRow};
use suwayomi_core::source::SManga;

use crate::category::category_manga::CategoryMangaService;
use crate::category::CategoryService;
use crate::error::{DomainError, Result};
use crate::meta::{MetaService, MetaTable};
use crate::source::SourceFetcher;
use crate::sql::bind_placeholders;

/// proxyThumbnailUrl
pub fn proxy_thumbnail_url(manga_id: i32) -> String {
    format!("/api/v1/manga/{manga_id}/thumbnail")
}

/// Mirrors `MangaTable.toDataClass`.
pub fn manga_row_to_data_class(row: &MangaRow) -> MangaDataClass {
    MangaDataClass {
        id: row.id,
        source_id: row.source.to_string(),
        url: row.url.clone(),
        title: row.title.clone(),
        thumbnail_url: Some(proxy_thumbnail_url(row.id)),
        thumbnail_url_last_fetched: row.thumbnail_url_last_fetched,
        initialized: row.initialized,
        artist: row.artist.clone(),
        author: row.author.clone(),
        description: row.description.clone(),
        genre: to_genre_list(row.genre.as_deref()),
        status: MangaStatus::from_i32(row.status),
        in_library: row.in_library,
        in_library_at: row.in_library_at,
        source: None,
        real_url: row.real_url.clone(),
        // Kotlin defaults are 0 (non-null); age = now - lastFetchedAt
        last_fetched_at: Some(row.last_fetched_at),
        chapters_last_fetched_at: Some(row.chapters_last_fetched_at),
        update_strategy: UpdateStrategy::from_db(&row.update_strategy),
        fresh_data: false,
        unread_count: None,
        download_count: None,
        chapter_count: None,
        last_read_at: None,
        last_chapter_read: None,
        age: Some(now_epoch_secs() - row.last_fetched_at),
        chapters_age: Some(now_epoch_secs() - row.chapters_last_fetched_at),
        trackers: None,
        last_modified_at: row.last_modified_at,
        version: row.version,
        memo: parse_memo(&row.memo),
    }
}

#[derive(Clone)]
pub struct MangaService {
    pub db: Db,
    pub fetcher: Arc<dyn SourceFetcher>,
}

impl MangaService {
    pub fn new(db: Db, fetcher: Arc<dyn SourceFetcher>) -> Self {
        Self { db, fetcher }
    }

    pub fn meta(&self) -> MetaService {
        MetaService::new(self.db.clone())
    }

    async fn fetch_row(&self, manga_id: i32) -> Result<MangaRow> {
        let sql = bind_placeholders("SELECT * FROM manga WHERE id = ?");
        sqlx::query_as::<_, MangaRow>(&sql)
            .bind(manga_id)
            .fetch_optional(self.db.pool())
            .await?
            .ok_or_else(|| DomainError::not_found(format!("Manga with id {manga_id} was not found")))
    }

    /// Mirrors `getManga(mangaId, onlineFetch)`.
    pub async fn get_manga(&self, manga_id: i32, online_fetch: bool) -> Result<MangaDataClass> {
        let row = self.fetch_row(manga_id).await?;
        if !online_fetch && row.initialized {
            return Ok(manga_row_to_data_class(&row));
        }
        // initialize manga via the source (JVM sandbox, Phase 5)
        let s_manga = SManga {
            url: row.url.clone(),
            title: row.title.clone(),
            thumbnail_url: row.thumbnail_url.clone(),
            artist: row.artist.clone(),
            author: row.author.clone(),
            description: row.description.clone(),
            genre: row.genre.clone(),
            status: row.status,
            update_strategy: UpdateStrategy::from_db(&row.update_strategy),
            initialized: row.initialized,
            memo: parse_memo(&row.memo),
        };
        let (updated, _) = self.fetcher.fetch_manga_update(row.source, &s_manga, &[], true, false).await?;
        let _ = updated; // DB write of fetched data lands with the sandbox path (Phase 5)

        let row = self.fetch_row(manga_id).await?;
        let mut dc = manga_row_to_data_class(&row);
        dc.fresh_data = true;
        Ok(dc)
    }

    /// Mirrors `getMangaFull(mangaId, onlineFetch)`.
    pub async fn get_manga_full(&self, manga_id: i32, online_fetch: bool) -> Result<MangaDataClass> {
        let mut dc = self.get_manga(manga_id, online_fetch).await?;
        let sql = bind_placeholders("SELECT * FROM chapter WHERE manga = ?");
        let rows = sqlx::query_as::<_, ChapterRow>(&sql).bind(manga_id).fetch_all(self.db.pool()).await?;
        let unread_count = rows.iter().filter(|c| !c.read).count() as i64;
        let download_count = rows.iter().filter(|c| c.is_downloaded).count() as i64;
        let chapter_count = rows.len() as i64;
        let last_read = rows.iter().filter(|c| c.read).max_by_key(|c| c.source_order);
        dc.unread_count = Some(unread_count);
        dc.download_count = Some(download_count);
        dc.chapter_count = Some(chapter_count);
        dc.last_chapter_read = last_read.map(chapter_row_to_data_class);
        Ok(dc)
    }

    /// getMangaMetaMap
    pub async fn get_meta_map(&self, manga_id: i32) -> Result<HashMap<String, String>> {
        self.meta().get_map(MetaTable::Manga, manga_id as i64).await
    }

    /// modifyMangaMeta / modifyMangasMetas
    pub async fn modify_metas(&self, metas_by_manga_id: &HashMap<i32, HashMap<String, String>>) -> Result<()> {
        let by_ref = metas_by_manga_id.iter().map(|(k, v)| (*k as i64, v.clone())).collect::<HashMap<_, _>>();
        self.meta().modify(MetaTable::Manga, &by_ref).await
    }

    /// getLatestChapter
    pub async fn get_latest_chapter(&self, manga_id: i32) -> Result<Option<ChapterDataClass>> {
        let sql = bind_placeholders("SELECT * FROM chapter WHERE manga = ? ORDER BY source_order DESC LIMIT 1");
        let row = sqlx::query_as::<_, ChapterRow>(&sql).bind(manga_id).fetch_optional(self.db.pool()).await?;
        Ok(row.map(|r| chapter_row_to_data_class(&r)))
    }

    /// getUnreadChapters
    pub async fn get_unread_chapters(&self, manga_id: i32) -> Result<Vec<ChapterDataClass>> {
        let sql =
            bind_placeholders("SELECT * FROM chapter WHERE manga = ? AND read = FALSE ORDER BY source_order DESC");
        let rows = sqlx::query_as::<_, ChapterRow>(&sql).bind(manga_id).fetch_all(self.db.pool()).await?;
        Ok(rows.iter().map(chapter_row_to_data_class).collect())
    }

    /// isInIncludedDownloadCategory
    pub async fn is_in_included_download_category(&self, manga_id: i32) -> Result<bool> {
        let category_service = CategoryService::new(self.db.clone());
        let cm = CategoryMangaService::new(self.db.clone());

        let mut manga_categories = cm.get_manga_categories(manga_id).await?;
        if manga_categories.is_empty() {
            let default = category_service
                .get_category_by_id(CategoryService::DEFAULT_CATEGORY_ID)
                .await?
                .ok_or_else(|| DomainError::not_found("Default category not found"))?;
            manga_categories = vec![default];
        }
        let manga_categories: std::collections::HashSet<i32> = manga_categories.iter().map(|c| c.id).collect();

        let categories = category_service.get_category_list().await?;
        let included: std::collections::HashSet<i32> = categories
            .iter()
            .filter(|c| matches!(c.include_in_download, suwayomi_core::models::IncludeOrExclude::Include))
            .map(|c| c.id)
            .collect();
        let excluded: std::collections::HashSet<i32> = categories
            .iter()
            .filter(|c| matches!(c.include_in_download, suwayomi_core::models::IncludeOrExclude::Exclude))
            .map(|c| c.id)
            .collect();

        if !manga_categories.is_disjoint(&excluded) {
            return Ok(false);
        }
        // If no category is explicitly included, unset categories count as included
        let included_or_unset: std::collections::HashSet<i32> = categories
            .iter()
            .filter(|c| {
                matches!(
                    c.include_in_download,
                    suwayomi_core::models::IncludeOrExclude::Include | suwayomi_core::models::IncludeOrExclude::Unset
                )
            })
            .map(|c| c.id)
            .collect();
        let effective: &std::collections::HashSet<i32> =
            if included.is_empty() { &included_or_unset } else { &included };
        Ok(!manga_categories.is_disjoint(effective))
    }
}

/// Parses the JSON `memo` column (stored as TEXT, mirroring Kotlin's column).
pub fn parse_memo(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or(serde_json::Value::Null)
}

/// Mirrors `ChapterTable.toDataClass`.
pub fn chapter_row_to_data_class(row: &ChapterRow) -> ChapterDataClass {
    ChapterDataClass {
        id: row.id,
        url: row.url.clone(),
        name: row.name.clone(),
        upload_date: row.date_upload,
        chapter_number: row.chapter_number,
        scanlator: row.scanlator.clone(),
        manga_id: row.manga,
        read: row.read,
        bookmarked: row.bookmark,
        last_page_read: row.last_page_read,
        last_read_at: row.last_read_at,
        index: row.source_order,
        fetched_at: row.fetched_at,
        real_url: row.real_url.clone(),
        downloaded: row.is_downloaded,
        page_count: row.page_count,
        last_modified_at: row.last_modified_at,
        version: row.version,
        memo: parse_memo(&row.memo),
    }
}
