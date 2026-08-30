//! Query root — mirrors `graphql/queries/*.kt`.
//! Core queries implemented; remaining queries land in later increments.

use async_graphql::{Context, Enum, InputObject, Object, SimpleObject, Union};
use suwayomi_core::schema::{CategoryRow, ChapterRow, MangaRow};
use suwayomi_domain::sql::bind_placeholders;

use crate::scalars::Cursor;

enum BindVal {
    I32(i32),
    I64(i64),
    Bool(bool),
    Str(String),
}
use crate::state::GraphQLState;
use crate::types::*;

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
#[derive(InputObject, Copy, Clone, Eq, PartialEq)]
pub struct MangaOrder {
    pub by: MangaOrderBy,
    pub by_type: Option<SortOrder>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum MangaOrderBy {
    Id,
    Title,
    InLibraryAt,
    LastFetchedAt,
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

/// Mirrors `CategoryFilter`/condition (core).
#[derive(InputObject, Default)]
pub struct CategoryCondition {
    pub id: Option<i32>,
    pub name: Option<String>,
    pub default: Option<bool>,
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

    async fn mangas(
        &self,
        ctx: &Context<'_>,
        condition: Option<MangaCondition>,
        order: Option<Vec<MangaOrder>>,
        first: Option<i32>,
        after: Option<Cursor>,
    ) -> async_graphql::Result<MangaNodeList> {
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

    async fn categories(
        &self,
        ctx: &Context<'_>,
        condition: Option<CategoryCondition>,
    ) -> async_graphql::Result<CategoryNodeList> {
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

    async fn chapters(
        &self,
        ctx: &Context<'_>,
        condition: Option<ChapterCondition>,
    ) -> async_graphql::Result<ChapterNodeList> {
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

    /// Minimal placeholder for remaining queries (Phase 4 increments).
    async fn about_server(&self) -> AboutServerPayload {
        AboutServerPayload {
            name: "Suwayomi (next)".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
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

#[derive(Union)]
pub enum UnionPlaceholder {
    AboutServer(AboutServerPayload),
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
