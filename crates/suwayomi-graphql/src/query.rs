//! Query root — mirrors `graphql/queries/*.kt`.
//! Core queries implemented; remaining queries land in later increments.

use async_graphql::{Context, Enum, InputObject, Object, SimpleObject};
use sqlx::Row;
use suwayomi_core::schema::{CategoryRow, ChapterRow, MangaRow};
use suwayomi_domain::sql::bind_placeholders;

use crate::scalars::Cursor;
use crate::state::GraphQLState;
use crate::types::*;

enum BindVal {
    I32(i32),
    I64(i64),
    Bool(bool),
    Str(String),
}

/// Mirrors `MangaCondition` from `MangaQuery.kt` (core filters).
#[derive(InputObject)]
pub struct MangaCondition {
    pub id: Option<i32>,
    pub source_id: Option<i64>,
    pub title: Option<String>,
    pub url: Option<String>,
    pub initialized: Option<bool>,
    pub artist: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub in_library: Option<bool>,
    pub status: Option<MangaStatus>,
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
pub struct ChapterCondition {
    pub manga_id: Option<i32>,
    pub id: Option<i32>,
    pub source_order: Option<i32>,
}

/// Mirrors `CategoryCondition` (core).
#[derive(InputObject, Default)]
pub struct CategoryCondition {
    pub id: Option<i32>,
    pub name: Option<String>,
    pub default: Option<bool>,
}

/// Mirrors `MetaCondition` from `MetaQuery.kt`.
#[derive(InputObject, Default)]
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
scalar_filter_input!(LongFilterInput, i64);
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
            pub ends_with: Option<String>,
            pub ends_with_all: Option<Vec<String>>,
            pub ends_with_any: Option<Vec<String>>,
            pub ends_with_insensitive: Option<String>,
            pub equal_to: Option<String>,
            pub greater_than: Option<String>,
            pub greater_than_or_equal_to: Option<String>,
            pub in_: Option<Vec<String>>,
            pub in_insensitive: Option<Vec<String>>,
            pub includes: Option<String>,
            pub includes_all: Option<Vec<String>>,
            pub includes_any: Option<Vec<String>>,
            pub includes_insensitive: Option<String>,
            pub is_null: Option<bool>,
            pub less_than: Option<String>,
            pub less_than_or_equal_to: Option<String>,
            pub not_distinct_from: Option<String>,
            pub not_distinct_from_insensitive: Option<String>,
            pub not_equal_to: Option<String>,
            pub not_equal_to_all: Option<Vec<String>>,
            pub not_equal_to_any: Option<Vec<String>>,
            pub not_equal_to_insensitive: Option<String>,
            pub not_in: Option<Vec<String>>,
            pub not_in_insensitive: Option<Vec<String>>,
            pub starts_with: Option<String>,
            pub starts_with_all: Option<Vec<String>>,
            pub starts_with_any: Option<Vec<String>>,
            pub starts_with_insensitive: Option<String>,
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

/// Root Query object.
#[derive(Default)]
pub struct QueryRoot;

#[Object]
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
        let _ = (filter, before, last, offset); // shape parity; applied incrementally
        let state = ctx.data::<GraphQLState>()?;
        let rows = query_mangas(state, condition.as_ref(), order.as_ref(), first, after.as_ref()).await?;
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
        let _ = (filter, order, before, after, first, last, offset); // shape parity
        let state = ctx.data::<GraphQLState>()?;
        let mut sql = "SELECT * FROM chapter".to_string();
        let mut binds: Vec<String> = Vec::new();
        let cond = condition.unwrap_or_default();
        let mut where_clauses: Vec<&str> = Vec::new();
        if let Some(v) = cond.manga_id {
            where_clauses.push("manga = ?");
            binds.push(v.to_string());
        }
        if let Some(v) = cond.id {
            where_clauses.push("id = ?");
            binds.push(v.to_string());
        }
        if let Some(v) = cond.source_order {
            where_clauses.push("source_order = ?");
            binds.push(v.to_string());
        }
        if !where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY source_order DESC");
        let sql = bind_placeholders(&sql);
        let rows = fetch_chapters(state, &sql, &binds).await?;
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

    /// Minimal placeholder for remaining queries (Phase 4 increments).
    async fn about_server(&self) -> AboutServerPayload {
        AboutServerPayload { name: "Suwayomi (next)".into(), version: env!("CARGO_PKG_VERSION").into() }
    }
}

/// Mirrors `CategoryOrderInput` (shape parity; currently unused by categories).
#[derive(InputObject)]
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
}

async fn query_mangas(
    state: &GraphQLState,
    condition: Option<&MangaCondition>,
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
            binds.push(BindVal::I64(v));
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
            BindVal::Str(x) => q.bind(x),
        };
    }
    let rows = q.fetch_all(state.db.pool()).await.map_err(async_graphql::Error::from)?;
    Ok(rows)
}

async fn fetch_chapters(state: &GraphQLState, sql: &str, binds: &[String]) -> async_graphql::Result<Vec<ChapterRow>> {
    let mut q = sqlx::query_as::<_, ChapterRow>(sql);
    for b in binds {
        if let Ok(v) = b.parse::<i32>() {
            q = q.bind(v);
        } else {
            q = q.bind(b.as_str());
        }
    }
    let rows = q.fetch_all(state.db.pool()).await.map_err(async_graphql::Error::from)?;
    Ok(rows)
}

/// Minimal payloads for unimplemented queries.
#[derive(SimpleObject, Clone)]
pub struct AboutServerPayload {
    pub name: String,
    pub version: String,
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
