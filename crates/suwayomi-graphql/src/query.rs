//! Query root — mirrors `graphql/queries/*.kt`.
//! Core queries implemented; remaining queries land in later increments.

use async_graphql::{Context, Enum, InputObject, Object, SimpleObject};
use sqlx::Row;
use suwayomi_core::schema::{CategoryRow, ChapterRow, MangaRow};
use suwayomi_domain::sql::bind_placeholders;

use crate::mutation_b4::{
    BackupRestoreState, BackupRestoreStatus, DownloadStatus, KoSyncStatusPayloadType, LibraryUpdateStatus,
    ValidateBackupInput, ValidateBackupResult,
};
use crate::scalars::{Cursor, LongString};
use crate::settings::WebUIChannel;
use crate::settings::{AboutServerPayload, SettingsType};
use crate::state::GraphQLState;
use crate::track::{SearchTrackerPayload, TrackRecordNodeList, TrackRecordType, TrackerNodeList, TrackerType};
use crate::types::*;

enum BindVal {
    I32(i32),
    I64(i64),
    Bool(bool),
    F64(f64),
    Str(String),
}

/// Mirrors `MangaCondition` from `MangaQuery.kt` (core filters).
#[derive(InputObject)]
#[graphql(name = "MangaConditionInput")]
pub struct MangaCondition {
    pub id: Option<i32>,
    /// LongString so WebUI source ids (strings, e.g. "0") match the schema;
    /// plain i64 would surface as `Int` and reject string input.
    pub source_id: Option<LongString>,
    pub title: Option<String>,
    pub url: Option<String>,
    pub initialized: Option<bool>,
    pub artist: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub in_library: Option<bool>,
    pub status: Option<MangaStatus>,
    /// Restrict to manga belonging to the given categories
    /// (WebUI library screen sends `categoryIds`).
    pub category_ids: Option<Vec<i32>>,
}

/// Mirrors `MangaOrderBy` from `MangaQuery.kt`.
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum MangaOrderBy {
    Id,
    Title,
    InLibraryAt,
    LastFetchedAt,
}

#[derive(InputObject)]
#[graphql(name = "MangaOrderInput")]
pub struct MangaOrder {
    pub by: MangaOrderBy,
    pub by_type: Option<SortOrder>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum SortOrder {
    Asc,
    Desc,
}

/// Mirrors `ChapterCondition` (core filters).
#[derive(InputObject, Default)]
#[graphql(name = "ChapterConditionInput")]
pub struct ChapterCondition {
    pub manga_id: Option<i32>,
    pub id: Option<i32>,
    pub source_order: Option<i32>,
}

/// Mirrors `CategoryCondition` (core).
#[derive(InputObject, Default)]
#[graphql(name = "CategoryConditionInput")]
pub struct CategoryCondition {
    pub id: Option<i32>,
    pub name: Option<String>,
    pub default: Option<bool>,
}

/// Mirrors `MetaCondition` from `MetaQuery.kt`.
#[derive(InputObject, Default)]
#[graphql(name = "MetaConditionInput")]
pub struct MetaCondition {
    pub key: Option<String>,
    pub value: Option<String>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum MetaOrderBy {
    Key,
    Value,
}

#[derive(InputObject)]
#[graphql(name = "MetaOrderInput")]
pub struct MetaOrder {
    pub by: MetaOrderBy,
    pub by_type: Option<SortOrder>,
}

// ---------------------------------------------------------------------------
// Filter inputs (shape parity with the baseline; filtering semantics applied
// incrementally — many filters are accepted and currently ignored).
// ---------------------------------------------------------------------------

macro_rules! scalar_filter_input {
    ($name:ident, $ty:ty) => {
        #[derive(InputObject, Default)]
        pub struct $name {
            pub distinct_from: Option<$ty>,
            pub distinct_from_all: Option<Vec<$ty>>,
            pub distinct_from_any: Option<Vec<$ty>>,
            pub equal_to: Option<$ty>,
            pub greater_than: Option<$ty>,
            pub greater_than_or_equal_to: Option<$ty>,
            pub in_: Option<Vec<$ty>>,
            pub is_null: Option<bool>,
            pub less_than: Option<$ty>,
            pub less_than_or_equal_to: Option<$ty>,
            pub not_distinct_from: Option<$ty>,
            pub not_equal_to: Option<$ty>,
            pub not_equal_to_all: Option<Vec<$ty>>,
            pub not_equal_to_any: Option<Vec<$ty>>,
            pub not_in: Option<Vec<$ty>>,
        }
    };
}

scalar_filter_input!(BooleanFilterInput, bool);
scalar_filter_input!(IntFilterInput, i32);
// Upstream `LongFilterInput` wraps `LongString` (accepts String or Number),
// so clients send e.g. `notEqualToAll: ["0"]` for epoch-second longs.
scalar_filter_input!(LongFilterInput, LongString);
scalar_filter_input!(DoubleFilterInput, f64);
scalar_filter_input!(MangaStatusFilterInput, MangaStatus);
scalar_filter_input!(ContentWarningFilterInput, ContentWarning);

macro_rules! string_filter_input {
    ($name:ident) => {
        #[derive(InputObject, Default)]
        pub struct $name {
            pub distinct_from: Option<String>,
            pub distinct_from_all: Option<Vec<String>>,
            pub distinct_from_any: Option<Vec<String>>,
            pub distinct_from_insensitive: Option<String>,
            pub distinct_from_insensitive_all: Option<Vec<String>>,
            pub distinct_from_insensitive_any: Option<Vec<String>>,
            pub ends_with: Option<String>,
            pub ends_with_all: Option<Vec<String>>,
            pub ends_with_any: Option<Vec<String>>,
            pub ends_with_insensitive: Option<String>,
            pub ends_with_insensitive_all: Option<Vec<String>>,
            pub ends_with_insensitive_any: Option<Vec<String>>,
            pub equal_to: Option<String>,
            pub greater_than: Option<String>,
            pub greater_than_insensitive: Option<String>,
            pub greater_than_or_equal_to: Option<String>,
            pub greater_than_or_equal_to_insensitive: Option<String>,
            pub in_: Option<Vec<String>>,
            pub in_insensitive: Option<Vec<String>>,
            pub includes: Option<String>,
            pub includes_all: Option<Vec<String>>,
            pub includes_any: Option<Vec<String>>,
            pub includes_insensitive: Option<String>,
            pub includes_insensitive_all: Option<Vec<String>>,
            pub includes_insensitive_any: Option<Vec<String>>,
            pub is_null: Option<bool>,
            pub less_than: Option<String>,
            pub less_than_insensitive: Option<String>,
            pub less_than_or_equal_to: Option<String>,
            pub less_than_or_equal_to_insensitive: Option<String>,
            pub like: Option<String>,
            pub like_all: Option<Vec<String>>,
            pub like_any: Option<Vec<String>>,
            pub like_insensitive: Option<String>,
            pub like_insensitive_all: Option<Vec<String>>,
            pub like_insensitive_any: Option<Vec<String>>,
            pub not_distinct_from: Option<String>,
            pub not_distinct_from_insensitive: Option<String>,
            pub not_ends_with: Option<String>,
            pub not_ends_with_all: Option<Vec<String>>,
            pub not_ends_with_any: Option<Vec<String>>,
            pub not_ends_with_insensitive: Option<String>,
            pub not_ends_with_insensitive_all: Option<Vec<String>>,
            pub not_ends_with_insensitive_any: Option<Vec<String>>,
            pub not_equal_to: Option<String>,
            pub not_equal_to_all: Option<Vec<String>>,
            pub not_equal_to_any: Option<Vec<String>>,
            pub not_equal_to_insensitive: Option<String>,
            pub not_equal_to_insensitive_all: Option<Vec<String>>,
            pub not_equal_to_insensitive_any: Option<Vec<String>>,
            pub not_in: Option<Vec<String>>,
            pub not_in_insensitive: Option<Vec<String>>,
            pub not_like: Option<String>,
            pub not_like_all: Option<Vec<String>>,
            pub not_like_any: Option<Vec<String>>,
            pub not_like_insensitive: Option<String>,
            pub not_like_insensitive_all: Option<Vec<String>>,
            pub not_like_insensitive_any: Option<Vec<String>>,
            pub not_starts_with: Option<String>,
            pub not_starts_with_all: Option<Vec<String>>,
            pub not_starts_with_any: Option<Vec<String>>,
            pub not_starts_with_insensitive: Option<String>,
            pub not_starts_with_insensitive_all: Option<Vec<String>>,
            pub not_starts_with_insensitive_any: Option<Vec<String>>,
            pub starts_with: Option<String>,
            pub starts_with_all: Option<Vec<String>>,
            pub starts_with_any: Option<Vec<String>>,
            pub starts_with_insensitive: Option<String>,
            pub starts_with_insensitive_all: Option<Vec<String>>,
            pub starts_with_insensitive_any: Option<Vec<String>>,
        }
    };
}

string_filter_input!(StringFilterInput);

#[derive(InputObject, Default)]
pub struct MangaFilterInput {
    pub and: Option<Vec<MangaFilterInput>>,
    pub artist: Option<StringFilterInput>,
    pub author: Option<StringFilterInput>,
    pub category_id: Option<IntFilterInput>,
    pub chapters_last_fetched_at: Option<LongFilterInput>,
    pub description: Option<StringFilterInput>,
    pub genre: Option<StringFilterInput>,
    pub id: Option<IntFilterInput>,
    pub in_library: Option<BooleanFilterInput>,
    pub in_library_at: Option<LongFilterInput>,
    pub initialized: Option<BooleanFilterInput>,
    pub last_fetched_at: Option<LongFilterInput>,
    pub not: Option<Box<MangaFilterInput>>,
    pub or: Option<Vec<MangaFilterInput>>,
    pub real_url: Option<StringFilterInput>,
    pub source_id: Option<LongFilterInput>,
    pub status: Option<MangaStatusFilterInput>,
    pub thumbnail_url: Option<StringFilterInput>,
    pub title: Option<StringFilterInput>,
    pub url: Option<StringFilterInput>,
}

#[derive(InputObject, Default)]
pub struct CategoryFilterInput {
    pub and: Option<Vec<CategoryFilterInput>>,
    pub default: Option<BooleanFilterInput>,
    pub id: Option<IntFilterInput>,
    pub name: Option<StringFilterInput>,
    pub not: Option<Box<CategoryFilterInput>>,
    pub or: Option<Vec<CategoryFilterInput>>,
    pub order: Option<IntFilterInput>,
}

#[derive(InputObject, Default)]
pub struct ChapterFilterInput {
    pub and: Option<Vec<ChapterFilterInput>>,
    pub chapter_number: Option<DoubleFilterInput>,
    pub fetched_at: Option<LongFilterInput>,
    pub id: Option<IntFilterInput>,
    pub in_library: Option<BooleanFilterInput>,
    pub is_bookmarked: Option<BooleanFilterInput>,
    pub is_downloaded: Option<BooleanFilterInput>,
    pub is_read: Option<BooleanFilterInput>,
    pub last_page_read: Option<IntFilterInput>,
    pub last_read_at: Option<LongFilterInput>,
    pub manga_id: Option<IntFilterInput>,
    pub name: Option<StringFilterInput>,
    pub not: Option<Box<ChapterFilterInput>>,
    pub or: Option<Vec<ChapterFilterInput>>,
    pub page_count: Option<IntFilterInput>,
    pub real_url: Option<StringFilterInput>,
    pub scanlator: Option<StringFilterInput>,
    pub source_order: Option<IntFilterInput>,
    pub upload_date: Option<LongFilterInput>,
    pub url: Option<StringFilterInput>,
}

#[derive(InputObject, Default)]
pub struct MetaFilterInput {
    pub and: Option<Vec<MetaFilterInput>>,
    pub key: Option<StringFilterInput>,
    pub not: Option<Box<MetaFilterInput>>,
    pub or: Option<Vec<MetaFilterInput>>,
    pub value: Option<StringFilterInput>,
}

/// Mirrors `SourceConditionInput`.
#[derive(InputObject, Default)]
#[graphql(name = "SourceConditionInput")]
pub struct SourceCondition {
    pub content_warning: Option<ContentWarning>,
    pub id: Option<LongString>,
    pub lang: Option<String>,
    pub name: Option<String>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum SourceOrderBy {
    Id,
    Name,
    Lang,
}

#[derive(InputObject)]
#[graphql(name = "SourceOrderInput")]
pub struct SourceOrder {
    pub by: SourceOrderBy,
    pub by_type: Option<SortOrder>,
}

#[derive(InputObject, Default)]
pub struct SourceFilterInput {
    pub and: Option<Vec<SourceFilterInput>>,
    pub content_warning: Option<ContentWarningFilterInput>,
    pub id: Option<LongFilterInput>,
    pub lang: Option<StringFilterInput>,
    pub name: Option<StringFilterInput>,
    pub not: Option<Box<SourceFilterInput>>,
    pub or: Option<Vec<SourceFilterInput>>,
}

#[derive(InputObject, Default)]
#[graphql(name = "ExtensionConditionInput")]
pub struct ExtensionCondition {
    pub lang: Option<String>,
    pub name: Option<String>,
    pub pkg_name: Option<String>,
    pub is_installed: Option<bool>,
    pub is_obsolete: Option<bool>,
    pub has_update: Option<bool>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum ExtensionOrderBy {
    PkgName,
    Name,
    ApkName,
}

#[derive(InputObject)]
#[graphql(name = "ExtensionOrderInput")]
pub struct ExtensionOrder {
    pub by: ExtensionOrderBy,
    pub by_type: Option<SortOrder>,
}

#[derive(InputObject, Default)]
pub struct ExtensionFilterInput {
    pub and: Option<Vec<ExtensionFilterInput>>,
    pub lang: Option<StringFilterInput>,
    pub name: Option<StringFilterInput>,
    pub not: Option<Box<ExtensionFilterInput>>,
    pub or: Option<Vec<ExtensionFilterInput>>,
    pub pkg_name: Option<StringFilterInput>,
}

#[derive(InputObject, Default)]
#[graphql(name = "ExtensionStoreConditionInput")]
pub struct ExtensionStoreCondition {
    pub id: Option<i32>,
    pub index_url: Option<String>,
    pub name: Option<String>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum ExtensionStoreOrderBy {
    Name,
    IndexUrl,
}

#[derive(InputObject)]
#[graphql(name = "ExtensionStoreOrderInput")]
pub struct ExtensionStoreOrder {
    pub by: ExtensionStoreOrderBy,
    pub by_type: Option<SortOrder>,
}

#[derive(InputObject, Default)]
pub struct ExtensionStoreFilterInput {
    pub and: Option<Vec<ExtensionStoreFilterInput>>,
    pub index_url: Option<StringFilterInput>,
    pub name: Option<StringFilterInput>,
    pub not: Option<Box<ExtensionStoreFilterInput>>,
    pub or: Option<Vec<ExtensionStoreFilterInput>>,
}

#[derive(InputObject, Default)]
#[graphql(name = "TrackerConditionInput")]
pub struct TrackerCondition {
    pub id: Option<i32>,
    pub is_logged_in: Option<bool>,
    pub name: Option<String>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum TrackerOrderBy {
    Id,
    Name,
    IsLoggedIn,
}

#[derive(InputObject)]
#[graphql(name = "TrackerOrderInput")]
pub struct TrackerOrder {
    pub by: TrackerOrderBy,
    pub by_type: Option<SortOrder>,
}

#[derive(InputObject, Default)]
#[graphql(name = "TrackRecordConditionInput")]
pub struct TrackRecordCondition {
    pub id: Option<i32>,
    pub manga_id: Option<i32>,
    pub tracker_id: Option<i32>,
    pub remote_id: Option<LongString>,
    pub title: Option<String>,
    pub status: Option<i32>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum TrackRecordOrderBy {
    Id,
    MangaId,
    TrackerId,
    RemoteId,
    Title,
    LastChapterRead,
    TotalChapters,
    Score,
    StartDate,
    FinishDate,
    Private,
}

#[derive(InputObject)]
#[graphql(name = "TrackRecordOrderInput")]
pub struct TrackRecordOrder {
    pub by: TrackRecordOrderBy,
    pub by_type: Option<SortOrder>,
}

#[derive(InputObject, Default)]
pub struct TrackRecordFilterInput {
    pub and: Option<Vec<TrackRecordFilterInput>>,
    pub manga_id: Option<IntFilterInput>,
    pub not: Option<Box<TrackRecordFilterInput>>,
    pub or: Option<Vec<TrackRecordFilterInput>>,
    pub title: Option<StringFilterInput>,
    pub tracker_id: Option<IntFilterInput>,
}

/// Mirrors `UpdateStatus` — deprecated query payload (idle).
#[derive(SimpleObject, Clone)]
#[graphql(name = "UpdateStatus")]
pub struct UpdateStatusPayload {
    pub complete_jobs: UpdateStatusJobs,
    pub failed_jobs: UpdateStatusJobs,
    pub is_running: bool,
    pub pending_jobs: UpdateStatusJobs,
    pub running_jobs: UpdateStatusJobs,
    pub skipped_categories: UpdateStatusCategories,
    pub skipped_jobs: UpdateStatusJobs,
    pub updating_categories: UpdateStatusCategories,
}

impl UpdateStatusPayload {
    pub fn idle() -> Self {
        Self {
            complete_jobs: UpdateStatusJobs::empty(),
            failed_jobs: UpdateStatusJobs::empty(),
            is_running: false,
            pending_jobs: UpdateStatusJobs::empty(),
            running_jobs: UpdateStatusJobs::empty(),
            skipped_categories: UpdateStatusCategories::empty(),
            skipped_jobs: UpdateStatusJobs::empty(),
            updating_categories: UpdateStatusCategories::empty(),
        }
    }
}

#[derive(SimpleObject, Clone)]
#[graphql(name = "UpdateStatusType")]
pub struct UpdateStatusJobs {
    pub mangas: MangaNodeList,
}

impl UpdateStatusJobs {
    pub fn empty() -> Self {
        Self { mangas: MangaNodeList::from_nodes(vec![]) }
    }
}

#[derive(SimpleObject, Clone)]
#[graphql(name = "UpdateStatusCategoryType")]
pub struct UpdateStatusCategories {
    pub categories: CategoryNodeList,
}

impl UpdateStatusCategories {
    pub fn empty() -> Self {
        Self { categories: CategoryNodeList::from_nodes(vec![]) }
    }
}

#[derive(InputObject)]
pub struct SearchTrackerInput {
    pub query: String,
    pub tracker_id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct LastUpdateTimestampPayload {
    pub timestamp: LongString,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum SyncState {
    Started,
    CreatingBackup,
    Downloading,
    Merging,
    Uploading,
    Restoring,
    Success,
    Error,
}

#[derive(SimpleObject, Clone)]
pub struct SyncStatus {
    pub backup_restore_id: Option<String>,
    pub end_date: Option<LongString>,
    pub error_message: Option<String>,
    pub start_date: LongString,
    pub state: SyncState,
}

#[derive(SimpleObject, Clone)]
pub struct AboutWebUI {
    pub channel: WebUIChannel,
    pub tag: String,
    pub update_timestamp: LongString,
    /// Build time as Unix epoch seconds (line 3 of version.txt; 0 when absent).
    pub build_time: LongString,
}

#[derive(SimpleObject, Clone)]
pub struct WebUIUpdateCheck {
    pub channel: WebUIChannel,
    pub tag: String,
    pub update_available: bool,
}

#[derive(SimpleObject, Clone)]
pub struct CheckForServerUpdatesPayload {
    pub channel: String,
    pub tag: String,
    pub url: String,
}

/// Root Query object.
#[derive(Default)]
pub struct QueryRoot;

#[Object(name = "Query")]
impl QueryRoot {
    async fn manga(&self, ctx: &Context<'_>, id: i32) -> async_graphql::Result<MangaType> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM manga WHERE id = ?");
        let row = sqlx::query_as::<_, MangaRow>(&sql)
            .bind(id)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?
            .ok_or_else(|| async_graphql::Error::new("Manga not found"))?;
        Ok(MangaType::from_row(&row))
    }

    #[allow(clippy::too_many_arguments)]
    async fn mangas(
        &self,
        ctx: &Context<'_>,
        condition: Option<MangaCondition>,
        filter: Option<MangaFilterInput>,
        order: Option<Vec<MangaOrder>>,
        before: Option<Cursor>,
        after: Option<Cursor>,
        first: Option<i32>,
        last: Option<i32>,
        offset: Option<i32>,
    ) -> async_graphql::Result<MangaNodeList> {
        let _ = (before, last, offset); // cursor/offset pagination applied incrementally
        let state = ctx.data::<GraphQLState>()?;
        let rows = query_mangas(state, condition.as_ref(), filter.as_ref(), order.as_ref(), first, after.as_ref()).await?;
        let nodes: Vec<MangaType> = rows.iter().map(MangaType::from_row).collect();
        Ok(MangaNodeList::from_nodes(nodes))
    }

    async fn category(&self, ctx: &Context<'_>, id: i32) -> async_graphql::Result<CategoryType> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM category WHERE id = ?");
        let row = sqlx::query_as::<_, CategoryRow>(&sql)
            .bind(id)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?
            .ok_or_else(|| async_graphql::Error::new("Category not found"))?;
        Ok(CategoryType::from(&row))
    }

    #[allow(clippy::too_many_arguments)]
    async fn categories(
        &self,
        ctx: &Context<'_>,
        condition: Option<CategoryCondition>,
        filter: Option<CategoryFilterInput>,
        order: Option<Vec<CategoryOrderInput>>,
        before: Option<Cursor>,
        after: Option<Cursor>,
        first: Option<i32>,
        last: Option<i32>,
        offset: Option<i32>,
    ) -> async_graphql::Result<CategoryNodeList> {
        let _ = (filter, order, before, after, first, last, offset); // shape parity
        let state = ctx.data::<GraphQLState>()?;
        let list = state.category.get_category_list().await.map_err(async_graphql::Error::from)?;
        let nodes: Vec<CategoryType> = list
            .iter()
            .filter(|c| {
                condition
                    .as_ref()
                    .map(|cond| {
                        cond.id.map(|v| v == c.id).unwrap_or(true)
                            && cond.name.as_ref().map(|v| &c.name == v).unwrap_or(true)
                            && cond.default.map(|v| v == c.default).unwrap_or(true)
                    })
                    .unwrap_or(true)
            })
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

    async fn chapter(&self, ctx: &Context<'_>, id: i32) -> async_graphql::Result<ChapterType> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM chapter WHERE id = ?");
        let row = sqlx::query_as::<_, ChapterRow>(&sql)
            .bind(id)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?
            .ok_or_else(|| async_graphql::Error::new("Chapter not found"))?;
        Ok(ChapterType::from_row(&row))
    }

    #[allow(clippy::too_many_arguments)]
    async fn chapters(
        &self,
        ctx: &Context<'_>,
        condition: Option<ChapterCondition>,
        filter: Option<ChapterFilterInput>,
        order: Option<Vec<ChapterOrderInput>>,
        before: Option<Cursor>,
        after: Option<Cursor>,
        first: Option<i32>,
        last: Option<i32>,
        offset: Option<i32>,
    ) -> async_graphql::Result<ChapterNodeList> {
        let _ = (before, after, last); // cursor pagination not yet wired; first/offset used
        let state = ctx.data::<GraphQLState>()?;
        let rows = query_chapters(state, condition.as_ref(), filter.as_ref(), order.as_ref(), first, offset).await?;
        let nodes: Vec<ChapterType> = rows.iter().map(ChapterType::from_row).collect();
        Ok(ChapterNodeList::from_nodes(nodes))
    }

    /// Mirrors `meta(key:)` — single global meta entry.
    async fn meta(&self, ctx: &Context<'_>, key: String) -> async_graphql::Result<GlobalMetaType> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT meta_key, value FROM global_meta WHERE meta_key = ?");
        let row = sqlx::query(&sql)
            .bind(&key)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?
            .ok_or_else(|| async_graphql::Error::new("Meta not found"))?;
        let value: String = row.try_get("value").map_err(async_graphql::Error::from)?;
        Ok(GlobalMetaType { key, value })
    }

    /// Mirrors `metas(condition:)` — global meta list.
    #[allow(clippy::too_many_arguments)]
    async fn metas(
        &self,
        ctx: &Context<'_>,
        condition: Option<MetaCondition>,
        filter: Option<MetaFilterInput>,
        order: Option<Vec<MetaOrder>>,
        before: Option<Cursor>,
        after: Option<Cursor>,
        first: Option<i32>,
        last: Option<i32>,
        offset: Option<i32>,
    ) -> async_graphql::Result<GlobalMetaNodeList> {
        let _ = (filter, before, last, offset); // shape parity
        let state = ctx.data::<GraphQLState>()?;
        let mut sql = "SELECT meta_key, value FROM global_meta".to_string();
        let mut where_clauses: Vec<String> = Vec::new();
        let mut binds: Vec<BindVal> = Vec::new();
        if let Some(cond) = condition {
            if let Some(v) = cond.key {
                where_clauses.push("meta_key = ?".into());
                binds.push(BindVal::Str(v));
            }
            if let Some(v) = cond.value {
                where_clauses.push("value = ?".into());
                binds.push(BindVal::Str(v));
            }
        }
        if let Some(cursor) = after {
            if let Ok(id) = cursor.0.parse::<i32>() {
                where_clauses.push("id > ?".into());
                binds.push(BindVal::I32(id));
            }
        }
        if !where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clauses.join(" AND "));
        }
        if let Some(orders) = order {
            if let Some(o) = orders.first() {
                let col = match o.by {
                    MetaOrderBy::Key => "meta_key",
                    MetaOrderBy::Value => "value",
                };
                let dir = match o.by_type {
                    Some(SortOrder::Asc) => "ASC",
                    _ => "DESC",
                };
                sql.push_str(&format!(" ORDER BY {col} {dir}"));
            }
        } else {
            sql.push_str(" ORDER BY meta_key ASC");
        }
        if let Some(limit) = first {
            sql.push_str(&format!(" LIMIT {}", limit.clamp(1, 500)));
        }
        let sql = bind_placeholders(&sql);
        let mut q = sqlx::query(&sql);
        for b in &binds {
            q = match b {
                BindVal::I32(x) => q.bind(*x),
                BindVal::I64(x) => q.bind(*x),
                BindVal::Bool(x) => q.bind(*x),
                BindVal::F64(x) => q.bind(*x),
                BindVal::Str(x) => q.bind(x),
            };
        }
        let rows = q.fetch_all(state.db.pool()).await.map_err(async_graphql::Error::from)?;
        let nodes: Vec<GlobalMetaType> = rows
            .iter()
            .map(|r| GlobalMetaType {
                key: r.try_get("meta_key").unwrap_or_default(),
                value: r.try_get("value").unwrap_or_default(),
            })
            .collect();
        Ok(GlobalMetaNodeList::from_nodes(nodes))
    }

    /// Mirrors `source(id:)` — single source by id.
    async fn source(&self, ctx: &Context<'_>, id: LongString) -> async_graphql::Result<SourceType> {
        let state = ctx.data::<GraphQLState>()?;
        // LOCAL_SOURCE_ID(0) 为合成本地源（不在 source 表），直接返回合成条目。
        if id.0 == suwayomi_domain::source::LOCAL_SOURCE_ID {
            return Ok(SourceType::local_source());
        }
        let sql = bind_placeholders("SELECT * FROM source WHERE id = ?");
        let row = sqlx::query_as::<_, suwayomi_core::schema::SourceRow>(&sql)
            .bind(id.0)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?
            .ok_or_else(|| async_graphql::Error::new("Source not found"))?;
        Ok(SourceType::from_row(&row))
    }

    /// Mirrors `sources(condition:, order:)`.
    #[allow(clippy::too_many_arguments)]
    async fn sources(
        &self,
        ctx: &Context<'_>,
        condition: Option<SourceCondition>,
        filter: Option<SourceFilterInput>,
        order: Option<Vec<SourceOrder>>,
        before: Option<Cursor>,
        after: Option<Cursor>,
        first: Option<i32>,
        last: Option<i32>,
        offset: Option<i32>,
    ) -> async_graphql::Result<SourceNodeList> {
        let _ = (filter, before, after, last, offset); // shape parity
        let state = ctx.data::<GraphQLState>()?;
        let mut sql = "SELECT * FROM source".to_string();
        let mut where_clauses: Vec<String> = Vec::new();
        let mut binds: Vec<BindVal> = Vec::new();
        if let Some(cond) = &condition {
            if let Some(v) = cond.id {
                where_clauses.push("id = ?".into());
                binds.push(BindVal::I64(v.0));
            }
            if let Some(v) = &cond.lang {
                where_clauses.push("lang = ?".into());
                binds.push(BindVal::Str(v.clone()));
            }
            if let Some(v) = &cond.name {
                where_clauses.push("name ILIKE ?".into());
                binds.push(BindVal::Str(format!("%{v}%")));
            }
            if let Some(v) = cond.content_warning {
                where_clauses.push("content_warning = ?".into());
                binds.push(BindVal::I32(v.to_i32()));
            }
        }
        if !where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clauses.join(" AND "));
        }
        if let Some(orders) = &order {
            if let Some(o) = orders.first() {
                let col = match o.by {
                    SourceOrderBy::Id => "id",
                    SourceOrderBy::Name => "name",
                    SourceOrderBy::Lang => "lang",
                };
                let dir = match o.by_type {
                    Some(SortOrder::Asc) => "ASC",
                    _ => "DESC",
                };
                sql.push_str(&format!(" ORDER BY {col} {dir}"));
            }
        } else {
            sql.push_str(" ORDER BY name ASC");
        }
        if let Some(limit) = first {
            sql.push_str(&format!(" LIMIT {}", limit.clamp(1, 500)));
        }
        let sql = bind_placeholders(&sql);
        let mut q = sqlx::query_as::<_, suwayomi_core::schema::SourceRow>(&sql);
        for b in &binds {
            q = match b {
                BindVal::I32(x) => q.bind(*x),
                BindVal::I64(x) => q.bind(*x),
                BindVal::Bool(x) => q.bind(*x),
                BindVal::F64(x) => q.bind(*x),
                BindVal::Str(x) => q.bind(x),
            };
        }
        let rows = q.fetch_all(state.db.pool()).await.map_err(async_graphql::Error::from)?;
        // 批量预取扩展 pkg_name 与 source meta，注入 SourceType 缓存字段——
        // 避免 iconUrl/meta 每个源一次 DB 查询（N+1 并发把连接池打满导致
        // "pool timed out"，DebugInformation 的 sources 查询会触发）。
        let pkg_by_ext: std::collections::HashMap<i64, String> = sqlx::query_as::<_, (i32, String)>(
            bind_placeholders("SELECT id, pkg_name FROM extension").as_str(),
        )
        .fetch_all(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?
        .into_iter()
        .map(|(id, pkg)| (i64::from(id), pkg))
        .collect();
        let mut meta_by_source: std::collections::HashMap<i64, Vec<crate::types::SourceMetaType>> =
            std::collections::HashMap::new();
        // Batch-load extension rows so SourceType.extension (and therefore
        // extensionStore) never fires per-source queries (N+1) — with 31+
        // sources and async_graphql's concurrent resolver execution that
        // exhausted the connection pool and hung the sources page.
        let ext_rows: Vec<suwayomi_core::schema::ExtensionRow> =
            sqlx::query_as::<_, suwayomi_core::schema::ExtensionRow>(
                bind_placeholders("SELECT * FROM extension").as_str(),
            )
            .fetch_all(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let ext_by_id: std::collections::HashMap<i64, suwayomi_core::schema::ExtensionRow> = ext_rows
            .into_iter()
            .map(|r| (i64::from(r.id), r))
            .collect();
        let meta_rows = sqlx::query_as::<_, (i64, String, String)>(
            bind_placeholders("SELECT source_ref, meta_key, value FROM source_meta").as_str(),
        )
        .fetch_all(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
        for (sid, key, value) in meta_rows {
            meta_by_source
                .entry(sid)
                .or_default()
                .push(crate::types::SourceMetaType { key, value, source_id: sid });
        }
        let mut nodes: Vec<SourceType> = rows
            .iter()
            .map(|r| {
                let mut st = SourceType::from_row(r);
                st.icon_pkg_name = pkg_by_ext.get(&i64::from(st.extension_id)).cloned();
                st.extension_row = ext_by_id.get(&i64::from(st.extension_id)).cloned();
                st.meta_cache = meta_by_source.get(&st.id).cloned().unwrap_or_default();
                st
            })
            .collect();
        // Inject the local source (id=0, lang=OTHER — WebUI "Other" group)
        // unless an explicit condition rules it out.
        let include_local = match &condition {
            None => true,
            Some(c) => {
                c.id.map(|v| v.0 == suwayomi_domain::source::LOCAL_SOURCE_ID).unwrap_or(true)
                    && c.lang.as_deref().map(|l| l == "OTHER").unwrap_or(true)
                    && c.name.as_deref().map(|n| n.to_lowercase().contains("local")).unwrap_or(true)
            }
        };
        if include_local {
            nodes.push(SourceType::local_source());
        }
        if order.is_none() {
            nodes.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        }
        Ok(SourceNodeList::from_nodes(nodes))
    }

    /// Mirrors `extension(pkgName:)` — single extension.
    async fn extension(&self, ctx: &Context<'_>, pkg_name: String) -> async_graphql::Result<ExtensionType> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM extension WHERE pkg_name = ?");
        let row = sqlx::query_as::<_, suwayomi_core::schema::ExtensionRow>(&sql)
            .bind(&pkg_name)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?
            .ok_or_else(|| async_graphql::Error::new("Extension not found"))?;
        Ok(ExtensionType { row })
    }

    /// Mirrors `extensions(condition:, order:)`.
    #[allow(clippy::too_many_arguments)]
    async fn extensions(
        &self,
        ctx: &Context<'_>,
        condition: Option<ExtensionCondition>,
        filter: Option<ExtensionFilterInput>,
        order: Option<Vec<ExtensionOrder>>,
        before: Option<Cursor>,
        after: Option<Cursor>,
        first: Option<i32>,
        last: Option<i32>,
        offset: Option<i32>,
    ) -> async_graphql::Result<ExtensionNodeList> {
        let _ = (filter, before, after, last, offset); // shape parity
        let state = ctx.data::<GraphQLState>()?;
        let mut sql = "SELECT * FROM extension".to_string();
        let mut where_clauses: Vec<String> = Vec::new();
        let mut binds: Vec<BindVal> = Vec::new();
        if let Some(cond) = condition {
            if let Some(v) = &cond.lang {
                where_clauses.push("lang = ?".into());
                binds.push(BindVal::Str(v.clone()));
            }
            if let Some(v) = &cond.name {
                where_clauses.push("name ILIKE ?".into());
                binds.push(BindVal::Str(format!("%{v}%")));
            }
            if let Some(v) = &cond.pkg_name {
                where_clauses.push("pkg_name = ?".into());
                binds.push(BindVal::Str(v.clone()));
            }
            if let Some(v) = cond.is_installed {
                where_clauses.push("is_installed = ?".into());
                binds.push(BindVal::Bool(v));
            }
            if let Some(v) = cond.is_obsolete {
                where_clauses.push("is_obsolete = ?".into());
                binds.push(BindVal::Bool(v));
            }
            if let Some(v) = cond.has_update {
                where_clauses.push("has_update = ?".into());
                binds.push(BindVal::Bool(v));
            }
        }
        if !where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clauses.join(" AND "));
        }
        if let Some(orders) = order {
            if let Some(o) = orders.first() {
                let col = match o.by {
                    ExtensionOrderBy::PkgName => "pkg_name",
                    ExtensionOrderBy::Name => "name",
                    ExtensionOrderBy::ApkName => "apk_name",
                };
                let dir = match o.by_type {
                    Some(SortOrder::Asc) => "ASC",
                    _ => "DESC",
                };
                sql.push_str(&format!(" ORDER BY {col} {dir}"));
            }
        } else {
            sql.push_str(" ORDER BY name ASC");
        }
        if let Some(limit) = first {
            sql.push_str(&format!(" LIMIT {}", limit.clamp(1, 500)));
        }
        let sql = bind_placeholders(&sql);
        let mut q = sqlx::query_as::<_, suwayomi_core::schema::ExtensionRow>(&sql);
        for b in &binds {
            q = match b {
                BindVal::I32(x) => q.bind(*x),
                BindVal::I64(x) => q.bind(*x),
                BindVal::Bool(x) => q.bind(*x),
                BindVal::F64(x) => q.bind(*x),
                BindVal::Str(x) => q.bind(x),
            };
        }
        let rows = q.fetch_all(state.db.pool()).await.map_err(async_graphql::Error::from)?;
        let nodes: Vec<ExtensionType> = rows.into_iter().map(|row| ExtensionType { row }).collect();
        Ok(ExtensionNodeList::from_nodes(nodes))
    }

    /// Mirrors `extensionStore(indexUrl:)`.
    async fn extension_store(&self, ctx: &Context<'_>, index_url: String) -> async_graphql::Result<ExtensionStoreType> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM extension_store WHERE index_url = ?");
        let row = sqlx::query_as::<_, suwayomi_core::schema::ExtensionStoreRow>(&sql)
            .bind(&index_url)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?
            .ok_or_else(|| async_graphql::Error::new("ExtensionStore not found"))?;
        Ok(ExtensionStoreType::from_row(row))
    }

    /// Mirrors `extensionStores(condition:, order:)`.
    #[allow(clippy::too_many_arguments)]
    async fn extension_stores(
        &self,
        ctx: &Context<'_>,
        condition: Option<ExtensionStoreCondition>,
        filter: Option<ExtensionStoreFilterInput>,
        _order: Option<Vec<ExtensionStoreOrder>>,
        before: Option<Cursor>,
        after: Option<Cursor>,
        first: Option<i32>,
        last: Option<i32>,
        offset: Option<i32>,
    ) -> async_graphql::Result<ExtensionStoreNodeList> {
        let _ = (filter, before, after, last, offset); // shape parity
        let state = ctx.data::<GraphQLState>()?;
        let mut sql = "SELECT * FROM extension_store".to_string();
        let mut where_clauses: Vec<String> = Vec::new();
        let mut binds: Vec<BindVal> = Vec::new();
        if let Some(cond) = condition {
            if let Some(v) = &cond.index_url {
                where_clauses.push("index_url = ?".into());
                binds.push(BindVal::Str(v.clone()));
            }
            if let Some(v) = &cond.name {
                where_clauses.push("name ILIKE ?".into());
                binds.push(BindVal::Str(format!("%{v}%")));
            }
        }
        if !where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clauses.join(" AND "));
        }
        if let Some(limit) = first {
            sql.push_str(&format!(" LIMIT {}", limit.clamp(1, 500)));
        }
        let sql = bind_placeholders(&sql);
        let mut q = sqlx::query_as::<_, suwayomi_core::schema::ExtensionStoreRow>(&sql);
        for b in &binds {
            q = match b {
                BindVal::Str(x) => q.bind(x),
                _ => unreachable!("extension_store has only string conditions"),
            };
        }
        let rows = q.fetch_all(state.db.pool()).await.map_err(async_graphql::Error::from)?;
        let nodes: Vec<ExtensionStoreType> = rows.into_iter().map(ExtensionStoreType::from_row).collect();
        Ok(ExtensionStoreNodeList::from_nodes(nodes))
    }

    /// Mirrors `tracker(id:)` — single tracker metadata.
    async fn tracker(&self, _ctx: &Context<'_>, id: i32) -> async_graphql::Result<TrackerType> {
        TrackerType::by_id(id, false).ok_or_else(|| async_graphql::Error::new("Tracker not found"))
    }

    /// Mirrors `trackers(condition:, order:)`.
    #[allow(clippy::too_many_arguments)]
    async fn trackers(
        &self,
        _ctx: &Context<'_>,
        condition: Option<TrackerCondition>,
        order: Option<Vec<TrackerOrder>>,
        before: Option<Cursor>,
        after: Option<Cursor>,
        first: Option<i32>,
        last: Option<i32>,
        offset: Option<i32>,
    ) -> async_graphql::Result<TrackerNodeList> {
        let _ = (order, before, after, last, offset); // shape parity
        let mut nodes = TrackerType::all();
        if let Some(cond) = condition {
            nodes.retain(|t| {
                cond.id.map(|v| v == t.id).unwrap_or(true)
                    && cond.is_logged_in.map(|v| v == t.is_logged_in).unwrap_or(true)
                    && cond.name.as_ref().map(|v| &t.name == v).unwrap_or(true)
            });
        }
        if let Some(limit) = first {
            nodes.truncate(limit.clamp(0, 500) as usize);
        }
        Ok(TrackerNodeList::from_nodes(nodes))
    }

    /// Mirrors `trackRecord(id:)`.
    async fn track_record(&self, ctx: &Context<'_>, id: i32) -> async_graphql::Result<TrackRecordType> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM track_record WHERE id = ?");
        let row = sqlx::query_as::<_, suwayomi_core::schema::TrackRecordRow>(&sql)
            .bind(id)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?
            .ok_or_else(|| async_graphql::Error::new("TrackRecord not found"))?;
        Ok(TrackRecordType::from_row(&row))
    }

    /// Mirrors `trackRecords(condition:, order:)`.
    #[allow(clippy::too_many_arguments)]
    async fn track_records(
        &self,
        ctx: &Context<'_>,
        condition: Option<TrackRecordCondition>,
        filter: Option<TrackRecordFilterInput>,
        order: Option<Vec<TrackRecordOrder>>,
        before: Option<Cursor>,
        after: Option<Cursor>,
        first: Option<i32>,
        last: Option<i32>,
        offset: Option<i32>,
    ) -> async_graphql::Result<TrackRecordNodeList> {
        let _ = (filter, before, after, last, offset); // shape parity
        let state = ctx.data::<GraphQLState>()?;
        let mut sql = "SELECT * FROM track_record".to_string();
        let mut where_clauses: Vec<String> = Vec::new();
        let mut binds: Vec<BindVal> = Vec::new();
        if let Some(cond) = condition {
            if let Some(v) = cond.id {
                where_clauses.push("id = ?".into());
                binds.push(BindVal::I32(v));
            }
            if let Some(v) = cond.manga_id {
                where_clauses.push("manga_id = ?".into());
                binds.push(BindVal::I32(v));
            }
            if let Some(v) = cond.tracker_id {
                where_clauses.push("sync_id = ?".into());
                binds.push(BindVal::I32(v));
            }
            if let Some(v) = cond.remote_id {
                where_clauses.push("remote_id = ?".into());
                binds.push(BindVal::I64(v.0));
            }
            if let Some(v) = &cond.title {
                where_clauses.push("title ILIKE ?".into());
                binds.push(BindVal::Str(format!("%{v}%")));
            }
            if let Some(v) = cond.status {
                where_clauses.push("status = ?".into());
                binds.push(BindVal::I32(v));
            }
        }
        if !where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clauses.join(" AND "));
        }
        if let Some(orders) = order {
            if let Some(o) = orders.first() {
                let col = match o.by {
                    TrackRecordOrderBy::Id => "id",
                    TrackRecordOrderBy::MangaId => "manga_id",
                    TrackRecordOrderBy::TrackerId => "sync_id",
                    TrackRecordOrderBy::RemoteId => "remote_id",
                    TrackRecordOrderBy::Title => "title",
                    TrackRecordOrderBy::LastChapterRead => "last_chapter_read",
                    TrackRecordOrderBy::TotalChapters => "total_chapters",
                    TrackRecordOrderBy::Score => "score",
                    TrackRecordOrderBy::StartDate => "start_date",
                    TrackRecordOrderBy::FinishDate => "finish_date",
                    TrackRecordOrderBy::Private => "private",
                };
                let dir = match o.by_type {
                    Some(SortOrder::Asc) => "ASC",
                    _ => "DESC",
                };
                sql.push_str(&format!(" ORDER BY {col} {dir}"));
            }
        } else {
            sql.push_str(" ORDER BY id ASC");
        }
        if let Some(limit) = first {
            sql.push_str(&format!(" LIMIT {}", limit.clamp(1, 500)));
        }
        let sql = bind_placeholders(&sql);
        let mut q = sqlx::query_as::<_, suwayomi_core::schema::TrackRecordRow>(&sql);
        for b in &binds {
            q = match b {
                BindVal::I32(x) => q.bind(*x),
                BindVal::I64(x) => q.bind(*x),
                BindVal::Bool(x) => q.bind(*x),
                BindVal::F64(x) => q.bind(*x),
                BindVal::Str(x) => q.bind(x),
            };
        }
        let rows = q.fetch_all(state.db.pool()).await.map_err(async_graphql::Error::from)?;
        let nodes: Vec<TrackRecordType> = rows.iter().map(TrackRecordType::from_row).collect();
        Ok(TrackRecordNodeList::from_nodes(nodes))
    }

    /// Mirrors `searchTracker(input:)` — tracker API search (Phase 6 wires login).
    async fn search_tracker(
        &self,
        _ctx: &Context<'_>,
        input: SearchTrackerInput,
    ) -> async_graphql::Result<SearchTrackerPayload> {
        let _ = input;
        Ok(SearchTrackerPayload { track_searches: vec![] })
    }

    /// Mirrors `downloadStatus()` — current download queue (Phase 6 wires the manager).
    async fn download_status(&self) -> DownloadStatus {
        DownloadStatus::idle()
    }

    /// Mirrors `updateStatus()` — deprecated library update status.
    async fn update_status(&self) -> UpdateStatusPayload {
        UpdateStatusPayload::idle()
    }

    /// Mirrors `libraryUpdateStatus()` — live status/progress of the global updater.
    async fn library_update_status(&self, ctx: &Context<'_>) -> async_graphql::Result<LibraryUpdateStatus> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        Ok(state.update.latest_status().await)
    }

    /// Mirrors `lastUpdateTimestamp()` — epoch-millis of the last finished global update.
    async fn last_update_timestamp(&self, ctx: &Context<'_>) -> async_graphql::Result<LastUpdateTimestampPayload> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        Ok(LastUpdateTimestampPayload { timestamp: LongString(state.update.last_update_timestamp_ms().await) })
    }

    /// Mirrors `koSyncStatus()` — Koreader sync (Phase 6 wires accounts).
    async fn ko_sync_status(&self, ctx: &Context<'_>) -> async_graphql::Result<KoSyncStatusPayloadType> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        let status = state.koreader.get_status().await?;
        Ok(KoSyncStatusPayloadType {
            is_logged_in: status.is_logged_in,
            server_address: status.server_address,
            username: status.username,
        })
    }

    /// Mirrors `lastSyncStatus()` — SyncYomi status.
    async fn last_sync_status(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<SyncStatus>> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        match state.sync_yomi.last_sync_status().await? {
            Some(st) => Ok(Some(SyncStatus {
                backup_restore_id: None,
                end_date: None,
                error_message: None,
                start_date: LongString(st.synced_at * 1000),
                state: SyncState::Success,
            })),
            None => Ok(None),
        }
    }

    /// Mirrors `aboutWebUI()` — the locally deployed WebUI version
    /// Mirrors `aboutWebUI()` — version from `<webui_dir>/version.txt`
    /// (line 1) with the channel on line 2 (written by the WebUI's own CI),
    /// falling back to the legacy `revision` file.
    #[graphql(name = "aboutWebUI")]
    async fn about_web_ui(&self, ctx: &Context<'_>) -> AboutWebUI {
        let dir = ctx.data::<GraphQLState>().map(|s| s.webui_dir.clone()).unwrap_or_default();
        let tag = local_webui_version(&dir);
        let channel = local_webui_channel(&dir);
        let build_time = local_webui_build_time(&dir);
        AboutWebUI { channel, tag, update_timestamp: LongString(0), build_time }
    }

    /// Mirrors `checkForServerUpdates()` — compares the local server build
    /// with the latest 576576/Suwayomi-next release on GitHub. Empty when
    /// up-to-date or the check fails.
    async fn check_for_server_updates(&self) -> Vec<CheckForServerUpdatesPayload> {
        match fetch_latest_server_release().await {
            Ok((tag, url)) => {
                let current = suwayomi_core::version::VERSION;
                tracing::info!("checkForServerUpdates: local={current} latest={tag}");
                if !tag.is_empty() && tag_to_num(&tag) > tag_to_num(current) {
                    vec![CheckForServerUpdatesPayload {
                        channel: "release".to_string(),
                        tag,
                        url,
                    }]
                } else {
                    vec![]
                }
            }
            Err(e) => {
                tracing::warn!("checkForServerUpdates failed: {e}");
                vec![]
            }
        }
    }

    /// Mirrors `checkForWebUIUpdate()` — compares the deployed revision with
    /// the latest 576576/Suwayomi-WebUI release. Empty tag on network failure
    /// (the WebUI then shows "unable to check for updates").
    #[graphql(name = "checkForWebUIUpdate")]
    async fn check_for_web_ui_update(&self, ctx: &Context<'_>) -> WebUIUpdateCheck {
        let current = ctx
            .data::<GraphQLState>()
            .map(|s| local_webui_version(&s.webui_dir))
            .unwrap_or_default();
        match fetch_latest_webui_release().await {
            Ok((latest, _)) => WebUIUpdateCheck {
                channel: WebUIChannel::Stable,
                tag: latest.clone(),
                update_available: !latest.is_empty() && tag_to_num(&latest) > tag_to_num(&current),
            },
            Err(e) => {
                tracing::warn!("checkForWebUIUpdate failed: {e}");
                WebUIUpdateCheck { channel: WebUIChannel::Stable, tag: String::new(), update_available: false }
            }
        }
    }

    /// Mirrors `restoreStatus(id:)`.
    async fn restore_status(&self, _id: String) -> BackupRestoreStatus {
        BackupRestoreStatus { manga_progress: 0, state: BackupRestoreState::Idle, total_manga: 0 }
    }

    /// Mirrors `validateBackup(input:)` — backup validation (Phase 6 parses proto).
    async fn validate_backup(
        &self,
        _ctx: &Context<'_>,
        input: ValidateBackupInput,
    ) -> async_graphql::Result<ValidateBackupResult> {
        let _ = input.backup;
        Ok(ValidateBackupResult { missing_sources: vec![], missing_trackers: vec![] })
    }

    /// Mirrors `settings()` — full settings registry.
    async fn settings(&self, ctx: &Context<'_>) -> async_graphql::Result<SettingsType> {
        let state = ctx.data::<GraphQLState>()?;
        let mut settings = SettingsType::from_config(&state.config);
        // Apply persisted overrides (the `settings` global_meta JSON blob
        // written by setSettings) so saved values survive restarts.
        let sql = bind_placeholders("SELECT value FROM global_meta WHERE meta_key = ?");
        if let Ok(row) = sqlx::query(&sql).bind("settings").fetch_optional(state.db.pool()).await {
            if let Some(row) = row {
                if let Ok(value) = row.try_get::<String, _>("value") {
                    if let Ok(blob) = serde_json::from_str::<serde_json::Value>(&value) {
                        settings.apply_overrides(&blob);
                    }
                }
            }
        }
        Ok(settings)
    }

    /// Mirrors `aboutServer()` — full payload.
    async fn about_server(&self, ctx: &Context<'_>) -> AboutServerPayload {
        let state = ctx.data::<GraphQLState>();
        let data_dir = state.as_ref().map(|s| s.data_dir.to_string_lossy().to_string()).unwrap_or_default();
        let sandbox_base = state.as_ref().ok().and_then(|s| s.sandbox_base.clone());
        let jvm = fetch_sandbox_jvm_info(sandbox_base.as_deref()).await;
        AboutServerPayload::current(&data_dir, jvm)
    }
}

/// Mirrors `CategoryOrderInput` (shape parity; currently unused by categories).
#[derive(InputObject)]
#[graphql(name = "CategoryOrderInput")]
pub struct CategoryOrderInput {
    pub by: CategoryOrderBy,
    pub by_type: Option<SortOrder>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum CategoryOrderBy {
    Id,
    Name,
    Order,
}

/// Mirrors `ChapterOrderInput` (shape parity; currently unused by chapters).
#[derive(InputObject)]
#[graphql(name = "ChapterOrderInput")]
pub struct ChapterOrderInput {
    pub by: ChapterOrderBy,
    pub by_type: Option<SortOrder>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum ChapterOrderBy {
    Id,
    MangaId,
    SourceOrder,
    UploadDate,
    FetchedAt,
    ChapterNumber,
    LastReadAt,
    Name,
}

async fn query_mangas(
    state: &GraphQLState,
    condition: Option<&MangaCondition>,
    filter: Option<&MangaFilterInput>,
    order: Option<&Vec<MangaOrder>>,
    first: Option<i32>,
    after: Option<&Cursor>,
) -> async_graphql::Result<Vec<MangaRow>> {
    let mut sql = "SELECT * FROM manga".to_string();
    let mut where_clauses: Vec<String> = Vec::new();
    let mut binds: Vec<BindVal> = vec![];

    if let Some(cond) = condition {
        if let Some(v) = cond.id {
            where_clauses.push("id = ?".into());
            binds.push(BindVal::I32(v));
        }
        if let Some(v) = cond.source_id {
            where_clauses.push("source = ?".into());
            binds.push(BindVal::I64(v.0));
        }
        if let Some(v) = &cond.title {
            where_clauses.push("title ILIKE ?".into());
            binds.push(BindVal::Str(format!("%{v}%")));
        }
        if let Some(v) = &cond.url {
            where_clauses.push("url = ?".into());
            binds.push(BindVal::Str(v.clone()));
        }
        if let Some(v) = cond.initialized {
            where_clauses.push("initialized = ?".into());
            binds.push(BindVal::Bool(v));
        }
        if let Some(v) = &cond.artist {
            where_clauses.push("artist ILIKE ?".into());
            binds.push(BindVal::Str(format!("%{v}%")));
        }
        if let Some(v) = &cond.author {
            where_clauses.push("author ILIKE ?".into());
            binds.push(BindVal::Str(format!("%{v}%")));
        }
        if let Some(v) = &cond.description {
            where_clauses.push("description ILIKE ?".into());
            binds.push(BindVal::Str(format!("%{v}%")));
        }
        if let Some(v) = cond.in_library {
            where_clauses.push("in_library = ?".into());
            binds.push(BindVal::Bool(v));
        }
        if let Some(v) = cond.status {
            where_clauses.push("status = ?".into());
            binds.push(BindVal::I32(v.to_i32()));
        }
        if let Some(ids) = &cond.category_ids {
            if !ids.is_empty() {
                let ph = vec!["?"; ids.len()].join(", ");
                where_clauses.push(format!("id IN (SELECT manga FROM category_manga WHERE category IN ({ph}))"));
                binds.extend(ids.iter().copied().map(BindVal::I32));
            }
        }
    }
    if let Some(f) = filter {
        build_manga_filter(&mut where_clauses, &mut binds, f);
    }
    if let Some(cursor) = after {
        if let Ok(id) = cursor.0.parse::<i32>() {
            where_clauses.push("id > ?".into());
            binds.push(BindVal::I32(id));
        }
    }
    if !where_clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_clauses.join(" AND "));
    }
    // ordering
    if let Some(orders) = order {
        if let Some(o) = orders.first() {
            let col = match o.by {
                MangaOrderBy::Id => "id",
                MangaOrderBy::Title => "title",
                MangaOrderBy::InLibraryAt => "in_library_at",
                MangaOrderBy::LastFetchedAt => "last_fetched_at",
            };
            let dir = match o.by_type {
                Some(SortOrder::Asc) => "ASC",
                _ => "DESC",
            };
            sql.push_str(&format!(" ORDER BY {col} {dir}"));
        }
    } else {
        sql.push_str(" ORDER BY title ASC");
    }
    if let Some(limit) = first {
        let limit = limit.clamp(1, 500);
        sql.push_str(&format!(" LIMIT {limit}"));
    }
    let sql = bind_placeholders(&sql);

    let mut q = sqlx::query_as::<_, MangaRow>(&sql);
    for b in &binds {
        q = match b {
            BindVal::I32(x) => q.bind(*x),
            BindVal::I64(x) => q.bind(*x),
            BindVal::Bool(x) => q.bind(*x),
            BindVal::F64(x) => q.bind(*x),
            BindVal::Str(x) => q.bind(x),
        };
    }
    let rows = q.fetch_all(state.db.pool()).await.map_err(async_graphql::Error::from)?;
    Ok(rows)
}

async fn fetch_chapters(state: &GraphQLState, sql: &str, binds: &[BindVal]) -> async_graphql::Result<Vec<ChapterRow>> {
    let mut q = sqlx::query_as::<_, ChapterRow>(sql);
    for b in binds {
        q = match b {
            BindVal::I32(x) => q.bind(*x),
            BindVal::I64(x) => q.bind(*x),
            BindVal::Bool(x) => q.bind(*x),
            BindVal::F64(x) => q.bind(*x),
            BindVal::Str(x) => q.bind(x),
        };
    }
    let rows = q.fetch_all(state.db.pool()).await.map_err(async_graphql::Error::from)?;
    Ok(rows)
}

/// Builds a filtered/ordered chapter query. Supports condition, filter
/// (incl. `lastReadAt.notEqualToAll` / `isNull` used by the WebUI history
/// page), multi-column order, and first/offset pagination.
async fn query_chapters(
    state: &GraphQLState,
    condition: Option<&ChapterCondition>,
    filter: Option<&ChapterFilterInput>,
    order: Option<&Vec<ChapterOrderInput>>,
    first: Option<i32>,
    offset: Option<i32>,
) -> async_graphql::Result<Vec<ChapterRow>> {
    let mut sql = "SELECT * FROM chapter".to_string();
    let mut where_clauses: Vec<String> = Vec::new();
    let mut binds: Vec<BindVal> = Vec::new();

    if let Some(cond) = condition {
        if let Some(v) = cond.manga_id {
            where_clauses.push("manga = ?".into());
            binds.push(BindVal::I32(v));
        }
        if let Some(v) = cond.id {
            where_clauses.push("id = ?".into());
            binds.push(BindVal::I32(v));
        }
        if let Some(v) = cond.source_order {
            where_clauses.push("source_order = ?".into());
            binds.push(BindVal::I32(v));
        }
    }
    if let Some(f) = filter {
        build_chapter_filter(&mut where_clauses, &mut binds, f);
    }
    if !where_clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_clauses.join(" AND "));
    }
    // ordering
    if let Some(orders) = order {
        if !orders.is_empty() {
            let parts: Vec<String> = orders
                .iter()
                .map(|o| {
                    let col = match o.by {
                        ChapterOrderBy::Id => "id",
                        ChapterOrderBy::MangaId => "manga",
                        ChapterOrderBy::SourceOrder => "source_order",
                        ChapterOrderBy::UploadDate => "date_upload",
                        ChapterOrderBy::FetchedAt => "fetched_at",
                        ChapterOrderBy::ChapterNumber => "chapter_number",
                        ChapterOrderBy::LastReadAt => "last_read_at",
                        ChapterOrderBy::Name => "name",
                    };
                    let dir = match o.by_type {
                        Some(SortOrder::Asc) => "ASC",
                        _ => "DESC",
                    };
                    format!("{col} {dir}")
                })
                .collect();
            sql.push_str(&format!(" ORDER BY {}", parts.join(", ")));
        }
    } else {
        sql.push_str(" ORDER BY source_order DESC");
    }
    if let Some(limit) = first {
        let limit = limit.clamp(1, 500);
        sql.push_str(&format!(" LIMIT {limit}"));
    }
    if let Some(off) = offset {
        if off > 0 {
            sql.push_str(&format!(" OFFSET {off}"));
        }
    }
    let sql = bind_placeholders(&sql);
    fetch_chapters(state, &sql, &binds).await
}

/// Numeric (Long/Int/Double) filter ops -> SQL fragments, mirroring the
/// upstream `FilterInput` semantics.
trait NumericFilterOps {
    fn eq(&self) -> Option<BindVal>;
    fn neq(&self) -> Option<BindVal>;
    fn neq_all(&self) -> Option<Vec<BindVal>>;
    fn in_v(&self) -> Option<Vec<BindVal>>;
    fn not_in_v(&self) -> Option<Vec<BindVal>>;
    fn gt(&self) -> Option<BindVal>;
    fn gte(&self) -> Option<BindVal>;
    fn lt(&self) -> Option<BindVal>;
    fn lte(&self) -> Option<BindVal>;
    fn is_null(&self) -> Option<bool>;
}

impl NumericFilterOps for LongFilterInput {
    fn eq(&self) -> Option<BindVal> {
        self.equal_to.map(|v| BindVal::I64(v.0))
    }
    fn neq(&self) -> Option<BindVal> {
        self.not_equal_to.map(|v| BindVal::I64(v.0))
    }
    fn neq_all(&self) -> Option<Vec<BindVal>> {
        self.not_equal_to_all.as_ref().map(|vs| vs.iter().map(|v| BindVal::I64(v.0)).collect())
    }
    fn in_v(&self) -> Option<Vec<BindVal>> {
        self.in_.as_ref().map(|vs| vs.iter().map(|v| BindVal::I64(v.0)).collect())
    }
    fn not_in_v(&self) -> Option<Vec<BindVal>> {
        self.not_in.as_ref().map(|vs| vs.iter().map(|v| BindVal::I64(v.0)).collect())
    }
    fn gt(&self) -> Option<BindVal> {
        self.greater_than.map(|v| BindVal::I64(v.0))
    }
    fn gte(&self) -> Option<BindVal> {
        self.greater_than_or_equal_to.map(|v| BindVal::I64(v.0))
    }
    fn lt(&self) -> Option<BindVal> {
        self.less_than.map(|v| BindVal::I64(v.0))
    }
    fn lte(&self) -> Option<BindVal> {
        self.less_than_or_equal_to.map(|v| BindVal::I64(v.0))
    }
    fn is_null(&self) -> Option<bool> {
        self.is_null
    }
}

impl NumericFilterOps for IntFilterInput {
    fn eq(&self) -> Option<BindVal> {
        self.equal_to.map(BindVal::I32)
    }
    fn neq(&self) -> Option<BindVal> {
        self.not_equal_to.map(BindVal::I32)
    }
    fn neq_all(&self) -> Option<Vec<BindVal>> {
        self.not_equal_to_all.as_ref().map(|vs| vs.iter().copied().map(BindVal::I32).collect())
    }
    fn in_v(&self) -> Option<Vec<BindVal>> {
        self.in_.as_ref().map(|vs| vs.iter().copied().map(BindVal::I32).collect())
    }
    fn not_in_v(&self) -> Option<Vec<BindVal>> {
        self.not_in.as_ref().map(|vs| vs.iter().copied().map(BindVal::I32).collect())
    }
    fn gt(&self) -> Option<BindVal> {
        self.greater_than.map(BindVal::I32)
    }
    fn gte(&self) -> Option<BindVal> {
        self.greater_than_or_equal_to.map(BindVal::I32)
    }
    fn lt(&self) -> Option<BindVal> {
        self.less_than.map(BindVal::I32)
    }
    fn lte(&self) -> Option<BindVal> {
        self.less_than_or_equal_to.map(BindVal::I32)
    }
    fn is_null(&self) -> Option<bool> {
        self.is_null
    }
}

impl NumericFilterOps for DoubleFilterInput {
    fn eq(&self) -> Option<BindVal> {
        self.equal_to.map(BindVal::F64)
    }
    fn neq(&self) -> Option<BindVal> {
        self.not_equal_to.map(BindVal::F64)
    }
    fn neq_all(&self) -> Option<Vec<BindVal>> {
        self.not_equal_to_all.as_ref().map(|vs| vs.iter().copied().map(BindVal::F64).collect())
    }
    fn in_v(&self) -> Option<Vec<BindVal>> {
        self.in_.as_ref().map(|vs| vs.iter().copied().map(BindVal::F64).collect())
    }
    fn not_in_v(&self) -> Option<Vec<BindVal>> {
        self.not_in.as_ref().map(|vs| vs.iter().copied().map(BindVal::F64).collect())
    }
    fn gt(&self) -> Option<BindVal> {
        self.greater_than.map(BindVal::F64)
    }
    fn gte(&self) -> Option<BindVal> {
        self.greater_than_or_equal_to.map(BindVal::F64)
    }
    fn lt(&self) -> Option<BindVal> {
        self.less_than.map(BindVal::F64)
    }
    fn lte(&self) -> Option<BindVal> {
        self.less_than_or_equal_to.map(BindVal::F64)
    }
    fn is_null(&self) -> Option<bool> {
        self.is_null
    }
}

fn build_numeric_filter<T: NumericFilterOps>(
    where_clauses: &mut Vec<String>,
    binds: &mut Vec<BindVal>,
    col: &str,
    f: &T,
) {
    if let Some(v) = f.eq() {
        where_clauses.push(format!("{col} = ?"));
        binds.push(v);
    }
    if let Some(v) = f.neq() {
        where_clauses.push(format!("{col} != ?"));
        binds.push(v);
    }
    if let Some(vs) = f.neq_all() {
        if !vs.is_empty() {
            let ph = vec!["?"; vs.len()].join(", ");
            where_clauses.push(format!("{col} NOT IN ({ph})"));
            binds.extend(vs);
        }
    }
    if let Some(vs) = f.in_v() {
        if !vs.is_empty() {
            let ph = vec!["?"; vs.len()].join(", ");
            where_clauses.push(format!("{col} IN ({ph})"));
            binds.extend(vs);
        }
    }
    if let Some(vs) = f.not_in_v() {
        if !vs.is_empty() {
            let ph = vec!["?"; vs.len()].join(", ");
            where_clauses.push(format!("{col} NOT IN ({ph})"));
            binds.extend(vs);
        }
    }
    if let Some(v) = f.gt() {
        where_clauses.push(format!("{col} > ?"));
        binds.push(v);
    }
    if let Some(v) = f.gte() {
        where_clauses.push(format!("{col} >= ?"));
        binds.push(v);
    }
    if let Some(v) = f.lt() {
        where_clauses.push(format!("{col} < ?"));
        binds.push(v);
    }
    if let Some(v) = f.lte() {
        where_clauses.push(format!("{col} <= ?"));
        binds.push(v);
    }
    if let Some(null) = f.is_null() {
        where_clauses.push(if null {
            format!("{col} IS NULL")
        } else {
            format!("{col} IS NOT NULL")
        });
    }
}

fn build_bool_filter(where_clauses: &mut Vec<String>, binds: &mut Vec<BindVal>, col: &str, f: &BooleanFilterInput) {
    if let Some(v) = f.equal_to {
        where_clauses.push(format!("{col} = ?"));
        binds.push(BindVal::Bool(v));
    }
    if let Some(v) = f.not_equal_to {
        where_clauses.push(format!("{col} != ?"));
        binds.push(BindVal::Bool(v));
    }
    if let Some(null) = f.is_null {
        where_clauses.push(if null {
            format!("{col} IS NULL")
        } else {
            format!("{col} IS NOT NULL")
        });
    }
}

/// `col OP ?` 字符串比较。
fn push_str_cmp(w: &mut Vec<String>, b: &mut Vec<BindVal>, col: &str, op: &str, v: &str) {
    w.push(format!("{col} {op} ?"));
    b.push(BindVal::Str(v.to_string()));
}

/// 大小写不敏感比较：`LOWER(col) OP LOWER(?)`。
fn push_str_cmp_insensitive(w: &mut Vec<String>, b: &mut Vec<BindVal>, col: &str, op: &str, v: &str) {
    w.push(format!("LOWER({col}) {op} LOWER(?)"));
    b.push(BindVal::Str(v.to_string()));
}

/// `col [NOT] LIKE/ILIKE 'left{value}right'`（单值，AND 语义）。
fn push_like(w: &mut Vec<String>, b: &mut Vec<BindVal>, col: &str, v: &str, not: bool, insensitive: bool, left: &str, right: &str) {
    let kw = if insensitive { "ILIKE" } else { "LIKE" };
    let neg = if not { "NOT " } else { "" };
    w.push(format!("{col} {neg}{kw} ?"));
    b.push(BindVal::Str(format!("{left}{v}{right}")));
}

/// 多值 ALL：每个值一条 clause（调用方 AND 连接）。
fn push_like_all(w: &mut Vec<String>, b: &mut Vec<BindVal>, col: &str, vs: &[String], not: bool, insensitive: bool, left: &str, right: &str) {
    for v in vs {
        push_like(w, b, col, v, not, insensitive, left, right);
    }
}

/// 多值 ANY：OR 组合成单条 clause。
fn push_like_any(w: &mut Vec<String>, b: &mut Vec<BindVal>, col: &str, vs: &[String], not: bool, insensitive: bool, left: &str, right: &str) {
    if vs.is_empty() {
        return;
    }
    let kw = if insensitive { "ILIKE" } else { "LIKE" };
    let neg = if not { "NOT " } else { "" };
    let parts: Vec<String> = vs.iter().map(|_| format!("{col} {neg}{kw} ?")).collect();
    w.push(format!("({})", parts.join(" OR ")));
    for v in vs {
        b.push(BindVal::Str(format!("{left}{v}{right}")));
    }
}

/// `col [NOT] IN (...)`，insensitive 时两侧 LOWER。
fn push_in_list(w: &mut Vec<String>, b: &mut Vec<BindVal>, col: &str, vs: &[String], not: bool, insensitive: bool) {
    if vs.is_empty() {
        return;
    }
    let ph = vec!["?"; vs.len()].join(", ");
    let neg = if not { "NOT " } else { "" };
    if insensitive {
        w.push(format!("LOWER({col}) {neg}IN ({ph})"));
        b.extend(vs.iter().map(|v| BindVal::Str(v.to_lowercase())));
    } else {
        w.push(format!("{col} {neg}IN ({ph})"));
        b.extend(vs.iter().cloned().map(BindVal::Str));
    }
}

fn build_string_filter(where_clauses: &mut Vec<String>, binds: &mut Vec<BindVal>, col: &str, f: &StringFilterInput) {
    // 相等 / 不等
    if let Some(v) = &f.equal_to {
        push_str_cmp(where_clauses, binds, col, "=", v);
    }
    if let Some(v) = &f.not_equal_to {
        push_str_cmp(where_clauses, binds, col, "!=", v);
    }
    if let Some(v) = &f.not_equal_to_insensitive {
        push_str_cmp_insensitive(where_clauses, binds, col, "!=", v);
    }
    if let Some(vs) = &f.not_equal_to_all {
        for v in vs {
            push_str_cmp(where_clauses, binds, col, "!=", v);
        }
    }
    if let Some(vs) = &f.not_equal_to_any {
        if !vs.is_empty() {
            let parts: Vec<String> = vs.iter().map(|_| format!("{col} != ?")).collect();
            where_clauses.push(format!("({})", parts.join(" OR ")));
            binds.extend(vs.iter().cloned().map(BindVal::Str));
        }
    }
    if let Some(vs) = &f.not_equal_to_insensitive_all {
        for v in vs {
            push_str_cmp_insensitive(where_clauses, binds, col, "!=", v);
        }
    }
    if let Some(vs) = &f.not_equal_to_insensitive_any {
        if !vs.is_empty() {
            let parts: Vec<String> = vs.iter().map(|_| format!("LOWER({col}) != LOWER(?)")).collect();
            where_clauses.push(format!("({})", parts.join(" OR ")));
            binds.extend(vs.iter().cloned().map(BindVal::Str));
        }
    }
    // 比较（> < >= <=，可选大小写不敏感）
    if let Some(v) = &f.greater_than {
        push_str_cmp(where_clauses, binds, col, ">", v);
    }
    if let Some(v) = &f.greater_than_insensitive {
        push_str_cmp_insensitive(where_clauses, binds, col, ">", v);
    }
    if let Some(v) = &f.greater_than_or_equal_to {
        push_str_cmp(where_clauses, binds, col, ">=", v);
    }
    if let Some(v) = &f.greater_than_or_equal_to_insensitive {
        push_str_cmp_insensitive(where_clauses, binds, col, ">=", v);
    }
    if let Some(v) = &f.less_than {
        push_str_cmp(where_clauses, binds, col, "<", v);
    }
    if let Some(v) = &f.less_than_insensitive {
        push_str_cmp_insensitive(where_clauses, binds, col, "<", v);
    }
    if let Some(v) = &f.less_than_or_equal_to {
        push_str_cmp(where_clauses, binds, col, "<=", v);
    }
    if let Some(v) = &f.less_than_or_equal_to_insensitive {
        push_str_cmp_insensitive(where_clauses, binds, col, "<=", v);
    }
    // distinct_from（!=） / not_distinct_from（=）
    if let Some(v) = &f.distinct_from {
        push_str_cmp(where_clauses, binds, col, "!=", v);
    }
    if let Some(v) = &f.distinct_from_insensitive {
        push_str_cmp_insensitive(where_clauses, binds, col, "!=", v);
    }
    if let Some(vs) = &f.distinct_from_all {
        for v in vs {
            push_str_cmp(where_clauses, binds, col, "!=", v);
        }
    }
    if let Some(vs) = &f.distinct_from_any {
        if !vs.is_empty() {
            let parts: Vec<String> = vs.iter().map(|_| format!("{col} != ?")).collect();
            where_clauses.push(format!("({})", parts.join(" OR ")));
            binds.extend(vs.iter().cloned().map(BindVal::Str));
        }
    }
    if let Some(vs) = &f.distinct_from_insensitive_all {
        for v in vs {
            push_str_cmp_insensitive(where_clauses, binds, col, "!=", v);
        }
    }
    if let Some(vs) = &f.distinct_from_insensitive_any {
        if !vs.is_empty() {
            let parts: Vec<String> = vs.iter().map(|_| format!("LOWER({col}) != LOWER(?)")).collect();
            where_clauses.push(format!("({})", parts.join(" OR ")));
            binds.extend(vs.iter().cloned().map(BindVal::Str));
        }
    }
    if let Some(v) = &f.not_distinct_from {
        push_str_cmp(where_clauses, binds, col, "=", v);
    }
    if let Some(v) = &f.not_distinct_from_insensitive {
        push_str_cmp_insensitive(where_clauses, binds, col, "=", v);
    }
    // includes（包含）
    if let Some(v) = &f.includes {
        push_like(where_clauses, binds, col, v, false, false, "%", "%");
    }
    if let Some(v) = &f.includes_insensitive {
        push_like(where_clauses, binds, col, v, false, true, "%", "%");
    }
    if let Some(vs) = &f.includes_all {
        push_like_all(where_clauses, binds, col, vs, false, false, "%", "%");
    }
    if let Some(vs) = &f.includes_any {
        push_like_any(where_clauses, binds, col, vs, false, false, "%", "%");
    }
    if let Some(vs) = &f.includes_insensitive_all {
        push_like_all(where_clauses, binds, col, vs, false, true, "%", "%");
    }
    if let Some(vs) = &f.includes_insensitive_any {
        push_like_any(where_clauses, binds, col, vs, false, true, "%", "%");
    }
    // like
    if let Some(v) = &f.like {
        push_like(where_clauses, binds, col, v, false, false, "%", "%");
    }
    if let Some(v) = &f.like_insensitive {
        push_like(where_clauses, binds, col, v, false, true, "%", "%");
    }
    if let Some(vs) = &f.like_all {
        push_like_all(where_clauses, binds, col, vs, false, false, "%", "%");
    }
    if let Some(vs) = &f.like_any {
        push_like_any(where_clauses, binds, col, vs, false, false, "%", "%");
    }
    if let Some(vs) = &f.like_insensitive_all {
        push_like_all(where_clauses, binds, col, vs, false, true, "%", "%");
    }
    if let Some(vs) = &f.like_insensitive_any {
        push_like_any(where_clauses, binds, col, vs, false, true, "%", "%");
    }
    // not like
    if let Some(v) = &f.not_like {
        push_like(where_clauses, binds, col, v, true, false, "%", "%");
    }
    if let Some(v) = &f.not_like_insensitive {
        push_like(where_clauses, binds, col, v, true, true, "%", "%");
    }
    if let Some(vs) = &f.not_like_all {
        push_like_all(where_clauses, binds, col, vs, true, false, "%", "%");
    }
    if let Some(vs) = &f.not_like_any {
        push_like_any(where_clauses, binds, col, vs, true, false, "%", "%");
    }
    if let Some(vs) = &f.not_like_insensitive_all {
        push_like_all(where_clauses, binds, col, vs, true, true, "%", "%");
    }
    if let Some(vs) = &f.not_like_insensitive_any {
        push_like_any(where_clauses, binds, col, vs, true, true, "%", "%");
    }
    // starts_with / ends_with
    if let Some(v) = &f.starts_with {
        push_like(where_clauses, binds, col, v, false, false, "", "%");
    }
    if let Some(v) = &f.starts_with_insensitive {
        push_like(where_clauses, binds, col, v, false, true, "", "%");
    }
    if let Some(vs) = &f.starts_with_all {
        push_like_all(where_clauses, binds, col, vs, false, false, "", "%");
    }
    if let Some(vs) = &f.starts_with_any {
        push_like_any(where_clauses, binds, col, vs, false, false, "", "%");
    }
    if let Some(vs) = &f.starts_with_insensitive_all {
        push_like_all(where_clauses, binds, col, vs, false, true, "", "%");
    }
    if let Some(vs) = &f.starts_with_insensitive_any {
        push_like_any(where_clauses, binds, col, vs, false, true, "", "%");
    }
    if let Some(v) = &f.ends_with {
        push_like(where_clauses, binds, col, v, false, false, "%", "");
    }
    if let Some(v) = &f.ends_with_insensitive {
        push_like(where_clauses, binds, col, v, false, true, "%", "");
    }
    if let Some(vs) = &f.ends_with_all {
        push_like_all(where_clauses, binds, col, vs, false, false, "%", "");
    }
    if let Some(vs) = &f.ends_with_any {
        push_like_any(where_clauses, binds, col, vs, false, false, "%", "");
    }
    if let Some(vs) = &f.ends_with_insensitive_all {
        push_like_all(where_clauses, binds, col, vs, false, true, "%", "");
    }
    if let Some(vs) = &f.ends_with_insensitive_any {
        push_like_any(where_clauses, binds, col, vs, false, true, "%", "");
    }
    // not starts_with / not ends_with
    if let Some(v) = &f.not_starts_with {
        push_like(where_clauses, binds, col, v, true, false, "", "%");
    }
    if let Some(v) = &f.not_starts_with_insensitive {
        push_like(where_clauses, binds, col, v, true, true, "", "%");
    }
    if let Some(vs) = &f.not_starts_with_all {
        push_like_all(where_clauses, binds, col, vs, true, false, "", "%");
    }
    if let Some(vs) = &f.not_starts_with_any {
        push_like_any(where_clauses, binds, col, vs, true, false, "", "%");
    }
    if let Some(vs) = &f.not_starts_with_insensitive_all {
        push_like_all(where_clauses, binds, col, vs, true, true, "", "%");
    }
    if let Some(vs) = &f.not_starts_with_insensitive_any {
        push_like_any(where_clauses, binds, col, vs, true, true, "", "%");
    }
    if let Some(v) = &f.not_ends_with {
        push_like(where_clauses, binds, col, v, true, false, "%", "");
    }
    if let Some(v) = &f.not_ends_with_insensitive {
        push_like(where_clauses, binds, col, v, true, true, "%", "");
    }
    if let Some(vs) = &f.not_ends_with_all {
        push_like_all(where_clauses, binds, col, vs, true, false, "%", "");
    }
    if let Some(vs) = &f.not_ends_with_any {
        push_like_any(where_clauses, binds, col, vs, true, false, "%", "");
    }
    if let Some(vs) = &f.not_ends_with_insensitive_all {
        push_like_all(where_clauses, binds, col, vs, true, true, "%", "");
    }
    if let Some(vs) = &f.not_ends_with_insensitive_any {
        push_like_any(where_clauses, binds, col, vs, true, true, "%", "");
    }
    // in / not in
    if let Some(vs) = &f.in_ {
        push_in_list(where_clauses, binds, col, vs, false, false);
    }
    if let Some(vs) = &f.in_insensitive {
        push_in_list(where_clauses, binds, col, vs, false, true);
    }
    if let Some(vs) = &f.not_in {
        push_in_list(where_clauses, binds, col, vs, true, false);
    }
    if let Some(vs) = &f.not_in_insensitive {
        push_in_list(where_clauses, binds, col, vs, true, true);
    }
    // is null
    if let Some(null) = f.is_null {
        where_clauses.push(if null {
            format!("{col} IS NULL")
        } else {
            format!("{col} IS NOT NULL")
        });
    }
}

/// Translates a `ChapterFilterInput` into SQL WHERE fragments (AND-combined,
/// with `and`/`or`/`not` logical composition).
fn build_chapter_filter(where_clauses: &mut Vec<String>, binds: &mut Vec<BindVal>, f: &ChapterFilterInput) {
    if let Some(v) = &f.chapter_number {
        build_numeric_filter(where_clauses, binds, "chapter_number", v);
    }
    if let Some(v) = &f.fetched_at {
        build_numeric_filter(where_clauses, binds, "fetched_at", v);
    }
    if let Some(v) = &f.id {
        build_numeric_filter(where_clauses, binds, "id", v);
    }
    if let Some(v) = &f.in_library {
        if let Some(b) = v.equal_to {
            where_clauses.push("manga IN (SELECT id FROM manga WHERE in_library = ?)".into());
            binds.push(BindVal::Bool(b));
        }
    }
    if let Some(v) = &f.is_bookmarked {
        build_bool_filter(where_clauses, binds, "bookmark", v);
    }
    if let Some(v) = &f.is_downloaded {
        build_bool_filter(where_clauses, binds, "is_downloaded", v);
    }
    if let Some(v) = &f.is_read {
        build_bool_filter(where_clauses, binds, "read", v);
    }
    if let Some(v) = &f.last_page_read {
        build_numeric_filter(where_clauses, binds, "last_page_read", v);
    }
    if let Some(v) = &f.last_read_at {
        build_numeric_filter(where_clauses, binds, "last_read_at", v);
    }
    if let Some(v) = &f.manga_id {
        build_numeric_filter(where_clauses, binds, "manga", v);
    }
    if let Some(v) = &f.name {
        build_string_filter(where_clauses, binds, "name", v);
    }
    if let Some(v) = &f.page_count {
        build_numeric_filter(where_clauses, binds, "page_count", v);
    }
    if let Some(v) = &f.real_url {
        build_string_filter(where_clauses, binds, "real_url", v);
    }
    if let Some(v) = &f.scanlator {
        build_string_filter(where_clauses, binds, "scanlator", v);
    }
    if let Some(v) = &f.source_order {
        build_numeric_filter(where_clauses, binds, "source_order", v);
    }
    if let Some(v) = &f.upload_date {
        build_numeric_filter(where_clauses, binds, "date_upload", v);
    }
    if let Some(v) = &f.url {
        build_string_filter(where_clauses, binds, "url", v);
    }
    // logical composition
    if let Some(ands) = &f.and {
        let mut inner: Vec<String> = Vec::new();
        for sub in ands {
            let mut sub_binds: Vec<BindVal> = Vec::new();
            let mut sub_clauses: Vec<String> = Vec::new();
            build_chapter_filter(&mut sub_clauses, &mut sub_binds, sub);
            if !sub_clauses.is_empty() {
                inner.push(format!("({})", sub_clauses.join(" AND ")));
                binds.extend(sub_binds);
            }
        }
        if !inner.is_empty() {
            where_clauses.push(format!("({})", inner.join(" AND ")));
        }
    }
    if let Some(ors) = &f.or {
        let mut inner: Vec<String> = Vec::new();
        for sub in ors {
            let mut sub_binds: Vec<BindVal> = Vec::new();
            let mut sub_clauses: Vec<String> = Vec::new();
            build_chapter_filter(&mut sub_clauses, &mut sub_binds, sub);
            if !sub_clauses.is_empty() {
                inner.push(format!("({})", sub_clauses.join(" AND ")));
                binds.extend(sub_binds);
            }
        }
        if !inner.is_empty() {
            where_clauses.push(format!("({})", inner.join(" OR ")));
        }
    }
    if let Some(not) = &f.not {
        let mut sub_binds: Vec<BindVal> = Vec::new();
        let mut sub_clauses: Vec<String> = Vec::new();
        build_chapter_filter(&mut sub_clauses, &mut sub_binds, not);
        if !sub_clauses.is_empty() {
            where_clauses.push(format!("NOT ({})", sub_clauses.join(" AND ")));
            binds.extend(sub_binds);
        }
    }
}

/// Translates a `MangaFilterInput` into SQL WHERE fragments (AND-combined,
/// with `and`/`or`/`not` logical composition).
fn build_manga_filter(where_clauses: &mut Vec<String>, binds: &mut Vec<BindVal>, f: &MangaFilterInput) {
    if let Some(v) = &f.artist {
        build_string_filter(where_clauses, binds, "artist", v);
    }
    if let Some(v) = &f.author {
        build_string_filter(where_clauses, binds, "author", v);
    }
    if let Some(v) = &f.category_id {
        if let Some(cid) = v.equal_to {
            where_clauses.push("id IN (SELECT manga FROM category_manga WHERE category = ?)".into());
            binds.push(BindVal::I32(cid));
        }
    }
    if let Some(v) = &f.chapters_last_fetched_at {
        build_numeric_filter(where_clauses, binds, "chapters_last_fetched_at", v);
    }
    if let Some(v) = &f.description {
        build_string_filter(where_clauses, binds, "description", v);
    }
    if let Some(v) = &f.genre {
        build_string_filter(where_clauses, binds, "genre", v);
    }
    if let Some(v) = &f.id {
        build_numeric_filter(where_clauses, binds, "id", v);
    }
    if let Some(v) = &f.in_library {
        build_bool_filter(where_clauses, binds, "in_library", v);
    }
    if let Some(v) = &f.in_library_at {
        build_numeric_filter(where_clauses, binds, "in_library_at", v);
    }
    if let Some(v) = &f.initialized {
        build_bool_filter(where_clauses, binds, "initialized", v);
    }
    if let Some(v) = &f.last_fetched_at {
        build_numeric_filter(where_clauses, binds, "last_fetched_at", v);
    }
    if let Some(v) = &f.real_url {
        build_string_filter(where_clauses, binds, "real_url", v);
    }
    if let Some(v) = &f.source_id {
        build_numeric_filter(where_clauses, binds, "source", v);
    }
    if let Some(v) = &f.status {
        // MangaStatusFilterInput — enum filter (equality + null only).
        if let Some(s) = v.equal_to {
            where_clauses.push("status = ?".into());
            binds.push(BindVal::I32(s.to_i32()));
        }
        if let Some(s) = v.not_equal_to {
            where_clauses.push("status != ?".into());
            binds.push(BindVal::I32(s.to_i32()));
        }
        if let Some(null) = v.is_null {
            where_clauses.push(if null {
                "status IS NULL".into()
            } else {
                "status IS NOT NULL".into()
            });
        }
    }
    if let Some(v) = &f.thumbnail_url {
        build_string_filter(where_clauses, binds, "thumbnail_url", v);
    }
    if let Some(v) = &f.title {
        build_string_filter(where_clauses, binds, "title", v);
    }
    if let Some(v) = &f.url {
        build_string_filter(where_clauses, binds, "url", v);
    }
    // logical composition
    if let Some(ands) = &f.and {
        let mut inner: Vec<String> = Vec::new();
        for sub in ands {
            let mut sub_binds: Vec<BindVal> = Vec::new();
            let mut sub_clauses: Vec<String> = Vec::new();
            build_manga_filter(&mut sub_clauses, &mut sub_binds, sub);
            if !sub_clauses.is_empty() {
                inner.push(format!("({})", sub_clauses.join(" AND ")));
                binds.extend(sub_binds);
            }
        }
        if !inner.is_empty() {
            where_clauses.push(format!("({})", inner.join(" AND ")));
        }
    }
    if let Some(ors) = &f.or {
        let mut inner: Vec<String> = Vec::new();
        for sub in ors {
            let mut sub_binds: Vec<BindVal> = Vec::new();
            let mut sub_clauses: Vec<String> = Vec::new();
            build_manga_filter(&mut sub_clauses, &mut sub_binds, sub);
            if !sub_clauses.is_empty() {
                inner.push(format!("({})", sub_clauses.join(" AND ")));
                binds.extend(sub_binds);
            }
        }
        if !inner.is_empty() {
            where_clauses.push(format!("({})", inner.join(" OR ")));
        }
    }
    if let Some(not) = &f.not {
        let mut sub_binds: Vec<BindVal> = Vec::new();
        let mut sub_clauses: Vec<String> = Vec::new();
        build_manga_filter(&mut sub_clauses, &mut sub_binds, not);
        if !sub_clauses.is_empty() {
            where_clauses.push(format!("NOT ({})", sub_clauses.join(" AND ")));
            binds.extend(sub_binds);
        }
    }
}

#[allow(dead_code)]
impl MangaStatusExt for MangaStatus {
    fn to_i32(&self) -> i32 {
        match self {
            Self::Unknown => 0,
            Self::Ongoing => 1,
            Self::Completed => 2,
            Self::Licensed => 3,
            Self::PublishingFinished => 4,
            Self::Cancelled => 5,
            Self::OnHiatus => 6,
        }
    }
}

trait MangaStatusExt {
    fn to_i32(&self) -> i32;
}

/// Locally deployed WebUI version from `<webui_dir>/revision` (e.g. `r3482`).
pub(crate) fn local_webui_version(dir: &std::path::Path) -> String {
    // version.txt 第一行为版本号（WebUI CI 构建时写入）；旧产物回退 revision
    if let Ok(content) = std::fs::read_to_string(dir.join("version.txt")) {
        if let Some(tag) = content.lines().next() {
            let tag = tag.trim().to_string();
            if !tag.is_empty() {
                return tag;
            }
        }
    }
    std::fs::read_to_string(dir.join("revision"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// version.txt 第二行为通道（Alpha/Beta/Release），映射到 WebUIChannel。
fn local_webui_channel(dir: &std::path::Path) -> crate::settings::WebUIChannel {
    use crate::settings::WebUIChannel;
    if let Ok(content) = std::fs::read_to_string(dir.join("version.txt")) {
        match content.lines().nth(1).unwrap_or("").trim() {
            "Release" => return WebUIChannel::Stable,
            _ => return WebUIChannel::Preview, // Alpha / Beta / 未知
        }
    }
    WebUIChannel::Stable
}

/// version.txt 第三行为构建时间戳（Unix 秒），缺省返回 0。
fn local_webui_build_time(dir: &std::path::Path) -> LongString {
    if let Ok(content) = std::fs::read_to_string(dir.join("version.txt")) {
        if let Some(ts) = content.lines().nth(2) {
            if let Ok(secs) = ts.trim().parse::<i64>() {
                return LongString(secs);
            }
        }
    }
    LongString(0)
}

/// `r3482` → 3482 for numeric comparison (plain string compare breaks on
/// `r349` vs `r3480`).
fn tag_to_num(tag: &str) -> i64 {
    tag.chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

/// Proxy candidates for GitHub API calls (api.github.com often 403s on direct
/// connections from CN networks). Tries env proxy first, then common local
/// proxy ports (Clash/Clash Verge/v2ray), then direct.
fn github_proxy_candidates() -> Vec<Option<String>> {
    let mut out = Vec::new();
    for key in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"] {
        if let Ok(v) = std::env::var(key) {
            if !v.trim().is_empty() {
                out.push(Some(v));
            }
        }
    }
    for port in [7890u16, 7897, 10809, 1080] {
        out.push(Some(format!("http://127.0.0.1:{port}")));
    }
    out.push(None); // direct last
    out
}

/// GET with the proxy fallback chain; returns the first successful response.
pub(crate) async fn github_get_with_fallback(url: &str) -> Result<reqwest::Response, String> {
    let mut last_err = String::new();
    for proxy in github_proxy_candidates() {
        let mut builder = reqwest::Client::builder().user_agent("Suwayomi-next/1.0");
        if let Some(p) = proxy.as_deref() {
            match reqwest::Proxy::all(p) {
                Ok(proxy) => {
                    builder = builder.proxy(proxy);
                }
                Err(e) => {
                    last_err = format!("proxy {p}: {e}");
                    continue;
                }
            }
        }
        let client = match builder.build() {
            Ok(c) => c,
            Err(e) => {
                last_err = e.to_string();
                continue;
            }
        };
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(resp),
            Ok(resp) => last_err = format!("{} {}", url, resp.status()),
            Err(e) => last_err = format!("{url}: {e}"),
        }
    }
    Err(last_err)
}

/// Latest 576576/Suwayomi-WebUI release: `(tag, asset_download_url)`.
///
/// Prefers the HTML `releases/latest` page (follows the redirect to
/// `/releases/tag/{tag}` and reads the tag from the final URL) — the GitHub
/// *API* is rate-limited (403 `API rate limit exceeded`) on shared/CN exit
/// IPs, while the website is not. The download URL is then derived from the
/// known asset naming scheme `Suwayomi-WebUI-{tag}.zip`. Falls back to the
/// API only if the page fetch fails entirely.
pub(crate) async fn fetch_latest_webui_release() -> Result<(String, String), String> {
    // 1) HTML page: tag from the redirect target URL
    if let Ok(resp) = github_get_with_fallback("https://github.com/576576/Suwayomi-WebUI/releases/latest").await {
        if let Some(tag) = resp.url().path_segments().and_then(|mut s| s.next_back()) {
            let tag = tag.to_string();
            if !tag.is_empty() && tag != "latest" {
                let url = format!("https://github.com/576576/Suwayomi-WebUI/releases/download/{tag}/Suwayomi-WebUI-{tag}.zip");
                return Ok((tag, url));
            }
        }
    }
    // 2) API fallback
    let resp = github_get_with_fallback("https://api.github.com/repos/576576/Suwayomi-WebUI/releases/latest").await?;
    let j: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let tag = j["tag_name"].as_str().unwrap_or("").to_string();
    let url = j["assets"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|a| a["browser_download_url"].as_str())
        .unwrap_or("")
        .to_string();
    if tag.is_empty() || url.is_empty() {
        return Err("no release asset".into());
    }
    Ok((tag, url))
}

/// Latest 576576/Suwayomi-next release: `(tag, release_page_url)`.
///
/// The `releases/latest` shortcut redirects to the plain `/releases` list
/// because every next release is a pre-release, so parse the first
/// `releases/tag/r\d+` link from the list page HTML (the GitHub API is
/// rate-limited on shared/CN exit IPs). Falls back to the API list.
pub(crate) async fn fetch_latest_server_release() -> Result<(String, String), String> {
    // 1) HTML list page: first `releases/tag/rNNNN` link
    if let Ok(resp) = github_get_with_fallback("https://github.com/576576/Suwayomi-next/releases").await {
        if let Ok(html) = resp.text().await {
            let marker = "/576576/Suwayomi-next/releases/tag/r";
            if let Some(i) = html.find(marker) {
                let rest = &html[i + marker.len()..];
                let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() {
                    let tag = format!("r{digits}");
                    let url = format!("https://github.com/576576/Suwayomi-next/releases/tag/{tag}");
                    return Ok((tag, url));
                }
            }
        }
    }
    // 2) API fallback
    let resp = github_get_with_fallback("https://api.github.com/repos/576576/Suwayomi-next/releases?per_page=1").await?;
    let j: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let tag = j[0]["tag_name"].as_str().unwrap_or("").to_string();
    let url = j[0]["html_url"].as_str().unwrap_or("").to_string();
    if tag.is_empty() {
        return Err("no release".into());
    }
    Ok((tag, url))
}

/// JVM info reported by the jvm-sandbox (`GET /jvm`), cached 60s; falls back
/// to "n/a" when the sandbox is absent or unreachable.
static JVM_CACHE: std::sync::OnceLock<std::sync::Mutex<(i64, crate::settings::JvmInfo)>> =
    std::sync::OnceLock::new();

async fn fetch_sandbox_jvm_info(sandbox_base: Option<&str>) -> crate::settings::JvmInfo {
    let now = chrono::Utc::now().timestamp();
    let cache = JVM_CACHE.get_or_init(|| std::sync::Mutex::new((0, crate::settings::JvmInfo {
        java_version: "n/a".into(),
        vm_name: "n/a".into(),
        vm_vendor: "n/a".into(),
        vm_version: "n/a".into(),
    })));
    if let Ok(guard) = cache.lock() {
        if guard.0 > now - 60 {
            return guard.1.clone();
        }
    }
    let fallback = || crate::settings::JvmInfo {
        java_version: "n/a".into(),
        vm_name: "n/a".into(),
        vm_vendor: "n/a".into(),
        vm_version: "n/a".into(),
    };
    let Some(base) = sandbox_base else { return fallback() };
    let client = match reqwest::Client::builder().user_agent("Suwayomi-next/1.0").build() {
        Ok(c) => c,
        Err(_) => return fallback(),
    };
    let resp = client
        .get(format!("{base}/jvm"))
        .timeout(std::time::Duration::from_millis(2000))
        .send()
        .await;
    let info = match resp {
        Ok(r) if r.status().is_success() => match r.json::<serde_json::Value>().await {
            Ok(j) => crate::settings::JvmInfo {
                java_version: j["javaVersion"].as_str().unwrap_or("n/a").to_string(),
                vm_name: j["vmName"].as_str().unwrap_or("n/a").to_string(),
                vm_vendor: j["vmVendor"].as_str().unwrap_or("n/a").to_string(),
                vm_version: j["vmVersion"].as_str().unwrap_or("n/a").to_string(),
            },
            Err(_) => fallback(),
        },
        _ => fallback(),
    };
    if let Ok(mut guard) = cache.lock() {
        *guard = (now, info.clone());
    }
    info
}
