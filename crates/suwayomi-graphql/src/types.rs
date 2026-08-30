//! GraphQL object types — field names must match
//! `docs/graphql/schema-baseline.graphql` exactly.

use async_graphql::{Context, Enum, Object, SimpleObject};
use suwayomi_core::db::Db;
use suwayomi_core::models::{now_epoch_secs, IncludeOrExclude as DomainInclude, MangaStatus as DomainStatus, UpdateStrategy as DomainStrategy};
use suwayomi_core::schema::{CategoryRow, ChapterRow, MangaRow};

use crate::scalars::{Cursor, LongString};
use crate::state::GraphQLState;
use suwayomi_domain::sql::bind_placeholders;

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum MangaStatus {
    Unknown,
    Ongoing,
    Completed,
    Licensed,
    PublishingFinished,
    Cancelled,
    OnHiatus,
}

impl From<DomainStatus> for MangaStatus {
    fn from(s: DomainStatus) -> Self {
        match s {
            DomainStatus::Unknown => Self::Unknown,
            DomainStatus::Ongoing => Self::Ongoing,
            DomainStatus::Completed => Self::Completed,
            DomainStatus::Licensed => Self::Licensed,
            DomainStatus::PublishingFinished => Self::PublishingFinished,
            DomainStatus::Cancelled => Self::Cancelled,
            DomainStatus::OnHiatus => Self::OnHiatus,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum UpdateStrategy {
    AlwaysUpdate,
    OnlyFetchOnce,
}

impl From<DomainStrategy> for UpdateStrategy {
    fn from(s: DomainStrategy) -> Self {
        match s {
            DomainStrategy::AlwaysUpdate => Self::AlwaysUpdate,
            DomainStrategy::OnlyFetchOnce => Self::OnlyFetchOnce,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum IncludeOrExclude {
    Exclude,
    Include,
    Unset,
}

impl From<DomainInclude> for IncludeOrExclude {
    fn from(s: DomainInclude) -> Self {
        match s {
            DomainInclude::Exclude => Self::Exclude,
            DomainInclude::Include => Self::Include,
            DomainInclude::Unset => Self::Unset,
        }
    }
}

/// MetaType — mirrors `graphql/types/MetaType.kt`.
#[derive(SimpleObject, Clone)]
pub struct MetaType {
    pub key: String,
    pub value: String,
}

impl From<(String, String)> for MetaType {
    fn from((key, value): (String, String)) -> Self {
        Self { key, value }
    }
}

/// MangaType — mirrors `graphql/types/MangaType.kt`.
#[derive(Clone)]
pub struct MangaType {
    pub id: i32,
    pub source_id: i64,
    pub url: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub thumbnail_url_last_fetched: i64,
    pub initialized: bool,
    pub artist: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub genre: Vec<String>,
    pub status: MangaStatus,
    pub in_library: bool,
    pub in_library_at: i64,
    pub update_strategy: UpdateStrategy,
    pub real_url: Option<String>,
    pub last_fetched_at: Option<i64>,
    pub chapters_last_fetched_at: Option<i64>,
}

impl MangaType {
    pub fn from_row(row: &MangaRow) -> Self {
        Self {
            id: row.id,
            source_id: row.source,
            url: row.url.clone(),
            title: row.title.clone(),
            thumbnail_url: row.thumbnail_url.clone(),
            thumbnail_url_last_fetched: row.thumbnail_url_last_fetched,
            initialized: row.initialized,
            artist: row.artist.clone(),
            author: row.author.clone(),
            description: row.description.clone(),
            genre: row
                .genre
                .as_deref()
                .map(|g| g.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default(),
            status: DomainStatus::from_i32(row.status).into(),
            in_library: row.in_library,
            in_library_at: row.in_library_at,
            update_strategy: DomainStrategy::from_db(&row.update_strategy).into(),
            real_url: row.real_url.clone(),
            last_fetched_at: Some(row.last_fetched_at),
            chapters_last_fetched_at: Some(row.chapters_last_fetched_at),
        }
    }

    async fn chapters_of(&self, db: &Db) -> Vec<ChapterRow> {
        let sql = bind_placeholders("SELECT * FROM chapter WHERE manga = ? ORDER BY source_order DESC");
        let pool = db.pool();
        sqlx::query_as::<_, ChapterRow>(&sql)
            .bind(self.id)
            .fetch_all(pool)
            .await
            .unwrap_or_default()
    }
}

#[Object]
impl MangaType {
    async fn id(&self) -> i32 {
        self.id
    }
    async fn source_id(&self) -> LongString {
        LongString(self.source_id)
    }
    async fn url(&self) -> &str {
        &self.url
    }
    async fn title(&self) -> &str {
        &self.title
    }
    async fn thumbnail_url(&self) -> Option<&str> {
        self.thumbnail_url.as_deref()
    }
    async fn thumbnail_url_last_fetched(&self) -> LongString {
        LongString(self.thumbnail_url_last_fetched)
    }
    async fn initialized(&self) -> bool {
        self.initialized
    }
    async fn artist(&self) -> Option<&str> {
        self.artist.as_deref()
    }
    async fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }
    async fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
    async fn genre(&self) -> &[String] {
        &self.genre
    }
    async fn status(&self) -> MangaStatus {
        self.status
    }
    async fn in_library(&self) -> bool {
        self.in_library
    }
    async fn in_library_at(&self) -> LongString {
        LongString(self.in_library_at)
    }
    async fn update_strategy(&self) -> UpdateStrategy {
        self.update_strategy
    }
    async fn real_url(&self) -> Option<&str> {
        self.real_url.as_deref()
    }
    async fn last_fetched_at(&self) -> Option<LongString> {
        self.last_fetched_at.map(LongString)
    }
    async fn chapters_last_fetched_at(&self) -> Option<LongString> {
        self.chapters_last_fetched_at.map(LongString)
    }
    async fn age(&self) -> Option<LongString> {
        self.last_fetched_at.map(|t| LongString(now_epoch_secs() - t))
    }
    async fn chapters_age(&self) -> Option<LongString> {
        self.chapters_last_fetched_at.map(|t| LongString(now_epoch_secs() - t))
    }

    async fn unread_count(&self, ctx: &Context<'_>) -> async_graphql::Result<i32> {
        let db = &ctx.data::<GraphQLState>()?.db;
        Ok(self.chapters_of(db).await.iter().filter(|c| !c.read).count() as i32)
    }
    async fn download_count(&self, ctx: &Context<'_>) -> async_graphql::Result<i32> {
        let db = &ctx.data::<GraphQLState>()?.db;
        Ok(self.chapters_of(db).await.iter().filter(|c| c.is_downloaded).count() as i32)
    }
    async fn bookmark_count(&self, ctx: &Context<'_>) -> async_graphql::Result<i32> {
        let db = &ctx.data::<GraphQLState>()?.db;
        Ok(self.chapters_of(db).await.iter().filter(|c| c.bookmark).count() as i32)
    }
    async fn has_duplicate_chapters(&self, ctx: &Context<'_>) -> async_graphql::Result<bool> {
        let chapters = self.chapters_of(&ctx.data::<GraphQLState>()?.db).await;
        let mut seen = std::collections::HashSet::new();
        Ok(chapters.iter().any(|c| !seen.insert((c.url.clone(), c.chapter_number.to_bits()))))
    }

    async fn chapters(&self, ctx: &Context<'_>) -> async_graphql::Result<ChapterNodeList> {
        let chapters = self.chapters_of(&ctx.data::<GraphQLState>()?.db).await;
        let nodes: Vec<ChapterType> = chapters.iter().map(ChapterType::from_row).collect();
        Ok(ChapterNodeList::from_nodes(nodes))
    }

    async fn categories(&self, ctx: &Context<'_>) -> async_graphql::Result<CategoryNodeList> {
        let state = ctx.data::<GraphQLState>()?;
        let list = state
            .category_manga
            .get_manga_categories(self.id)
            .await
            .map_err(async_graphql::Error::from)?;
        let nodes: Vec<CategoryType> = list
            .iter()
            .map(|c| CategoryType {
                id: c.id,
                order: c.order,
                name: c.name.clone(),
                default: c.default,
                include_in_update: c.include_in_update.into(),
                include_in_download: c.include_in_download.into(),
            })
            .collect();
        Ok(CategoryNodeList::from_nodes(nodes))
    }

    async fn meta(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<MetaType>> {
        let state = ctx.data::<GraphQLState>()?;
        let map = state
            .manga
            .get_meta_map(self.id)
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(map.into_iter().map(MetaType::from).collect())
    }

    async fn source(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<SourceType>> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM source WHERE id = ?");
        let row = sqlx::query_as::<_, suwayomi_core::schema::SourceRow>(&sql)
            .bind(self.source_id)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(row.map(|r| SourceType { id: r.id.to_string(), name: r.name, lang: r.lang }))
    }

    async fn track_records(&self) -> TrackRecordNodeList {
        TrackRecordNodeList::empty()
    }

    async fn last_read_chapter(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<ChapterType>> {
        Ok(self
            .chapters_of(&ctx.data::<GraphQLState>()?.db)
            .await
            .into_iter()
            .filter(|c| c.read)
            .max_by_key(|c| c.source_order)
            .map(|c| ChapterType::from_row(&c)))
    }
    async fn latest_read_chapter(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<ChapterType>> {
        Ok(self
            .chapters_of(&ctx.data::<GraphQLState>()?.db)
            .await
            .into_iter()
            .filter(|c| c.read && c.last_read_at > 0)
            .max_by_key(|c| c.last_read_at)
            .map(|c| ChapterType::from_row(&c)))
    }
    async fn first_unread_chapter(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<ChapterType>> {
        let mut chapters = self.chapters_of(&ctx.data::<GraphQLState>()?.db).await;
        chapters.sort_by_key(|c| std::cmp::Reverse(c.source_order));
        Ok(chapters.into_iter().find(|c| !c.read).map(|c| ChapterType::from_row(&c)))
    }
    async fn highest_numbered_chapter(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<ChapterType>> {
        Ok(self
            .chapters_of(&ctx.data::<GraphQLState>()?.db)
            .await
            .into_iter()
            .max_by(|a, b| a.chapter_number.partial_cmp(&b.chapter_number).unwrap_or(std::cmp::Ordering::Equal))
            .map(|c| ChapterType::from_row(&c)))
    }
    async fn latest_fetched_chapter(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<ChapterType>> {
        Ok(self
            .chapters_of(&ctx.data::<GraphQLState>()?.db)
            .await
            .into_iter()
            .max_by_key(|c| c.fetched_at)
            .map(|c| ChapterType::from_row(&c)))
    }
    async fn latest_uploaded_chapter(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<ChapterType>> {
        Ok(self
            .chapters_of(&ctx.data::<GraphQLState>()?.db)
            .await
            .into_iter()
            .max_by_key(|c| c.date_upload)
            .map(|c| ChapterType::from_row(&c)))
    }
}

/// ChapterType — mirrors `graphql/types/ChapterType.kt`.
#[derive(Clone)]
pub struct ChapterType {
    pub id: i32,
    pub url: String,
    pub name: String,
    pub upload_date: i64,
    pub chapter_number: f32,
    pub scanlator: Option<String>,
    pub manga_id: i32,
    pub read: bool,
    pub bookmarked: bool,
    pub last_page_read: i32,
    pub last_read_at: i64,
    pub source_order: i32,
    pub fetched_at: i64,
    pub real_url: Option<String>,
    pub downloaded: bool,
    pub page_count: i32,
}

impl ChapterType {
    pub fn from_row(row: &ChapterRow) -> Self {
        Self {
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
            source_order: row.source_order,
            fetched_at: row.fetched_at,
            real_url: row.real_url.clone(),
            downloaded: row.is_downloaded,
            page_count: row.page_count,
        }
    }
}

#[Object]
impl ChapterType {
    async fn id(&self) -> i32 {
        self.id
    }
    async fn url(&self) -> &str {
        &self.url
    }
    async fn name(&self) -> &str {
        &self.name
    }
    async fn upload_date(&self) -> LongString {
        LongString(self.upload_date)
    }
    async fn chapter_number(&self) -> f32 {
        self.chapter_number
    }
    async fn scanlator(&self) -> Option<&str> {
        self.scanlator.as_deref()
    }
    async fn manga_id(&self) -> i32 {
        self.manga_id
    }
    async fn is_read(&self) -> bool {
        self.read
    }
    async fn is_bookmarked(&self) -> bool {
        self.bookmarked
    }
    async fn last_page_read(&self) -> i32 {
        self.last_page_read
    }
    async fn last_read_at(&self) -> LongString {
        LongString(self.last_read_at)
    }
    async fn source_order(&self) -> i32 {
        self.source_order
    }
    async fn fetched_at(&self) -> LongString {
        LongString(self.fetched_at)
    }
    async fn real_url(&self) -> Option<&str> {
        self.real_url.as_deref()
    }
    async fn is_downloaded(&self) -> bool {
        self.downloaded
    }
    async fn page_count(&self) -> i32 {
        self.page_count
    }
    async fn manga(&self, ctx: &Context<'_>) -> async_graphql::Result<MangaType> {
        let state = ctx.data::<GraphQLState>()?;
        Ok(MangaType::from_row(&fetch_manga_row(state, self.manga_id).await?))
    }
    async fn meta(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<MetaType>> {
        let state = ctx.data::<GraphQLState>()?;
        let map = state
            .chapter
            .get_meta_map(self.id)
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(map.into_iter().map(MetaType::from).collect())
    }
}

async fn fetch_manga_row(state: &GraphQLState, id: i32) -> async_graphql::Result<MangaRow> {
    let sql = bind_placeholders("SELECT * FROM manga WHERE id = ?");
    sqlx::query_as::<_, MangaRow>(&sql)
        .bind(id)
        .fetch_one(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)
}

/// CategoryType — mirrors `graphql/types/CategoryType.kt`.
#[derive(Clone)]
pub struct CategoryType {
    pub id: i32,
    pub order: i32,
    pub name: String,
    pub default: bool,
    pub include_in_update: IncludeOrExclude,
    pub include_in_download: IncludeOrExclude,
}

impl From<&CategoryRow> for CategoryType {
    fn from(r: &CategoryRow) -> Self {
        Self {
            id: r.id,
            order: r.sort_order,
            name: r.name.clone(),
            default: r.is_default,
            include_in_update: DomainInclude::from_i32(r.include_in_update).into(),
            include_in_download: DomainInclude::from_i32(r.include_in_download).into(),
        }
    }
}

#[Object]
impl CategoryType {
    async fn id(&self) -> i32 {
        self.id
    }
    async fn order(&self) -> i32 {
        self.order
    }
    async fn name(&self) -> &str {
        &self.name
    }
    async fn default(&self) -> bool {
        self.default
    }
    async fn include_in_update(&self) -> IncludeOrExclude {
        self.include_in_update
    }
    async fn include_in_download(&self) -> IncludeOrExclude {
        self.include_in_download
    }
    async fn mangas(&self, ctx: &Context<'_>) -> async_graphql::Result<MangaNodeList> {
        let state = ctx.data::<GraphQLState>()?;
        let list = state
            .category_manga
            .get_category_manga_list(self.id)
            .await
            .map_err(async_graphql::Error::from)?;
        let mut nodes = Vec::new();
        for dc in list {
            let sql = bind_placeholders("SELECT * FROM manga WHERE id = ?");
            let row = sqlx::query_as::<_, MangaRow>(&sql)
                .bind(dc.id)
                .fetch_one(state.db.pool())
                .await
                .map_err(async_graphql::Error::from)?;
            nodes.push(MangaType::from_row(&row));
        }
        Ok(MangaNodeList::from_nodes(nodes))
    }
    async fn meta(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<MetaType>> {
        let state = ctx.data::<GraphQLState>()?;
        let map = state
            .category
            .get_meta_map(self.id)
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(map.into_iter().map(MetaType::from).collect())
    }
}

/// PageType — mirrors `graphql/types/PageType.kt`.
#[derive(SimpleObject, Clone)]
pub struct PageType {
    pub index: i32,
    pub url: String,
    pub image_url: Option<String>,
}

/// SourceType — mirrors `graphql/types/SourceType.kt` (minimal).
#[derive(SimpleObject, Clone)]
pub struct SourceType {
    pub id: String,
    pub name: String,
    pub lang: String,
}

// ---- pagination (NodeList) ----

#[derive(SimpleObject, Clone)]
pub struct PageInfo {
    pub start_cursor: Option<Cursor>,
    pub end_cursor: Option<Cursor>,
    pub has_next_page: bool,
    pub has_previous_page: bool,
}

#[derive(SimpleObject, Clone)]
pub struct MangaEdge {
    pub cursor: Cursor,
    pub node: MangaType,
}

#[derive(SimpleObject, Clone)]
pub struct MangaNodeList {
    pub nodes: Vec<MangaType>,
    pub edges: Vec<MangaEdge>,
    pub page_info: PageInfo,
    pub total_count: i32,
}

impl MangaNodeList {
    pub fn from_nodes(nodes: Vec<MangaType>) -> Self {
        let total = nodes.len() as i32;
        // Kotlin getEdges: only first & last edges
        let edges = if nodes.is_empty() {
            vec![]
        } else if nodes.len() == 1 {
            vec![MangaEdge { cursor: Cursor("0".into()), node: nodes[0].clone() }]
        } else {
            vec![
                MangaEdge { cursor: Cursor("0".into()), node: nodes[0].clone() },
                MangaEdge { cursor: Cursor((nodes.len() - 1).to_string()), node: nodes[nodes.len() - 1].clone() },
            ]
        };
        Self {
            page_info: PageInfo {
                start_cursor: Some(Cursor("0".into())),
                end_cursor: Some(Cursor((total.saturating_sub(1)).to_string())),
                has_next_page: false,
                has_previous_page: false,
            },
            nodes,
            edges,
            total_count: total,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct ChapterNodeList {
    pub nodes: Vec<ChapterType>,
    pub edges: Vec<ChapterEdge>,
    pub page_info: PageInfo,
    pub total_count: i32,
}

#[derive(SimpleObject, Clone)]
pub struct ChapterEdge {
    pub cursor: Cursor,
    pub node: ChapterType,
}

impl ChapterNodeList {
    pub fn from_nodes(nodes: Vec<ChapterType>) -> Self {
        let total = nodes.len() as i32;
        let edges = if nodes.is_empty() {
            vec![]
        } else if nodes.len() == 1 {
            vec![ChapterEdge { cursor: Cursor("0".into()), node: nodes[0].clone() }]
        } else {
            vec![
                ChapterEdge { cursor: Cursor("0".into()), node: nodes[0].clone() },
                ChapterEdge { cursor: Cursor((nodes.len() - 1).to_string()), node: nodes[nodes.len() - 1].clone() },
            ]
        };
        Self {
            page_info: PageInfo {
                start_cursor: Some(Cursor("0".into())),
                end_cursor: Some(Cursor((total.saturating_sub(1)).to_string())),
                has_next_page: false,
                has_previous_page: false,
            },
            nodes,
            edges,
            total_count: total,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct CategoryNodeList {
    pub nodes: Vec<CategoryType>,
    pub edges: Vec<CategoryEdge>,
    pub page_info: PageInfo,
    pub total_count: i32,
}

#[derive(SimpleObject, Clone)]
pub struct CategoryEdge {
    pub cursor: Cursor,
    pub node: CategoryType,
}

impl CategoryNodeList {
    pub fn from_nodes(nodes: Vec<CategoryType>) -> Self {
        let total = nodes.len() as i32;
        let edges = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| CategoryEdge { cursor: Cursor(i.to_string()), node: n.clone() })
            .collect();
        Self {
            page_info: PageInfo {
                start_cursor: Some(Cursor("0".into())),
                end_cursor: Some(Cursor(total.saturating_sub(1).to_string())),
                has_next_page: false,
                has_previous_page: false,
            },
            nodes,
            edges,
            total_count: total,
        }
    }
}

/// TrackRecordNodeList — minimal (Phase 6 fills records).
#[derive(SimpleObject, Clone)]
pub struct TrackRecordNodeList {
    pub nodes: Vec<TrackRecordType>,
    pub edges: Vec<TrackRecordEdge>,
    pub page_info: PageInfo,
    pub total_count: i32,
}

#[derive(SimpleObject, Clone)]
pub struct TrackRecordType {
    pub id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct TrackRecordEdge {
    pub cursor: Cursor,
    pub node: TrackRecordType,
}

impl TrackRecordNodeList {
    pub fn empty() -> Self {
        Self {
            nodes: vec![],
            edges: vec![],
            page_info: PageInfo {
                start_cursor: None,
                end_cursor: None,
                has_next_page: false,
                has_previous_page: false,
            },
            total_count: 0,
        }
    }
}
