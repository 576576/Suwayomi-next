//! GraphQL object types — field names must match
//! `docs/graphql/schema-baseline.graphql` exactly.

use async_graphql::{Context, Enum, Interface, Object, SimpleObject, Union};
use suwayomi_core::db::Db;
use suwayomi_core::models::{
    now_epoch_secs, IncludeOrExclude as DomainInclude, MangaStatus as DomainStatus, UpdateStrategy as DomainStrategy,
};
use suwayomi_core::schema::{CategoryRow, ChapterRow, MangaRow};

use crate::scalars::{Cursor, LongString};
use base64::Engine;
use crate::state::GraphQLState;
use crate::track::TrackRecordNodeList;
use sqlx::Row;
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

/// Mirrors `ContentWarning` enum (source/extension content rating).
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum ContentWarning {
    Safe,
    Mixed,
    Nsfw,
}

impl ContentWarning {
    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => Self::Mixed,
            2 => Self::Nsfw,
            _ => Self::Safe,
        }
    }

    pub fn to_i32(self) -> i32 {
        match self {
            Self::Safe => 0,
            Self::Mixed => 1,
            Self::Nsfw => 2,
        }
    }
}

/// MetaType — mirrors `graphql/types/MetaType.kt` (interface).
/// Implemented by GlobalMetaType / MangaMetaType / ChapterMetaType /
/// CategoryMetaType / SourceMetaType.
#[derive(Interface, Clone)]
#[allow(clippy::duplicated_attributes)] // two distinct interface fields, clippy false positive
#[graphql(field(name = "key", ty = "String"), field(name = "value", ty = "String"))]
pub enum MetaType {
    Global(GlobalMetaType),
    Manga(MangaMetaType),
    Chapter(ChapterMetaType),
    Category(CategoryMetaType),
    Source(SourceMetaType),
}

/// Mirrors `GlobalMetaType.kt`.
#[derive(SimpleObject, Clone)]
pub struct GlobalMetaType {
    pub key: String,
    pub value: String,
}

impl From<(String, String)> for GlobalMetaType {
    fn from((key, value): (String, String)) -> Self {
        Self { key, value }
    }
}

/// Mirrors `MangaMetaType.kt` (meta: `[MangaMetaType!]!` on MangaType).
#[derive(Clone)]
pub struct MangaMetaType {
    pub key: String,
    pub value: String,
    pub manga_id: i32,
}

#[Object]
impl MangaMetaType {
    async fn key(&self) -> &str {
        &self.key
    }
    async fn value(&self) -> &str {
        &self.value
    }
    async fn manga_id(&self) -> i32 {
        self.manga_id
    }
    async fn manga(&self, ctx: &Context<'_>) -> async_graphql::Result<MangaType> {
        let state = ctx.data::<GraphQLState>()?;
        Ok(MangaType::from_row(&fetch_manga_row(state, self.manga_id).await?))
    }
}

/// Mirrors `ChapterMetaType.kt` (meta: `[ChapterMetaType!]!` on ChapterType).
#[derive(Clone)]
pub struct ChapterMetaType {
    pub key: String,
    pub value: String,
    pub chapter_id: i32,
}

#[Object]
impl ChapterMetaType {
    async fn key(&self) -> &str {
        &self.key
    }
    async fn value(&self) -> &str {
        &self.value
    }
    async fn chapter_id(&self) -> i32 {
        self.chapter_id
    }
}

/// Mirrors `CategoryMetaType.kt` (meta: `[CategoryMetaType!]!` on CategoryType).
#[derive(Clone)]
pub struct CategoryMetaType {
    pub key: String,
    pub value: String,
    pub category_id: i32,
}

#[Object]
impl CategoryMetaType {
    async fn key(&self) -> &str {
        &self.key
    }
    async fn value(&self) -> &str {
        &self.value
    }
    async fn category_id(&self) -> i32 {
        self.category_id
    }
    async fn category(&self, ctx: &Context<'_>) -> async_graphql::Result<CategoryType> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM category WHERE id = ?");
        let row = sqlx::query_as::<_, CategoryRow>(&sql)
            .bind(self.category_id)
            .fetch_one(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(CategoryType::from(&row))
    }
}

/// Mirrors `SourceMetaType.kt` (meta: `[SourceMetaType!]!` on SourceType).
#[derive(Clone)]
pub struct SourceMetaType {
    pub key: String,
    pub value: String,
    pub source_id: i64,
}

#[Object]
impl SourceMetaType {
    async fn key(&self) -> &str {
        &self.key
    }
    async fn value(&self) -> &str {
        &self.value
    }
    async fn source_id(&self) -> LongString {
        LongString(self.source_id)
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
    /// Alternative titles (other-language titles from archive metadata).
    pub alt_titles: Vec<String>,
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
            // external covers go through the same-origin image proxy (CORS-safe,
            // disk-cached); the DB keeps the raw URL for refreshes.
            thumbnail_url: proxied_cover_url(row.thumbnail_url.clone()),
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
            alt_titles: serde_json::from_str(&row.alt_titles).unwrap_or_default(),
        }
    }

    async fn chapters_of(&self, db: &Db) -> Vec<ChapterRow> {
        let sql = bind_placeholders("SELECT * FROM chapter WHERE manga = ? ORDER BY source_order DESC");
        let pool = db.pool();
        sqlx::query_as::<_, ChapterRow>(&sql).bind(self.id).fetch_all(pool).await.unwrap_or_default()
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
    async fn alt_titles(&self) -> &[String] {
        &self.alt_titles
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
        let state = ctx.data::<GraphQLState>()?;
        // A freshly-browsed manga has no chapter rows yet — pull them from
        // the source so the details page shows the chapter list without a
        // manual "refresh chapters". Idempotent: once rows exist this is a
        // plain DB read.
        if self.chapters_of(&state.db).await.is_empty() {
            let _ = state.chapter.get_chapter_list(self.id, true).await;
        }
        let chapters = self.chapters_of(&state.db).await;
        let nodes: Vec<ChapterType> = chapters.iter().map(ChapterType::from_row).collect();
        Ok(ChapterNodeList::from_nodes(nodes))
    }

    async fn categories(&self, ctx: &Context<'_>) -> async_graphql::Result<CategoryNodeList> {
        let state = ctx.data::<GraphQLState>()?;
        let list = state.category_manga.get_manga_categories(self.id).await.map_err(async_graphql::Error::from)?;
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

    async fn meta(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<MangaMetaType>> {
        let state = ctx.data::<GraphQLState>()?;
        let map = state.manga.get_meta_map(self.id).await.map_err(async_graphql::Error::from)?;
        Ok(map.into_iter().map(|(k, v)| MangaMetaType { key: k, value: v, manga_id: self.id }).collect())
    }

    async fn source(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<SourceType>> {
        let state = ctx.data::<GraphQLState>()?;
        // 本地源漫画的 source_id = LOCAL_SOURCE_ID(0) 指向合成本地源（不在 source 表，
        // 由 sources resolver 注入）——此处直接返回合成条目，避免 WebUI 详情页来源为 null。
        if self.source_id == suwayomi_domain::source::LOCAL_SOURCE_ID {
            return Ok(Some(SourceType::local_source()));
        }
        let sql = bind_placeholders("SELECT * FROM source WHERE id = ?");
        let row = sqlx::query_as::<_, suwayomi_core::schema::SourceRow>(&sql)
            .bind(self.source_id)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(row.map(|r| SourceType::from_row(&r)))
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
    async fn meta(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<ChapterMetaType>> {
        let state = ctx.data::<GraphQLState>()?;
        let map = state.chapter.get_meta_map(self.id).await.map_err(async_graphql::Error::from)?;
        Ok(map.into_iter().map(|(k, v)| ChapterMetaType { key: k, value: v, chapter_id: self.id }).collect())
    }
}

async fn fetch_manga_row(state: &GraphQLState, id: i32) -> async_graphql::Result<MangaRow> {
    let sql = bind_placeholders("SELECT * FROM manga WHERE id = ?");
    sqlx::query_as::<_, MangaRow>(&sql).bind(id).fetch_one(state.db.pool()).await.map_err(async_graphql::Error::from)
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
        let list = state.category_manga.get_category_manga_list(self.id).await.map_err(async_graphql::Error::from)?;
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
    async fn meta(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<CategoryMetaType>> {
        let state = ctx.data::<GraphQLState>()?;
        let map = state.category.get_meta_map(self.id).await.map_err(async_graphql::Error::from)?;
        Ok(map.into_iter().map(|(k, v)| CategoryMetaType { key: k, value: v, category_id: self.id }).collect())
    }
}

/// PageType — mirrors `graphql/types/PageType.kt`.
#[derive(SimpleObject, Clone)]
pub struct PageType {
    pub index: i32,
    pub url: String,
    pub image_url: Option<String>,
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

/// TrackRecordNodeList — full implementation lives in `track.rs`.

// ---- GlobalMeta NodeList (Query.metas) ----

#[derive(SimpleObject, Clone)]
pub struct MetaEdge {
    pub cursor: Cursor,
    pub node: GlobalMetaType,
}

#[derive(SimpleObject, Clone)]
pub struct GlobalMetaNodeList {
    pub nodes: Vec<GlobalMetaType>,
    pub edges: Vec<MetaEdge>,
    pub page_info: PageInfo,
    pub total_count: i32,
}

impl GlobalMetaNodeList {
    pub fn from_nodes(nodes: Vec<GlobalMetaType>) -> Self {
        let total = nodes.len() as i32;
        let edges = if nodes.is_empty() {
            vec![]
        } else if nodes.len() == 1 {
            vec![MetaEdge { cursor: Cursor("0".into()), node: nodes[0].clone() }]
        } else {
            vec![
                MetaEdge { cursor: Cursor("0".into()), node: nodes[0].clone() },
                MetaEdge { cursor: Cursor((nodes.len() - 1).to_string()), node: nodes[nodes.len() - 1].clone() },
            ]
        };
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

// ---------------------------------------------------------------------------
// Source / Extension domain types (batch A2)
// ---------------------------------------------------------------------------

/// Mirrors `SortSelection` (SortFilter.default).
#[derive(SimpleObject, Clone)]
pub struct SortSelection {
    pub ascending: bool,
    pub index: i32,
}

/// Mirrors `TriState` (TriStateFilter.default).
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum TriState {
    Ignore,
    Include,
    Exclude,
}

#[derive(SimpleObject, Clone)]
pub struct CheckBoxFilter {
    pub default: bool,
    pub name: String,
}

#[derive(SimpleObject, Clone)]
pub struct GroupFilter {
    pub filters: Vec<Filter>,
    pub name: String,
}

#[derive(SimpleObject, Clone)]
pub struct HeaderFilter {
    pub name: String,
}

#[derive(SimpleObject, Clone)]
pub struct SelectFilter {
    pub default: i32,
    pub name: String,
    pub values: Vec<String>,
}

#[derive(SimpleObject, Clone)]
pub struct SeparatorFilter {
    pub name: String,
}

#[derive(SimpleObject, Clone)]
pub struct SortFilter {
    pub default: Option<SortSelection>,
    pub name: String,
    pub values: Vec<String>,
}

#[derive(SimpleObject, Clone)]
pub struct TextFilter {
    pub default: String,
    pub name: String,
}

#[derive(SimpleObject, Clone)]
pub struct TriStateFilter {
    pub default: TriState,
    pub name: String,
}

/// Mirrors `union Filter = ...`.
#[derive(Union, Clone)]
pub enum Filter {
    CheckBox(CheckBoxFilter),
    Group(GroupFilter),
    Header(HeaderFilter),
    Select(SelectFilter),
    Separator(SeparatorFilter),
    Sort(SortFilter),
    Text(TextFilter),
    TriState(TriStateFilter),
}

#[derive(SimpleObject, Clone)]
pub struct CheckBoxPreference {
    pub current_value: Option<bool>,
    pub default: bool,
    pub enabled: bool,
    pub key: Option<String>,
    pub summary: Option<String>,
    pub title: Option<String>,
    pub visible: bool,
}

#[derive(SimpleObject, Clone)]
pub struct EditTextPreference {
    pub current_value: Option<String>,
    pub default: Option<String>,
    pub dialog_message: Option<String>,
    pub dialog_title: Option<String>,
    pub enabled: bool,
    pub key: Option<String>,
    pub summary: Option<String>,
    pub text: Option<String>,
    pub title: Option<String>,
    pub visible: bool,
}

#[derive(SimpleObject, Clone)]
pub struct ListPreference {
    pub current_value: Option<String>,
    pub default: Option<String>,
    pub enabled: bool,
    pub entries: Vec<String>,
    pub entry_values: Vec<String>,
    pub key: Option<String>,
    pub summary: Option<String>,
    pub title: Option<String>,
    pub visible: bool,
}

#[derive(SimpleObject, Clone)]
pub struct MultiSelectListPreference {
    pub current_value: Option<Vec<String>>,
    pub default: Option<Vec<String>>,
    pub dialog_message: Option<String>,
    pub dialog_title: Option<String>,
    pub enabled: bool,
    pub entries: Vec<String>,
    pub entry_values: Vec<String>,
    pub key: Option<String>,
    pub summary: Option<String>,
    pub title: Option<String>,
    pub visible: bool,
}

#[derive(SimpleObject, Clone)]
pub struct SwitchPreference {
    pub current_value: Option<bool>,
    pub default: bool,
    pub enabled: bool,
    pub key: Option<String>,
    pub summary: Option<String>,
    pub title: Option<String>,
    pub visible: bool,
}

/// Mirrors `union Preference = ...`.
#[derive(Union, Clone)]
pub enum Preference {
    CheckBox(CheckBoxPreference),
    EditText(EditTextPreference),
    List(ListPreference),
    MultiSelectList(MultiSelectListPreference),
    Switch(SwitchPreference),
}

/// Mirrors `ExtensionType` — built from the `extension` table row.
#[derive(Clone)]
pub struct ExtensionType {
    pub row: suwayomi_core::schema::ExtensionRow,
}

impl ExtensionType {
    pub fn proxy_icon_url(pkg_name: &str) -> String {
        format!("/api/v1/extension/icon/{pkg_name}")
    }
}

#[Object]
impl ExtensionType {
    async fn apk_name(&self) -> Option<&str> {
        self.row.apk_name.as_deref()
    }
    async fn apk_url(&self) -> Option<&str> {
        self.row.apk_url.as_deref()
    }
    async fn content_warning(&self) -> ContentWarning {
        ContentWarning::from_i32(self.row.content_warning)
    }
    async fn extension_lib(&self) -> Option<&str> {
        self.row.extension_lib.as_deref()
    }
    async fn has_update(&self) -> bool {
        self.row.has_update
    }
    async fn icon_url(&self) -> String {
        // 返回服务器代理端点（下载扩展图标并缓存）而非原始远程 URL——
        // WebUI 会拼 baseUrl 前缀，远程完整 URL 会被破坏导致图标加载失败。
        ExtensionType::proxy_icon_url(&self.row.pkg_name)
    }
    async fn is_installed(&self) -> bool {
        self.row.is_installed
    }
    async fn is_nsfw(&self) -> bool {
        self.row.content_warning >= 1
    }
    async fn is_obsolete(&self) -> bool {
        self.row.is_obsolete
    }
    async fn jar_url(&self) -> Option<&str> {
        self.row.jar_url.as_deref()
    }
    async fn lang(&self) -> &str {
        &self.row.lang
    }
    async fn name(&self) -> &str {
        &self.row.name
    }
    async fn pkg_name(&self) -> &str {
        &self.row.pkg_name
    }
    async fn repo(&self) -> Option<&str> {
        self.row.store_index_url.as_deref()
    }
    async fn store_index_url(&self) -> Option<&str> {
        self.row.store_index_url.as_deref()
    }
    async fn version_code(&self) -> i32 {
        self.row.version_code as i32
    }
    async fn version_code_long(&self) -> LongString {
        LongString(self.row.version_code)
    }
    async fn version_name(&self) -> &str {
        &self.row.version_name
    }
    async fn extension_store(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<ExtensionStoreType>> {
        let Some(idx) = &self.row.store_index_url else { return Ok(None) };
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM extension_store WHERE index_url = ?");
        let row = sqlx::query_as::<_, suwayomi_core::schema::ExtensionStoreRow>(&sql)
            .bind(idx)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(row.map(ExtensionStoreType::from_row))
    }
    async fn source(&self, ctx: &Context<'_>) -> async_graphql::Result<SourceNodeList> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM source WHERE extension = ?");
        let rows = sqlx::query_as::<_, suwayomi_core::schema::SourceRow>(&sql)
            .bind(self.row.id)
            .fetch_all(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let nodes: Vec<SourceType> = rows.iter().map(SourceType::from_row).collect();
        Ok(SourceNodeList::from_nodes(nodes))
    }
}

/// Mirrors `ExtensionStoreType`.
#[derive(Clone)]
pub struct ExtensionStoreType {
    pub row: suwayomi_core::schema::ExtensionStoreRow,
}

impl ExtensionStoreType {
    pub fn from_row(row: suwayomi_core::schema::ExtensionStoreRow) -> Self {
        Self { row }
    }
}

#[Object]
impl ExtensionStoreType {
    async fn badge_label(&self) -> &str {
        &self.row.badge_label
    }
    async fn contact_discord(&self) -> Option<&str> {
        self.row.contact_discord.as_deref()
    }
    async fn contact_website(&self) -> &str {
        &self.row.contact_website
    }
    async fn extension_list_url(&self) -> Option<&str> {
        self.row.extension_list_url.as_deref()
    }
    async fn index_url(&self) -> &str {
        &self.row.index_url
    }
    async fn is_legacy(&self) -> bool {
        self.row.is_legacy
    }
    async fn name(&self) -> &str {
        &self.row.name
    }
    async fn signing_key(&self) -> &str {
        &self.row.signing_key
    }
    async fn extensions(&self, ctx: &Context<'_>) -> async_graphql::Result<ExtensionNodeList> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM extension WHERE store_index_url = ?");
        let rows = sqlx::query_as::<_, suwayomi_core::schema::ExtensionRow>(&sql)
            .bind(&self.row.index_url)
            .fetch_all(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let nodes: Vec<ExtensionType> = rows.into_iter().map(|r| ExtensionType { row: r }).collect();
        Ok(ExtensionNodeList::from_nodes(nodes))
    }
}

/// Full SourceType — mirrors `graphql/types/SourceType.kt`.
#[derive(Clone)]
pub struct SourceType {
    pub id: i64,
    pub name: String,
    pub lang: String,
    pub content_warning: i32,
    pub extension_id: i32,
    pub extension_row: Option<suwayomi_core::schema::ExtensionRow>,
    /// Batch-injected by the `sources` resolver (avoids N+1 icon lookups).
    pub icon_pkg_name: Option<String>,
    /// Batch-injected by the `sources` resolver (avoids N+1 meta lookups).
    pub meta_cache: Vec<SourceMetaType>,
}

impl SourceType {
    pub fn from_row(row: &suwayomi_core::schema::SourceRow) -> Self {
        Self {
            id: row.id,
            name: row.name.clone(),
            lang: row.lang.clone(),
            content_warning: row.content_warning,
            extension_id: row.extension,
            extension_row: None,
            icon_pkg_name: None,
            meta_cache: Vec::new(),
        }
    }

    /// Synthetic entry for the local source — id `0` (`Sources.LOCAL_SOURCE_ID`
    /// in the WebUI), grouped under "Other" via `lang = OTHER`.
    pub fn local_source() -> Self {
        use suwayomi_core::schema::ExtensionRow;
        Self {
            id: suwayomi_domain::source::LOCAL_SOURCE_ID,
            name: "Local".to_string(),
            lang: "OTHER".to_string(),
            content_warning: 0,
            extension_id: -1,
            extension_row: Some(ExtensionRow {
                id: -1,
                apk_name: None,
                store_index_url: None,
                icon_url: String::new(),
                name: "Local".to_string(),
                pkg_name: "local".to_string(),
                apk_url: None,
                jar_url: None,
                extension_lib: None,
                version_name: String::new(),
                version_code: 0,
                lang: "OTHER".to_string(),
                content_warning: 0,
                is_installed: true,
                has_update: false,
                is_obsolete: false,
                class_name: String::new(),
            }),
            icon_pkg_name: None,
            meta_cache: Vec::new(),
        }
    }

    pub fn extension_id(&self) -> i32 {
        self.extension_id
    }
}

#[Object]
impl SourceType {
    async fn id(&self) -> LongString {
        LongString(self.id)
    }
    async fn name(&self) -> &str {
        &self.name
    }
    async fn lang(&self) -> &str {
        &self.lang
    }
    async fn content_warning(&self) -> ContentWarning {
        ContentWarning::from_i32(self.content_warning)
    }
    async fn icon_url(&self, ctx: &Context<'_>) -> async_graphql::Result<String> {
        // 本地源（id=0）为合成条目，无扩展图标——WebUI 显示文件夹图标。
        if self.id == suwayomi_domain::source::LOCAL_SOURCE_ID {
            return Ok(String::new());
        }
        // 优先使用 sources resolver 批量注入的 pkg_name（避免 N+1），
        // 未注入时按 extension_id 回退查询。
        if let Some(pkg) = &self.icon_pkg_name {
            return Ok(ExtensionType::proxy_icon_url(pkg));
        }
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT pkg_name FROM extension WHERE id = ?");
        let pkg: Option<String> = sqlx::query_scalar(&sql)
            .bind(self.extension_id)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(pkg.map(|p| ExtensionType::proxy_icon_url(&p)).unwrap_or_default())
    }
    async fn is_nsfw(&self) -> bool {
        self.content_warning >= 1
    }
    async fn supports_latest(&self) -> bool {
        false // extension runtime not loaded yet (Phase 5)
    }
    async fn is_configurable(&self) -> bool {
        false // extension runtime not loaded yet (Phase 5)
    }
    async fn display_name(&self) -> &str {
        &self.name
    }
    async fn home_url(&self) -> Option<String> {
        None // requires HttpSource instance (Phase 5)
    }
    async fn base_url(&self) -> Option<String> {
        None // requires HttpSource instance (Phase 5)
    }
    async fn extension(&self, ctx: &Context<'_>) -> async_graphql::Result<ExtensionType> {
        if let Some(row) = &self.extension_row {
            return Ok(ExtensionType { row: row.clone() });
        }
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM extension WHERE id = ?");
        let row = sqlx::query_as::<_, suwayomi_core::schema::ExtensionRow>(&sql)
            .bind(self.extension_id())
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        row.map(|r| ExtensionType { row: r }).ok_or_else(|| async_graphql::Error::new("extension not found"))
    }
    async fn filters(&self) -> Vec<Filter> {
        vec![]
    }
    async fn manga(&self, ctx: &Context<'_>) -> async_graphql::Result<MangaNodeList> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM manga WHERE source = ? ORDER BY title ASC");
        let rows = sqlx::query_as::<_, MangaRow>(&sql)
            .bind(self.id)
            .fetch_all(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let nodes: Vec<MangaType> = rows.iter().map(MangaType::from_row).collect();
        Ok(MangaNodeList::from_nodes(nodes))
    }
    async fn meta(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<SourceMetaType>> {
        // 优先使用 sources resolver 批量注入的缓存（避免 N+1），空则回退查询。
        if !self.meta_cache.is_empty() {
            return Ok(self.meta_cache.clone());
        }
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT meta_key, value FROM source_meta WHERE source_ref = ?");
        let rows =
            sqlx::query(&sql).bind(self.id).fetch_all(state.db.pool()).await.map_err(async_graphql::Error::from)?;
        Ok(rows
            .iter()
            .map(|r| SourceMetaType {
                key: r.try_get("meta_key").unwrap_or_default(),
                value: r.try_get("value").unwrap_or_default(),
                source_id: self.id,
            })
            .collect())
    }
    async fn preferences(&self) -> Vec<Preference> {
        vec![]
    }
}

// ---- Source / Extension NodeLists ----

#[derive(SimpleObject, Clone)]
pub struct SourceEdge {
    pub cursor: Cursor,
    pub node: SourceType,
}

#[derive(SimpleObject, Clone)]
pub struct SourceNodeList {
    pub nodes: Vec<SourceType>,
    pub edges: Vec<SourceEdge>,
    pub page_info: PageInfo,
    pub total_count: i32,
}

impl SourceNodeList {
    pub fn from_nodes(nodes: Vec<SourceType>) -> Self {
        let total = nodes.len() as i32;
        let edges = if nodes.is_empty() {
            vec![]
        } else if nodes.len() == 1 {
            vec![SourceEdge { cursor: Cursor("0".into()), node: nodes[0].clone() }]
        } else {
            vec![
                SourceEdge { cursor: Cursor("0".into()), node: nodes[0].clone() },
                SourceEdge { cursor: Cursor((nodes.len() - 1).to_string()), node: nodes[nodes.len() - 1].clone() },
            ]
        };
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

#[derive(SimpleObject, Clone)]
pub struct ExtensionEdge {
    pub cursor: Cursor,
    pub node: ExtensionType,
}

#[derive(SimpleObject, Clone)]
pub struct ExtensionNodeList {
    pub nodes: Vec<ExtensionType>,
    pub edges: Vec<ExtensionEdge>,
    pub page_info: PageInfo,
    pub total_count: i32,
}

impl ExtensionNodeList {
    pub fn from_nodes(nodes: Vec<ExtensionType>) -> Self {
        let total = nodes.len() as i32;
        let edges = if nodes.is_empty() {
            vec![]
        } else if nodes.len() == 1 {
            vec![ExtensionEdge { cursor: Cursor("0".into()), node: nodes[0].clone() }]
        } else {
            vec![
                ExtensionEdge { cursor: Cursor("0".into()), node: nodes[0].clone() },
                ExtensionEdge { cursor: Cursor((nodes.len() - 1).to_string()), node: nodes[nodes.len() - 1].clone() },
            ]
        };
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

#[derive(SimpleObject, Clone)]
pub struct ExtensionStoreEdge {
    pub cursor: Cursor,
    pub node: ExtensionStoreType,
}

#[derive(SimpleObject, Clone)]
pub struct ExtensionStoreNodeList {
    pub nodes: Vec<ExtensionStoreType>,
    pub edges: Vec<ExtensionStoreEdge>,
    pub page_info: PageInfo,
    pub total_count: i32,
}

impl ExtensionStoreNodeList {
    pub fn from_nodes(nodes: Vec<ExtensionStoreType>) -> Self {
        let total = nodes.len() as i32;
        let edges = if nodes.is_empty() {
            vec![]
        } else if nodes.len() == 1 {
            vec![ExtensionStoreEdge { cursor: Cursor("0".into()), node: nodes[0].clone() }]
        } else {
            vec![
                ExtensionStoreEdge { cursor: Cursor("0".into()), node: nodes[0].clone() },
                ExtensionStoreEdge {
                    cursor: Cursor((nodes.len() - 1).to_string()),
                    node: nodes[nodes.len() - 1].clone(),
                },
            ]
        };
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

/// Wraps an external cover URL into the server's same-origin image proxy
/// (`/api/v1/image/{b64url}`). The browser then loads covers without CORS
/// failures (extension CDNs usually omit CORS headers and the WebUI sets
/// `crossOrigin='anonymous'`) and repeated requests hit the local disk cache
/// instead of the upstream CDN. Non-http URLs pass through unchanged.
pub fn proxied_cover_url(url: Option<String>) -> Option<String> {
    url.map(|u| {
        if u.starts_with("http://") || u.starts_with("https://") {
            let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(u.as_bytes());
            format!("/api/v1/image/{b64}")
        } else {
            u
        }
    })
}
