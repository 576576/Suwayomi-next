//! OPDS data access — mirrors `opds/repository/*.kt`.
//!
//! All queries run against the `suwayomi` schema (search_path is set by the
//! connection hook); unqualified names resolve via search_path.

use sqlx::FromRow;
use sqlx::PgPool;

use crate::constants::ITEMS_PER_PAGE;

/// Flat join row: manga + source name/lang (library & search feeds).
#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)] // FromRow keeps all columns; not every field is read
struct MangaJoinedRow {
    id: i32,
    url: String,
    title: String,
    initialized: bool,
    artist: Option<String>,
    author: Option<String>,
    description: Option<String>,
    genre: Option<String>,
    status: i32,
    thumbnail_url: Option<String>,
    thumbnail_url_last_fetched: i64,
    in_library: bool,
    in_library_at: i64,
    source: i64,
    real_url: Option<String>,
    last_fetched_at: i64,
    chapters_last_fetched_at: i64,
    update_strategy: String,
    last_modified_at: i64,
    version: i64,
    is_syncing: bool,
    memo: String,
    source_name: String,
    source_lang: String,
}

/// Flat join row: chapter + manga summary + total chapters.
#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
struct ChapterJoinedRow {
    id: i32,
    name: String,
    date_upload: i64,
    chapter_number: f32,
    scanlator: Option<String>,
    last_page_read: i32,
    last_read_at: i64,
    source_order: i32,
    is_downloaded: bool,
    page_count: i32,
    manga_id: i32,
    manga_title: String,
    manga_author: Option<String>,
    manga_thumbnail_url: Option<String>,
    manga_total_chapters: i64,
}

/// Manga entry for an OPDS acquisition feed.
#[derive(Debug, Clone)]
pub struct MangaAcqEntry {
    pub id: i32,
    pub title: String,
    pub url: Option<String>,
    pub author: Option<String>,
    pub genres: Vec<String>,
    pub status: i32,
    pub description: Option<String>,
    pub thumbnail_url: Option<String>,
    pub last_fetched_at: i64,
    pub source_name: String,
    pub source_lang: String,
    pub in_library: bool,
    pub total_chapters: i64,
}

/// Manga details for series/chapter feeds.
#[derive(Debug, Clone)]
pub struct MangaDetails {
    pub id: i32,
    pub title: String,
    pub author: Option<String>,
    pub thumbnail_url: Option<String>,
    pub total_chapters: i64,
}

/// Chapter entry for a chapter-list acquisition feed.
#[derive(Debug, Clone)]
pub struct ChapterListEntry {
    pub id: i32,
    pub name: String,
    pub chapter_number: f32,
    pub source_order: i32,
    pub scanlator: Option<String>,
    pub last_page_read: i32,
    pub last_read_at: i64,
    pub page_count: i32,
    pub downloaded: bool,
    pub upload_date: i64,
    pub manga_id: i32,
    pub manga_title: String,
    pub manga_author: Option<String>,
    pub manga_thumbnail_url: Option<String>,
    pub manga_total_chapters: i64,
}

/// Chapter metadata for the per-chapter details feed.
#[derive(Debug, Clone)]
pub struct ChapterMetadataEntry {
    pub id: i32,
    pub name: String,
    pub chapter_number: f32,
    pub source_order: i32,
    pub scanlator: Option<String>,
    pub last_page_read: i32,
    pub last_read_at: i64,
    pub page_count: i32,
    pub downloaded: bool,
    pub read: bool,
    pub bookmark: bool,
    pub upload_date: i64,
}

/// Generic navigation item (category / genre / status / language / source).
#[derive(Debug, Clone)]
pub struct NavEntry {
    pub id: String,
    pub title: String,
    pub manga_count: Option<usize>,
    pub description: Option<String>,
}

/// Paginated query result.
#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
}

pub struct OpdsRepository<'p> {
    pool: &'p PgPool,
}

const MANGA_SELECT: &str = "SELECT m.id, m.url, m.title, m.initialized, m.artist, m.author, m.description, m.genre, m.status, \
     m.thumbnail_url, m.thumbnail_url_last_fetched, m.in_library, m.in_library_at, m.source, m.real_url, \
     m.last_fetched_at, m.chapters_last_fetched_at, m.update_strategy, m.last_modified_at, m.version, \
     m.is_syncing, m.memo, s.name AS source_name, s.lang AS source_lang \
     FROM manga m JOIN source s ON s.id = m.source";

/// Library sort keys (mirror Suwayomi library sort enum used by OPDS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Title,
    DateAdded,
    LastReadAt,
    LastModifiedAt,
    LatestUpload,
    TotalChapters,
    Unread,
}

impl SortKey {
    /// Parses a sort key from an OPDS `sort` query param.
    pub fn parse(s: &str) -> Self {
        match s {
            "date_added" | "date-added" | "recently_added" => Self::DateAdded,
            "last_read_at" | "last_read" => Self::LastReadAt,
            "last_modified_at" | "last_modified" => Self::LastModifiedAt,
            "latest_upload" | "latest_uploaded" => Self::LatestUpload,
            "total_chapters" | "chapter_count" => Self::TotalChapters,
            "unread" => Self::Unread,
            _ => Self::Title,
        }
    }
}

/// Library filters supported by the OPDS library feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryFilter {
    All,
    Unread,
    Downloaded,
    Ongoing,
    Completed,
}

impl LibraryFilter {
    /// Parses a filter from an OPDS `filter` query param.
    pub fn parse(s: &str) -> Self {
        match s {
            "unread" => Self::Unread,
            "downloaded" => Self::Downloaded,
            "ongoing" => Self::Ongoing,
            "completed" => Self::Completed,
            _ => Self::All,
        }
    }
}

fn split_genres(g: Option<&str>) -> Vec<String> {
    g.unwrap_or("")
        .split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

impl<'p> OpdsRepository<'p> {
    pub fn new(pool: &'p PgPool) -> Self {
        Self { pool }
    }

    fn to_acq(r: MangaJoinedRow, total_chapters: i64) -> MangaAcqEntry {
        MangaAcqEntry {
            id: r.id,
            title: r.title,
            url: r.real_url.clone().or(Some(r.url)),
            author: r.author.clone(),
            genres: split_genres(r.genre.as_deref()),
            status: r.status,
            description: r.description.clone(),
            thumbnail_url: r.thumbnail_url.clone(),
            last_fetched_at: r.last_fetched_at,
            source_name: r.source_name,
            source_lang: r.source_lang,
            in_library: r.in_library,
            total_chapters,
        }
    }

    /// Library manga feed with cross-filters + sort (mirrors `MangaRepository.getLibraryManga`).
    #[allow(clippy::too_many_arguments)]
    pub async fn library_manga(
        &self,
        source_id: Option<i64>,
        category_id: Option<i32>,
        status_id: Option<i32>,
        lang_code: Option<&str>,
        genre: Option<&str>,
        page_num: usize,
        sort: SortKey,
        filter: LibraryFilter,
    ) -> Result<Page<MangaAcqEntry>, sqlx::Error> {
        let mut params: Vec<String> = Vec::new();
        if let Some(id) = source_id {
            params.push(format!("m.source = {id}"));
        }
        if let Some(cid) = category_id {
            params.push(format!("EXISTS (SELECT 1 FROM category_manga cm WHERE cm.manga = m.id AND cm.category = {cid})"));
        }
        if let Some(sid) = status_id {
            params.push(format!("m.status = {sid}"));
        }
        if let Some(lang) = lang_code.filter(|l| !l.is_empty()) {
            params.push(format!("s.lang = '{}'", lang.replace('\'', "''")));
        }
        if let Some(g) = genre.filter(|g| !g.is_empty()) {
            params.push(format!("m.genre ILIKE '%{}%'", g.replace('\'', "''")));
        }
        match filter {
            LibraryFilter::Unread => {
                params.push("EXISTS (SELECT 1 FROM chapter c WHERE c.manga = m.id AND c.read = FALSE)".into())
            }
            LibraryFilter::Downloaded => {
                params.push("EXISTS (SELECT 1 FROM chapter c WHERE c.manga = m.id AND c.is_downloaded = TRUE)".into())
            }
            LibraryFilter::Ongoing => params.push("m.status = 1".into()),
            LibraryFilter::Completed => params.push("m.status = 2".into()),
            LibraryFilter::All => {}
        }

        let order = match sort {
            SortKey::Title => "m.title ASC",
            SortKey::DateAdded => "m.in_library_at DESC, m.id DESC",
            SortKey::LastReadAt => "(SELECT MAX(c.last_read_at) FROM chapter c WHERE c.manga = m.id) DESC NULLS LAST, m.id DESC",
            SortKey::LastModifiedAt => "m.last_modified_at DESC, m.id DESC",
            SortKey::LatestUpload => "(SELECT MAX(c.date_upload) FROM chapter c WHERE c.manga = m.id) DESC NULLS LAST, m.id DESC",
            SortKey::TotalChapters => "(SELECT COUNT(*) FROM chapter c WHERE c.manga = m.id) DESC, m.id DESC",
            SortKey::Unread => "(SELECT COUNT(*) FROM chapter c WHERE c.manga = m.id AND c.read = FALSE) DESC, m.id DESC",
        };

        let offset = (page_num.saturating_sub(1)) * ITEMS_PER_PAGE;
        let base = if params.is_empty() { "WHERE m.in_library = TRUE".to_string() } else { format!("WHERE m.in_library = TRUE AND {}", params.join(" AND ")) };
        let sql = format!("{MANGA_SELECT} {base} ORDER BY {order} LIMIT {ITEMS_PER_PAGE} OFFSET {offset}");
        let count_sql = format!("SELECT COUNT(*) FROM manga m JOIN source s ON s.id = m.source {base}");

        let total: i64 = sqlx::query_scalar(&count_sql).fetch_one(self.pool).await?;
        let rows: Vec<MangaJoinedRow> = sqlx::query_as(&sql).fetch_all(self.pool).await?;
        let items = self.attach_total_chapters(rows).await?;
        Ok(Page { items, total })
    }

    /// Search library manga by query/author/title (mirrors `findMangaByCriteria`).
    pub async fn search_manga(
        &self,
        query: Option<&str>,
        author: Option<&str>,
        title: Option<&str>,
        page_num: usize,
    ) -> Result<Page<MangaAcqEntry>, sqlx::Error> {
        let mut conds: Vec<String> = Vec::new();
        if let Some(q) = query.filter(|s| !s.is_empty()) {
            let q = q.replace('\'', "''");
            conds.push(format!("(m.title ILIKE '%{q}%' OR m.author ILIKE '%{q}%' OR m.artist ILIKE '%{q}%')"));
        }
        if let Some(a) = author.filter(|s| !s.is_empty()) {
            let a = a.replace('\'', "''");
            conds.push(format!("(m.author ILIKE '%{a}%' OR m.artist ILIKE '%{a}%')"));
        }
        if let Some(t) = title.filter(|s| !s.is_empty()) {
            let t = t.replace('\'', "''");
            conds.push(format!("m.title ILIKE '%{t}%'"));
        }
        let base = if conds.is_empty() {
            "WHERE m.in_library = TRUE".to_string()
        } else {
            format!("WHERE m.in_library = TRUE AND {}", conds.join(" AND "))
        };
        let offset = (page_num.saturating_sub(1)) * ITEMS_PER_PAGE;
        let sql = format!("{MANGA_SELECT} {base} ORDER BY m.title ASC LIMIT {ITEMS_PER_PAGE} OFFSET {offset}");
        let count_sql = format!("SELECT COUNT(*) FROM manga m JOIN source s ON s.id = m.source {base}");
        let total: i64 = sqlx::query_scalar(&count_sql).fetch_one(self.pool).await?;
        let rows: Vec<MangaJoinedRow> = sqlx::query_as(&sql).fetch_all(self.pool).await?;
        let items = self.attach_total_chapters(rows).await?;
        Ok(Page { items, total })
    }

    async fn attach_total_chapters(&self, rows: Vec<MangaJoinedRow>) -> Result<Vec<MangaAcqEntry>, sqlx::Error> {
        let mut items: Vec<MangaAcqEntry> = Vec::with_capacity(rows.len());
        for r in rows {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chapter WHERE manga = $1")
                .bind(r.id)
                .fetch_one(self.pool)
                .await?;
            items.push(Self::to_acq(r, count));
        }
        Ok(items)
    }

    /// Manga details + total chapter count.
    pub async fn manga_details(&self, manga_id: i32) -> Result<Option<MangaDetails>, sqlx::Error> {
        let row: Option<MangaJoinedRow> =
            sqlx::query_as(&format!("{MANGA_SELECT} WHERE m.id = $1")).bind(manga_id).fetch_optional(self.pool).await?;
        let Some(r) = row else { return Ok(None) };
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chapter WHERE manga = $1")
            .bind(manga_id)
            .fetch_one(self.pool)
            .await?;
        Ok(Some(MangaDetails {
            id: r.id,
            title: r.title,
            author: r.author,
            thumbnail_url: r.thumbnail_url,
            total_chapters: count,
        }))
    }

    /// Recently read chapters (history feed).
    pub async fn history(&self, page_num: usize) -> Result<Page<ChapterListEntry>, sqlx::Error> {
        self.chapter_page(
            "WHERE m.in_library = TRUE AND c.last_read_at > 0",
            "ORDER BY c.last_read_at DESC, c.id DESC",
            page_num,
        )
        .await
    }

    /// Recent chapter updates for library manga (library-updates feed).
    pub async fn library_updates(&self, page_num: usize) -> Result<Page<ChapterListEntry>, sqlx::Error> {
        self.chapter_page("WHERE m.in_library = TRUE", "ORDER BY c.date_upload DESC, c.id DESC", page_num).await
    }

    async fn chapter_page(
        &self,
        where_clause: &str,
        order_clause: &str,
        page_num: usize,
    ) -> Result<Page<ChapterListEntry>, sqlx::Error> {
        let offset = (page_num.saturating_sub(1)) * ITEMS_PER_PAGE;
        let sql = format!(
            "SELECT c.id, c.name, c.date_upload, c.chapter_number, c.scanlator, c.last_page_read, c.last_read_at, \
             c.source_order, c.is_downloaded, c.page_count, m.id AS manga_id, m.title AS manga_title, \
             m.author AS manga_author, m.thumbnail_url AS manga_thumbnail_url, \
             (SELECT COUNT(*) FROM chapter cc WHERE cc.manga = m.id) AS manga_total_chapters \
             FROM chapter c JOIN manga m ON m.id = c.manga {where_clause} {order_clause} LIMIT {ITEMS_PER_PAGE} OFFSET {offset}"
        );
        // NOTE: the count query must NOT carry ORDER BY (aggregate error);
        // errors terminate the embedded PGlite session.
        let count_sql = format!("SELECT COUNT(*) FROM chapter c JOIN manga m ON m.id = c.manga {where_clause}");
        let total: i64 = sqlx::query_scalar(&count_sql).fetch_one(self.pool).await?;
        let rows: Vec<ChapterJoinedRow> = sqlx::query_as(&sql).fetch_all(self.pool).await?;
        let items = rows
            .into_iter()
            .map(|r| ChapterListEntry {
                id: r.id,
                name: r.name,
                chapter_number: r.chapter_number,
                source_order: r.source_order,
                scanlator: r.scanlator,
                last_page_read: r.last_page_read,
                last_read_at: r.last_read_at,
                page_count: r.page_count,
                downloaded: r.is_downloaded,
                upload_date: r.date_upload,
                manga_id: r.manga_id,
                manga_title: r.manga_title,
                manga_author: r.manga_author,
                manga_thumbnail_url: r.manga_thumbnail_url,
                manga_total_chapters: r.manga_total_chapters,
            })
            .collect();
        Ok(Page { items, total })
    }

    /// Chapters for one manga with sort/filter/pagination.
    pub async fn chapters_for_manga(
        &self,
        manga_id: i32,
        sort: &str,
        filter: &str,
        page_num: usize,
    ) -> Result<Page<ChapterListEntry>, sqlx::Error> {
        let (order_col, direction) = match sort {
            "date_asc" => ("date_upload", "ASC"),
            "date_desc" => ("date_upload", "DESC"),
            "number_desc" | "desc" => ("source_order", "DESC"),
            _ => ("source_order", "ASC"),
        };
        let filter_clause = match filter {
            "unread" => "AND c.read = FALSE",
            _ => "",
        };
        let where_clause = format!("WHERE c.manga = {manga_id} {filter_clause}");
        let order_clause = format!("ORDER BY c.{order_col} {direction}, c.id ASC");
        self.chapter_page(&where_clause, &order_clause, page_num).await
    }

    /// Chapter metadata for the details feed (by source order).
    #[allow(clippy::type_complexity)]
    pub async fn chapter_metadata(
        &self,
        manga_id: i32,
        source_order: i32,
    ) -> Result<Option<ChapterMetadataEntry>, sqlx::Error> {
        let row: Option<(i32, String, f32, i32, Option<String>, i32, i64, i32, bool, bool, bool, i64)> = sqlx::query_as(
            "SELECT id, name, chapter_number, source_order, scanlator, last_page_read, last_read_at, page_count, \
             is_downloaded, read, bookmark, date_upload FROM chapter WHERE manga = $1 AND source_order = $2",
        )
        .bind(manga_id)
        .bind(source_order)
        .fetch_optional(self.pool)
        .await?;
        Ok(row.map(|(id, name, chapter_number, so, scanlator, lpr, lra, pc, dl, rd, bm, du)| ChapterMetadataEntry {
            id,
            name,
            chapter_number,
            source_order: so,
            scanlator,
            last_page_read: lpr,
            last_read_at: lra,
            page_count: pc,
            downloaded: dl,
            read: rd,
            bookmark: bm,
            upload_date: du,
        }))
    }

    /// Category navigation entries with manga counts.
    pub async fn categories(&self) -> Result<Vec<NavEntry>, sqlx::Error> {
        let rows: Vec<(i32, String, i64)> = sqlx::query_as(
            "SELECT c.id, c.name, (SELECT COUNT(*) FROM category_manga cm WHERE cm.category = c.id) \
             FROM category c ORDER BY c.sort_order, c.id",
        )
        .fetch_all(self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, cnt)| NavEntry { id: id.to_string(), title: name, manga_count: Some(cnt as usize), description: None })
            .collect())
    }

    /// Genre navigation entries (from library manga genre lists).
    pub async fn genres(&self) -> Result<Vec<NavEntry>, sqlx::Error> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT g, COUNT(*) FROM (SELECT unnest(string_to_array(NULLIF(genre, ''), ',')) AS g \
             FROM manga WHERE in_library = TRUE AND genre IS NOT NULL AND genre <> '') t \
             GROUP BY g ORDER BY g",
        )
        .fetch_all(self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(g, cnt)| NavEntry { id: g.trim().to_string(), title: g.trim().to_string(), manga_count: Some(cnt as usize), description: None })
            .collect())
    }

    /// Status navigation entries (MangaStatus enum, counts from library).
    pub async fn statuses(&self) -> Result<Vec<NavEntry>, sqlx::Error> {
        let rows: Vec<(i32, i64)> =
            sqlx::query_as("SELECT status, COUNT(*) FROM manga WHERE in_library = TRUE GROUP BY status").fetch_all(self.pool).await?;
        let counts: std::collections::HashMap<i32, usize> = rows.into_iter().map(|(s, c)| (s, c as usize)).collect();
        let defs: &[(i32, &str)] = &[
            (0, "Unknown"),
            (1, "Ongoing"),
            (2, "Completed"),
            (3, "Licensed"),
            (4, "Publishing Finished"),
            (5, "Cancelled"),
            (6, "On Hiatus"),
        ];
        Ok(defs
            .iter()
            .map(|(id, title)| NavEntry { id: id.to_string(), title: title.to_string(), manga_count: counts.get(id).copied(), description: None })
            .collect())
    }

    /// Content language navigation entries (library).
    pub async fn languages(&self) -> Result<Vec<NavEntry>, sqlx::Error> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT s.lang, COUNT(*) FROM manga m JOIN source s ON s.id = m.source \
             WHERE m.in_library = TRUE GROUP BY s.lang ORDER BY s.lang",
        )
        .fetch_all(self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(lang, cnt)| NavEntry { id: lang.clone(), title: display_language(&lang), manga_count: Some(cnt as usize), description: None })
            .collect())
    }

    /// Sources with series in the library.
    pub async fn library_sources(&self) -> Result<Vec<NavEntry>, sqlx::Error> {
        let rows: Vec<(i64, String, i64)> = sqlx::query_as(
            "SELECT s.id, s.name, COUNT(m.id) FROM source s JOIN manga m ON m.source = s.id \
             WHERE m.in_library = TRUE GROUP BY s.id, s.name ORDER BY s.name",
        )
        .fetch_all(self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, cnt)| NavEntry { id: id.to_string(), title: name, manga_count: Some(cnt as usize), description: None })
            .collect())
    }

    /// All installed sources (explore feed).
    pub async fn explore_sources(&self) -> Result<Vec<NavEntry>, sqlx::Error> {
        let rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM source ORDER BY name").fetch_all(self.pool).await?;
        Ok(rows.into_iter().map(|(id, name)| NavEntry { id: id.to_string(), title: name, manga_count: None, description: None }).collect())
    }

    /// Source display name for a source id.
    pub async fn source_name(&self, source_id: i64) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar("SELECT name FROM source WHERE id = $1").bind(source_id).fetch_optional(self.pool).await
    }
}

/// Best-effort language display name (falls back to the code itself).
pub fn display_language(lang: &str) -> String {
    let name = match lang {
        "en" => "English",
        "ja" | "jp" => "Japanese",
        "zh" => "Chinese",
        "ko" => "Korean",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "pt" => "Portuguese",
        "it" => "Italian",
        "ru" => "Russian",
        "ar" => "Arabic",
        "th" => "Thai",
        "vi" => "Vietnamese",
        "id" => "Indonesian",
        _ => lang,
    };
    name.to_string()
}
