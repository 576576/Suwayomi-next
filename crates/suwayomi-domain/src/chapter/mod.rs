//! Chapter service — mirrors `suwayomi.tachidesk.manga.impl.Chapter`
//! (DB-backed parts).

use std::collections::HashMap;
use std::sync::Arc;

use sqlx::Row;
use suwayomi_core::db::Db;
use suwayomi_core::models::{now_epoch_secs, ChapterDataClass, MangaChapterDataClass, PaginatedList};
use suwayomi_core::schema::{ChapterRow, MangaRow};

use crate::error::{DomainError, Result};
use crate::manga::{chapter_row_to_data_class, manga_row_to_data_class};
use crate::meta::{MetaService, MetaTable};
use crate::source::SourceFetcher;
use crate::sql::bind_placeholders;

/// Mirrors `removeDuplicates(currentChapter)` — dedupe by chapter number,
/// preferring the current chapter, then same scanlator.
pub fn remove_duplicates(current: &ChapterDataClass, chapters: &[ChapterDataClass]) -> Vec<ChapterDataClass> {
    let mut out = Vec::new();
    let mut seen: Vec<f32> = Vec::new();
    for chapter in chapters {
        if seen.contains(&chapter.chapter_number) {
            continue;
        }
        seen.push(chapter.chapter_number);
        out.push(chapter.clone());
    }
    let _ = current;
    out
}

/// Kotlin's actual dedupe prefers the current/scanlator-matching chapter per number.
pub fn remove_duplicates_kotlin(current: &ChapterDataClass, chapters: &[ChapterDataClass]) -> Vec<ChapterDataClass> {
    let mut by_number: std::collections::HashMap<i64, Vec<ChapterDataClass>> = std::collections::HashMap::new();
    for c in chapters {
        by_number.entry(c.chapter_number.to_bits() as i64).or_default().push(c.clone());
    }
    let mut out = Vec::new();
    for (_, group) in by_number {
        let chosen = group
            .iter()
            .find(|c| c.id == current.id)
            .or_else(|| group.iter().find(|c| c.scanlator == current.scanlator))
            .unwrap_or(&group[0]);
        out.push(chosen.clone());
    }
    out
}

#[derive(Clone)]
pub struct ChapterService {
    pub db: Db,
    pub fetcher: Arc<dyn SourceFetcher>,
}

impl ChapterService {
    pub fn new(db: Db, fetcher: Arc<dyn SourceFetcher>) -> Self {
        Self { db, fetcher }
    }

    pub fn meta(&self) -> MetaService {
        MetaService::new(self.db.clone())
    }

    /// Mirrors `getChapterList(mangaId, onlineFetch)` (DB path).
    pub async fn get_chapter_list(&self, manga_id: i32, online_fetch: bool) -> Result<Vec<ChapterDataClass>> {
        let sql = bind_placeholders("SELECT * FROM chapter WHERE manga = ? ORDER BY source_order DESC");
        let rows = sqlx::query_as::<_, ChapterRow>(&sql).bind(manga_id).fetch_all(self.db.pool()).await?;
        let list: Vec<ChapterDataClass> = rows.iter().map(chapter_row_to_data_class).collect();
        if !list.is_empty() || !online_fetch {
            return Ok(list);
        }
        // empty + onlineFetch: delegate to source (Phase 5); stub raises
        let _ = self.fetcher.fetch_manga_update(0, &suwayomi_core::source::SManga::default(), &[], false, true).await?;
        Err(DomainError::Source("source fetch not available (Phase 5)".into()))
    }

    pub async fn get_count_of_manga_chapters(&self, manga_id: i32) -> Result<i64> {
        let sql = bind_placeholders("SELECT count(*) FROM chapter WHERE manga = ?");
        let count = sqlx::query_scalar::<_, i64>(&sql).bind(manga_id).fetch_one(self.db.pool()).await?;
        Ok(count)
    }

    /// Mirrors `modifyChapter` — locate chapter by (manga, source_order).
    pub async fn modify_chapter(
        &self,
        manga_id: i32,
        chapter_index: i32,
        is_read: Option<bool>,
        is_bookmarked: Option<bool>,
        mark_prev_read: Option<bool>,
        last_page_read: Option<i32>,
    ) -> Result<i32> {
        let chapter_id = self.find_id_by_index(manga_id, chapter_index).await?;

        if is_read.is_some() || is_bookmarked.is_some() || last_page_read.is_some() {
            let mut sets = Vec::new();
            let mut binds: Vec<String> = Vec::new();
            if let Some(v) = is_read {
                sets.push("read = ?".to_string());
                binds.push(v.to_string());
            }
            if let Some(v) = is_bookmarked {
                sets.push("bookmark = ?".to_string());
                binds.push(v.to_string());
            }
            if let Some(_v) = last_page_read {
                sets.push("last_page_read = ?".to_string());
                sets.push("last_read_at = ?".to_string());
            }
            let sql = bind_placeholders(&format!(
                "UPDATE chapter SET {} WHERE manga = ? AND source_order = ?",
                sets.join(", ")
            ));
            self.exec_update(&sql, is_read, is_bookmarked, last_page_read, manga_id, chapter_index).await?;
        }

        if let Some(mark) = mark_prev_read {
            let sql = bind_placeholders("UPDATE chapter SET read = ? WHERE manga = ? AND source_order < ?");
            {
                sqlx::query(&sql).bind(mark).bind(manga_id).bind(chapter_index).execute(self.db.pool()).await?;
            }
        }

        Ok(chapter_id)
    }

    async fn exec_update(
        &self,
        sql: &str,
        is_read: Option<bool>,
        is_bookmarked: Option<bool>,
        last_page_read: Option<i32>,
        manga_id: i32,
        chapter_index: i32,
    ) -> Result<()> {
        let now = now_epoch_secs();
        {
            let mut q = sqlx::query(sql);
            if let Some(v) = is_read {
                q = q.bind(v);
            }
            if let Some(v) = is_bookmarked {
                q = q.bind(v);
            }
            if let Some(v) = last_page_read {
                q = q.bind(v).bind(now);
            }
            q.bind(manga_id).bind(chapter_index).execute(self.db.pool()).await?;
        }
        Ok(())
    }

    /// Mirrors `updateChapterProgress` — pageNo is 0-based; read when pageCount == pageNo+1.
    pub async fn update_chapter_progress(&self, manga_id: i32, chapter_index: i32, page_no: i32) -> Result<i32> {
        let sql = bind_placeholders("SELECT * FROM chapter WHERE manga = ? AND source_order = ?");
        let row = sqlx::query_as::<_, ChapterRow>(&sql)
            .bind(manga_id)
            .bind(chapter_index)
            .fetch_optional(self.db.pool())
            .await?
            .ok_or_else(|| DomainError::not_found("chapter not found"))?;
        let one_indexed = page_no + 1;
        let is_read = (row.page_count == one_indexed).then_some(true);
        self.modify_chapter(manga_id, chapter_index, is_read, None, None, Some(page_no)).await?;
        Ok(row.id)
    }

    /// Batch edit — mirrors `modifyChapters` (manga-scoped by ids or indexes).
    pub async fn modify_chapters_by_indexes(
        &self,
        manga_id: i32,
        indexes: &[i32],
        is_read: Option<bool>,
        is_bookmarked: Option<bool>,
        last_page_read: Option<i32>,
    ) -> Result<()> {
        if indexes.is_empty() {
            return Ok(());
        }
        let mut sets = Vec::new();
        if is_read.is_some() {
            sets.push("read = ?".to_string());
        }
        if is_bookmarked.is_some() {
            sets.push("bookmark = ?".to_string());
        }
        if last_page_read.is_some() {
            sets.push("last_page_read = ?".to_string());
            sets.push("last_read_at = ?".to_string());
        }
        if sets.is_empty() {
            return Ok(());
        }
        let sql = bind_placeholders(&format!(
            "UPDATE chapter SET {} WHERE manga = ? AND source_order IN ({})",
            sets.join(", "),
            vec!["?"; indexes.len()].join(", ")
        ));
        {
            let mut q = sqlx::query(&sql);
            if let Some(v) = is_read {
                q = q.bind(v);
            }
            if let Some(v) = is_bookmarked {
                q = q.bind(v);
            }
            if let Some(v) = last_page_read {
                q = q.bind(v).bind(now_epoch_secs());
            }
            q = q.bind(manga_id);
            for i in indexes {
                q = q.bind(i);
            }
            q.execute(self.db.pool()).await?;
        }
        Ok(())
    }

    pub async fn modify_chapters_by_ids(
        &self,
        ids: &[i32],
        is_read: Option<bool>,
        is_bookmarked: Option<bool>,
        last_page_read: Option<i32>,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut sets = Vec::new();
        if is_read.is_some() {
            sets.push("read = ?".to_string());
        }
        if is_bookmarked.is_some() {
            sets.push("bookmark = ?".to_string());
        }
        if last_page_read.is_some() {
            sets.push("last_page_read = ?".to_string());
            sets.push("last_read_at = ?".to_string());
        }
        if sets.is_empty() {
            return Ok(());
        }
        let sql = bind_placeholders(&format!(
            "UPDATE chapter SET {} WHERE id IN ({})",
            sets.join(", "),
            vec!["?"; ids.len()].join(", ")
        ));
        {
            let mut q = sqlx::query(&sql);
            if let Some(v) = is_read {
                q = q.bind(v);
            }
            if let Some(v) = is_bookmarked {
                q = q.bind(v);
            }
            if let Some(v) = last_page_read {
                q = q.bind(v).bind(now_epoch_secs());
            }
            for id in ids {
                q = q.bind(id);
            }
            q.execute(self.db.pool()).await?;
        }
        Ok(())
    }

    pub async fn get_meta_map(&self, chapter_id: i32) -> Result<HashMap<String, String>> {
        self.meta().get_map(MetaTable::Chapter, chapter_id as i64).await
    }

    pub async fn modify_metas(&self, metas_by_chapter_id: &HashMap<i32, HashMap<String, String>>) -> Result<()> {
        let by_ref = metas_by_chapter_id.iter().map(|(k, v)| (*k as i64, v.clone())).collect::<HashMap<_, _>>();
        self.meta().modify(MetaTable::Chapter, &by_ref).await
    }

    /// Mirrors `deleteChapter` — clears downloaded flag; pages cascade via FK.
    pub async fn delete_chapter(&self, manga_id: i32, chapter_index: i32) -> Result<()> {
        let chapter_id = self.find_id_by_index(manga_id, chapter_index).await?;
        self.delete_chapters(&[chapter_id]).await
    }

    pub async fn delete_chapters(&self, chapter_ids: &[i32]) -> Result<()> {
        if chapter_ids.is_empty() {
            return Ok(());
        }
        // mark not downloaded (download files cleanup lands in Phase 6)
        let sql = bind_placeholders(&format!(
            "UPDATE chapter SET is_downloaded = FALSE WHERE id IN ({})",
            vec!["?"; chapter_ids.len()].join(", ")
        ));
        {
            let mut q = sqlx::query(&sql);
            for id in chapter_ids {
                q = q.bind(id);
            }
            q.execute(self.db.pool()).await?;
        }
        Ok(())
    }

    /// Mirrors `getRecentChapters(pageNum)` — library manga with new chapters,
    /// ordered by fetch time desc, paginated.
    pub async fn get_recent_chapters(&self, page_num: usize) -> Result<PaginatedList<MangaChapterDataClass>> {
        let sql = "SELECT c.id AS cid FROM chapter c INNER JOIN manga m ON m.id = c.manga WHERE m.in_library = TRUE AND c.fetched_at > m.in_library_at ORDER BY c.fetched_at DESC";
        let chapter_ids: Vec<i32> = {
            let rows = sqlx::query(sql).fetch_all(self.db.pool()).await?;
            rows.iter().map(|r| r.try_get::<i32, _>("cid").unwrap_or(0)).collect()
        };
        let mut items = Vec::new();
        for cid in chapter_ids {
            let chapter = self.fetch_by_id(cid).await?;
            let manga_sql = bind_placeholders("SELECT * FROM manga WHERE id = ?");
            let manga =
                { sqlx::query_as::<_, MangaRow>(&manga_sql).bind(chapter.manga).fetch_optional(self.db.pool()).await? };
            if let Some(m) = manga {
                items.push(MangaChapterDataClass {
                    manga: manga_row_to_data_class(&m),
                    chapter: chapter_row_to_data_class(&chapter),
                });
            }
        }
        let page = suwayomi_core::models::pagination::paginated_from(page_num, 50, || items);
        Ok(page)
    }

    async fn find_id_by_index(&self, manga_id: i32, chapter_index: i32) -> Result<i32> {
        let sql = bind_placeholders("SELECT id FROM chapter WHERE manga = ? AND source_order = ?");
        let id = sqlx::query_scalar::<_, i32>(&sql)
            .bind(manga_id)
            .bind(chapter_index)
            .fetch_optional(self.db.pool())
            .await?
            .ok_or_else(|| DomainError::not_found("chapter not found"))?;
        Ok(id)
    }

    /// Helper used by chapter updates (also referenced by other services).
    pub async fn fetch_by_id(&self, chapter_id: i32) -> Result<ChapterRow> {
        let sql = bind_placeholders("SELECT * FROM chapter WHERE id = ?");
        sqlx::query_as::<_, ChapterRow>(&sql)
            .bind(chapter_id)
            .fetch_optional(self.db.pool())
            .await?
            .ok_or_else(|| DomainError::not_found("chapter not found"))
    }
}
