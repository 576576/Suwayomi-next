//! Mutation root — mirrors `graphql/mutations/*.kt`.
//! Batch B1: Category + Meta mutations (DB-driven; fully implemented).

use async_graphql::{Context, Enum, InputObject, Object, SimpleObject};
use sqlx::Row;
use std::collections::HashMap;

use suwayomi_core::schema::{CategoryRow, ChapterRow, MangaRow};
use suwayomi_domain::meta::{MetaService, MetaTable};
use suwayomi_domain::sql::bind_placeholders;

use crate::scalars::LongString;
use crate::state::GraphQLState;
use crate::types::*;

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

#[derive(InputObject)]
pub struct MetaInput {
    pub key: String,
    pub value: String,
}

#[derive(InputObject)]
pub struct GlobalMetaTypeInput {
    pub key: String,
    pub value: String,
}

#[derive(InputObject)]
pub struct MangaMetaTypeInput {
    pub key: String,
    pub manga_id: i32,
    pub value: String,
}

#[derive(InputObject)]
pub struct ChapterMetaTypeInput {
    pub chapter_id: i32,
    pub key: String,
    pub value: String,
}

#[derive(InputObject)]
pub struct SourceMetaTypeInput {
    pub key: String,
    pub source_id: LongString,
    pub value: String,
}

#[derive(InputObject)]
pub struct CategoryMetaTypeInput {
    pub category_id: i32,
    pub key: String,
    pub value: String,
}

#[derive(InputObject)]
pub struct CreateCategoryInput {
    pub client_mutation_id: Option<String>,
    pub default: Option<bool>,
    pub include_in_download: Option<IncludeOrExclude>,
    pub include_in_update: Option<IncludeOrExclude>,
    pub name: String,
    pub order: Option<i32>,
}

#[derive(InputObject)]
pub struct DeleteCategoryInput {
    pub category_id: i32,
    pub client_mutation_id: Option<String>,
}

#[derive(InputObject)]
pub struct UpdateCategoryPatchInput {
    pub default: Option<bool>,
    pub include_in_download: Option<IncludeOrExclude>,
    pub include_in_update: Option<IncludeOrExclude>,
    pub name: Option<String>,
}

#[derive(InputObject)]
pub struct UpdateCategoryInput {
    pub client_mutation_id: Option<String>,
    pub id: i32,
    pub patch: UpdateCategoryPatchInput,
}

#[derive(InputObject)]
pub struct UpdateCategoriesInput {
    pub client_mutation_id: Option<String>,
    pub ids: Vec<i32>,
    pub patch: UpdateCategoryPatchInput,
}

#[derive(InputObject)]
pub struct UpdateCategoryOrderInput {
    pub client_mutation_id: Option<String>,
    pub id: i32,
    pub position: i32,
}

#[derive(InputObject)]
pub struct UpdateMangaCategoriesPatchInput {
    pub add_to_categories: Option<Vec<i32>>,
    pub clear_categories: Option<bool>,
    pub remove_from_categories: Option<Vec<i32>>,
}

#[derive(InputObject)]
pub struct UpdateMangaCategoriesInput {
    pub client_mutation_id: Option<String>,
    pub id: i32,
    pub patch: UpdateMangaCategoriesPatchInput,
}

#[derive(InputObject)]
pub struct UpdateMangasCategoriesInput {
    pub client_mutation_id: Option<String>,
    pub ids: Vec<i32>,
    pub patch: UpdateMangaCategoriesPatchInput,
}

#[derive(InputObject)]
pub struct SetCategoryMetaInput {
    pub client_mutation_id: Option<String>,
    pub meta: CategoryMetaTypeInput,
}

#[derive(InputObject)]
pub struct SetCategoryMetasItemInput {
    pub category_ids: Vec<i32>,
    pub metas: Vec<MetaInput>,
}

#[derive(InputObject)]
pub struct SetCategoryMetasInput {
    pub client_mutation_id: Option<String>,
    pub items: Vec<SetCategoryMetasItemInput>,
}

#[derive(InputObject)]
pub struct DeleteCategoryMetaInput {
    pub category_id: i32,
    pub client_mutation_id: Option<String>,
    pub key: String,
}

#[derive(InputObject)]
pub struct DeleteCategoryMetasItemInput {
    pub category_ids: Vec<i32>,
    pub keys: Option<Vec<String>>,
    pub prefixes: Option<Vec<String>>,
}

#[derive(InputObject)]
pub struct DeleteCategoryMetasInput {
    pub client_mutation_id: Option<String>,
    pub items: Vec<DeleteCategoryMetasItemInput>,
}

#[derive(InputObject)]
pub struct SetGlobalMetaInput {
    pub client_mutation_id: Option<String>,
    pub meta: GlobalMetaTypeInput,
}

#[derive(InputObject)]
pub struct SetGlobalMetasInput {
    pub client_mutation_id: Option<String>,
    pub metas: Vec<MetaInput>,
}

#[derive(InputObject)]
pub struct DeleteGlobalMetaInput {
    pub client_mutation_id: Option<String>,
    pub key: String,
}

#[derive(InputObject)]
pub struct DeleteGlobalMetasInput {
    pub client_mutation_id: Option<String>,
    pub keys: Option<Vec<String>>,
    pub prefixes: Option<Vec<String>>,
}

#[derive(InputObject)]
pub struct SetMangaMetaInput {
    pub client_mutation_id: Option<String>,
    pub meta: MangaMetaTypeInput,
}

#[derive(InputObject)]
pub struct SetMangaMetasItemInput {
    pub manga_ids: Vec<i32>,
    pub metas: Vec<MetaInput>,
}

#[derive(InputObject)]
pub struct SetMangaMetasInput {
    pub client_mutation_id: Option<String>,
    pub items: Vec<SetMangaMetasItemInput>,
}

#[derive(InputObject)]
pub struct DeleteMangaMetaInput {
    pub client_mutation_id: Option<String>,
    pub key: String,
    pub manga_id: i32,
}

#[derive(InputObject)]
pub struct DeleteMangaMetasItemInput {
    pub keys: Option<Vec<String>>,
    pub manga_ids: Vec<i32>,
    pub prefixes: Option<Vec<String>>,
}

#[derive(InputObject)]
pub struct DeleteMangaMetasInput {
    pub client_mutation_id: Option<String>,
    pub items: Vec<DeleteMangaMetasItemInput>,
}

#[derive(InputObject)]
pub struct SetChapterMetaInput {
    pub client_mutation_id: Option<String>,
    pub meta: ChapterMetaTypeInput,
}

#[derive(InputObject)]
pub struct SetChapterMetasItemInput {
    pub chapter_ids: Vec<i32>,
    pub metas: Vec<MetaInput>,
}

#[derive(InputObject)]
pub struct SetChapterMetasInput {
    pub client_mutation_id: Option<String>,
    pub items: Vec<SetChapterMetasItemInput>,
}

#[derive(InputObject)]
pub struct DeleteChapterMetaInput {
    pub chapter_id: i32,
    pub client_mutation_id: Option<String>,
    pub key: String,
}

#[derive(InputObject)]
pub struct DeleteChapterMetasItemInput {
    pub chapter_ids: Vec<i32>,
    pub keys: Option<Vec<String>>,
    pub prefixes: Option<Vec<String>>,
}

#[derive(InputObject)]
pub struct DeleteChapterMetasInput {
    pub client_mutation_id: Option<String>,
    pub items: Vec<DeleteChapterMetasItemInput>,
}

#[derive(InputObject)]
pub struct SetSourceMetaInput {
    pub client_mutation_id: Option<String>,
    pub meta: SourceMetaTypeInput,
}

#[derive(InputObject)]
pub struct SetSourceMetasItemInput {
    pub metas: Vec<MetaInput>,
    pub source_ids: Vec<LongString>,
}

#[derive(InputObject)]
pub struct SetSourceMetasInput {
    pub client_mutation_id: Option<String>,
    pub items: Vec<SetSourceMetasItemInput>,
}

#[derive(InputObject)]
pub struct DeleteSourceMetaInput {
    pub client_mutation_id: Option<String>,
    pub key: String,
    pub source_id: LongString,
}

#[derive(InputObject)]
pub struct DeleteSourceMetasItemInput {
    pub keys: Option<Vec<String>>,
    pub prefixes: Option<Vec<String>>,
    pub source_ids: Vec<LongString>,
}

#[derive(InputObject)]
pub struct DeleteSourceMetasInput {
    pub client_mutation_id: Option<String>,
    pub items: Vec<DeleteSourceMetasItemInput>,
}

// ---------------------------------------------------------------------------
// Payloads
// ---------------------------------------------------------------------------

#[derive(SimpleObject, Clone)]
pub struct CreateCategoryPayload {
    pub category: CategoryType,
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct DeleteCategoryPayload {
    pub category: Option<CategoryType>,
    pub client_mutation_id: Option<String>,
    pub mangas: Vec<MangaType>,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateCategoryPayload {
    pub category: CategoryType,
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateCategoriesPayload {
    pub categories: Vec<CategoryType>,
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateCategoryOrderPayload {
    pub categories: Vec<CategoryType>,
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateMangaCategoriesPayload {
    pub client_mutation_id: Option<String>,
    pub manga: MangaType,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateMangasCategoriesPayload {
    pub client_mutation_id: Option<String>,
    pub mangas: Vec<MangaType>,
}

#[derive(SimpleObject, Clone)]
pub struct SetCategoryMetaPayload {
    pub client_mutation_id: Option<String>,
    pub meta: CategoryMetaType,
}

#[derive(SimpleObject, Clone)]
pub struct SetCategoryMetasPayload {
    pub categories: Vec<CategoryType>,
    pub client_mutation_id: Option<String>,
    pub metas: Vec<CategoryMetaType>,
}

#[derive(SimpleObject, Clone)]
pub struct DeleteCategoryMetaPayload {
    pub category: CategoryType,
    pub client_mutation_id: Option<String>,
    pub meta: Option<CategoryMetaType>,
}

#[derive(SimpleObject, Clone)]
pub struct DeleteCategoryMetasPayload {
    pub categories: Vec<CategoryType>,
    pub client_mutation_id: Option<String>,
    pub metas: Vec<CategoryMetaType>,
}

#[derive(SimpleObject, Clone)]
pub struct SetGlobalMetaPayload {
    pub client_mutation_id: Option<String>,
    pub meta: GlobalMetaType,
}

#[derive(SimpleObject, Clone)]
pub struct SetGlobalMetasPayload {
    pub client_mutation_id: Option<String>,
    pub metas: Vec<GlobalMetaType>,
}

#[derive(SimpleObject, Clone)]
pub struct DeleteGlobalMetaPayload {
    pub client_mutation_id: Option<String>,
    pub meta: Option<GlobalMetaType>,
}

#[derive(SimpleObject, Clone)]
pub struct DeleteGlobalMetasPayload {
    pub client_mutation_id: Option<String>,
    pub metas: Vec<GlobalMetaType>,
}

#[derive(SimpleObject, Clone)]
pub struct SetMangaMetaPayload {
    pub client_mutation_id: Option<String>,
    pub meta: MangaMetaType,
}

#[derive(SimpleObject, Clone)]
pub struct SetMangaMetasPayload {
    pub client_mutation_id: Option<String>,
    pub mangas: Vec<MangaType>,
    pub metas: Vec<MangaMetaType>,
}

#[derive(SimpleObject, Clone)]
pub struct DeleteMangaMetaPayload {
    pub client_mutation_id: Option<String>,
    pub manga: MangaType,
    pub meta: Option<MangaMetaType>,
}

#[derive(SimpleObject, Clone)]
pub struct DeleteMangaMetasPayload {
    pub client_mutation_id: Option<String>,
    pub mangas: Vec<MangaType>,
    pub metas: Vec<MangaMetaType>,
}

#[derive(SimpleObject, Clone)]
pub struct SetChapterMetaPayload {
    pub client_mutation_id: Option<String>,
    pub meta: ChapterMetaType,
}

#[derive(SimpleObject, Clone)]
pub struct SetChapterMetasPayload {
    pub chapters: Vec<ChapterType>,
    pub client_mutation_id: Option<String>,
    pub metas: Vec<ChapterMetaType>,
}

#[derive(SimpleObject, Clone)]
pub struct DeleteChapterMetaPayload {
    pub chapter: ChapterType,
    pub client_mutation_id: Option<String>,
    pub meta: Option<ChapterMetaType>,
}

#[derive(SimpleObject, Clone)]
pub struct DeleteChapterMetasPayload {
    pub chapters: Vec<ChapterType>,
    pub client_mutation_id: Option<String>,
    pub metas: Vec<ChapterMetaType>,
}

#[derive(SimpleObject, Clone)]
pub struct SetSourceMetaPayload {
    pub client_mutation_id: Option<String>,
    pub meta: SourceMetaType,
}

#[derive(SimpleObject, Clone)]
pub struct SetSourceMetasPayload {
    pub client_mutation_id: Option<String>,
    pub metas: Vec<SourceMetaType>,
    pub sources: Vec<SourceType>,
}

#[derive(SimpleObject, Clone)]
pub struct DeleteSourceMetaPayload {
    pub client_mutation_id: Option<String>,
    pub meta: Option<SourceMetaType>,
    pub source: Option<SourceType>,
}

#[derive(SimpleObject, Clone)]
pub struct DeleteSourceMetasPayload {
    pub client_mutation_id: Option<String>,
    pub metas: Vec<SourceMetaType>,
    pub sources: Vec<SourceType>,
}

// ---------------------------------------------------------------------------
// B3: Manga/Chapter mutations
// ---------------------------------------------------------------------------

#[derive(InputObject)]
pub struct UpdateMangaPatchInput {
    pub in_library: Option<bool>,
}

#[derive(InputObject)]
pub struct UpdateMangaInput {
    pub client_mutation_id: Option<String>,
    pub id: i32,
    pub patch: UpdateMangaPatchInput,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateMangaPayload {
    pub client_mutation_id: Option<String>,
    pub manga: MangaType,
}

#[derive(InputObject)]
pub struct UpdateMangasInput {
    pub client_mutation_id: Option<String>,
    pub ids: Vec<i32>,
    pub patch: UpdateMangaPatchInput,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateMangasPayload {
    pub client_mutation_id: Option<String>,
    pub mangas: Vec<MangaType>,
}

#[derive(InputObject)]
pub struct UpdateChapterPatchInput {
    pub is_bookmarked: Option<bool>,
    pub is_read: Option<bool>,
    pub last_page_read: Option<i32>,
}

#[derive(InputObject)]
pub struct UpdateChapterInput {
    pub client_mutation_id: Option<String>,
    pub id: i32,
    pub patch: UpdateChapterPatchInput,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateChapterPayload {
    pub chapter: ChapterType,
    pub client_mutation_id: Option<String>,
}

#[derive(InputObject)]
pub struct UpdateChaptersInput {
    pub client_mutation_id: Option<String>,
    pub ids: Vec<i32>,
    pub patch: UpdateChapterPatchInput,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateChaptersPayload {
    pub chapters: Vec<ChapterType>,
    pub client_mutation_id: Option<String>,
}

#[derive(InputObject)]
pub struct FetchMangaInput {
    pub client_mutation_id: Option<String>,
    pub id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct FetchMangaPayload {
    pub client_mutation_id: Option<String>,
    pub manga: MangaType,
}

#[derive(InputObject)]
pub struct FetchMangaAndChaptersInput {
    pub client_mutation_id: Option<String>,
    pub fetch_chapters: bool,
    pub fetch_manga: bool,
    pub id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct FetchMangaAndChaptersPayload {
    pub chapters: Vec<ChapterType>,
    pub client_mutation_id: Option<String>,
    pub manga: MangaType,
}

#[derive(InputObject)]
pub struct FetchChaptersInput {
    pub client_mutation_id: Option<String>,
    pub manga_id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct FetchChaptersPayload {
    pub chapters: Vec<ChapterType>,
    pub client_mutation_id: Option<String>,
}

#[derive(InputObject)]
pub struct FetchChapterPagesInput {
    pub chapter_id: i32,
    pub client_mutation_id: Option<String>,
    pub format: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct SyncConflictInfoType {
    pub device_name: String,
    pub remote_page: i32,
}

#[derive(SimpleObject, Clone)]
pub struct FetchChapterPagesPayload {
    pub chapter: ChapterType,
    pub client_mutation_id: Option<String>,
    pub pages: Vec<String>,
    pub sync_conflict: Option<SyncConflictInfoType>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum FetchSourceMangaType {
    Search,
    Popular,
    Latest,
}

#[derive(InputObject)]
pub struct FilterChangeInput {
    pub check_box_state: Option<bool>,
    pub position: Option<i32>,
    pub sort_state: Option<SortSelectionInput>,
    pub state: Option<i32>,
    pub text_state: Option<String>,
    pub tri_state: Option<TriState>,
}

#[derive(InputObject)]
pub struct SortSelectionInput {
    pub ascending: bool,
    pub index: i32,
}

#[derive(InputObject)]
pub struct FetchSourceMangaInput {
    pub client_mutation_id: Option<String>,
    pub filters: Option<Vec<FilterChangeInput>>,
    pub page: i32,
    pub query: Option<String>,
    pub source: LongString,
    #[graphql(name = "type")]
    pub r#type: FetchSourceMangaType,
}

#[derive(SimpleObject, Clone)]
pub struct FetchSourceMangaPayload {
    pub client_mutation_id: Option<String>,
    pub has_next_page: bool,
    pub mangas: Vec<MangaType>,
}

// ---------------------------------------------------------------------------
// Mutation root
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MutationRoot;

#[Object]
impl MutationRoot {
    // ---- Category ----

    async fn create_category(
        &self,
        ctx: &Context<'_>,
        input: CreateCategoryInput,
    ) -> async_graphql::Result<CreateCategoryPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let pool = state.db.pool();
        let name = input.name;
        // uniqueness check
        let exists: i64 =
            sqlx::query_scalar(bind_placeholders("SELECT COUNT(*) FROM category WHERE name = ?").as_str())
                .bind(&name)
                .fetch_one(pool)
                .await
                .map_err(async_graphql::Error::from)?;
        if exists > 0 {
            return Err(async_graphql::Error::new("'name' must be unique"));
        }
        if name.eq_ignore_ascii_case("Default") {
            return Err(async_graphql::Error::new("'name' must not be Default"));
        }
        if let Some(order) = input.order {
            if order <= 0 {
                return Err(async_graphql::Error::new("'order' must not be <= 0"));
            }
        }
        if let Some(order) = input.order {
            sqlx::query(
                bind_placeholders("UPDATE category SET sort_order = sort_order + 1 WHERE sort_order >= ?").as_str(),
            )
            .bind(order)
            .execute(pool)
            .await
            .map_err(async_graphql::Error::from)?;
        }
        let id: i32 = sqlx::query_scalar(
            bind_placeholders(
                "INSERT INTO category (name, sort_order, is_default, include_in_update, include_in_download) VALUES (?, ?, ?, ?, ?) RETURNING id",
            )
            .as_str(),
        )
        .bind(&name)
        .bind(input.order.unwrap_or(i32::MAX))
        .bind(input.default.unwrap_or(false))
        .bind(input.include_in_update.map(|v| v as i32).unwrap_or(0))
        .bind(input.include_in_download.map(|v| v as i32).unwrap_or(0))
        .fetch_one(pool)
        .await
        .map_err(async_graphql::Error::from)?;
        state.category.normalize_categories().await.map_err(async_graphql::Error::from)?;
        let row = fetch_category_row(state, id).await?;
        Ok(CreateCategoryPayload { category: CategoryType::from(&row), client_mutation_id: input.client_mutation_id })
    }

    async fn delete_category(
        &self,
        ctx: &Context<'_>,
        input: DeleteCategoryInput,
    ) -> async_graphql::Result<DeleteCategoryPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let id = input.category_id;
        if id == 0 {
            return Ok(DeleteCategoryPayload {
                client_mutation_id: input.client_mutation_id,
                category: None,
                mangas: vec![],
            });
        }
        let pool = state.db.pool();
        let cat_row = fetch_category_row_opt(state, id).await?;
        // mangas in this category
        let mangas: Vec<MangaRow> = sqlx::query_as::<_, MangaRow>(
            bind_placeholders(
                "SELECT m.* FROM manga m INNER JOIN category_manga cm ON cm.manga = m.id WHERE cm.category = ?",
            )
            .as_str(),
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .map_err(async_graphql::Error::from)?;
        sqlx::query(bind_placeholders("DELETE FROM category WHERE id = ?").as_str())
            .bind(id)
            .execute(pool)
            .await
            .map_err(async_graphql::Error::from)?;
        state.category.normalize_categories().await.map_err(async_graphql::Error::from)?;
        Ok(DeleteCategoryPayload {
            client_mutation_id: input.client_mutation_id,
            category: cat_row.map(|r| CategoryType::from(&r)),
            mangas: mangas.iter().map(MangaType::from_row).collect(),
        })
    }

    async fn update_category(
        &self,
        ctx: &Context<'_>,
        input: UpdateCategoryInput,
    ) -> async_graphql::Result<UpdateCategoryPayload> {
        let state = ctx.data::<GraphQLState>()?;
        apply_category_patch(state, input.id, &input.patch).await?;
        let row = fetch_category_row(state, input.id).await?;
        Ok(UpdateCategoryPayload { category: CategoryType::from(&row), client_mutation_id: input.client_mutation_id })
    }

    async fn update_categories(
        &self,
        ctx: &Context<'_>,
        input: UpdateCategoriesInput,
    ) -> async_graphql::Result<UpdateCategoriesPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let mut categories = Vec::new();
        for id in &input.ids {
            apply_category_patch(state, *id, &input.patch).await?;
            categories.push(CategoryType::from(&fetch_category_row(state, *id).await?));
        }
        Ok(UpdateCategoriesPayload { categories, client_mutation_id: input.client_mutation_id })
    }

    async fn update_category_order(
        &self,
        ctx: &Context<'_>,
        input: UpdateCategoryOrderInput,
    ) -> async_graphql::Result<UpdateCategoryOrderPayload> {
        let state = ctx.data::<GraphQLState>()?;
        // find current position (1-based) of the category
        let row = fetch_category_row(state, input.id).await?;
        state.category.reorder_category(row.sort_order, input.position).await.map_err(async_graphql::Error::from)?;
        state.category.normalize_categories().await.map_err(async_graphql::Error::from)?;
        let list = state.category.get_category_list().await.map_err(async_graphql::Error::from)?;
        let categories = list
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
        Ok(UpdateCategoryOrderPayload { categories, client_mutation_id: input.client_mutation_id })
    }

    async fn update_manga_categories(
        &self,
        ctx: &Context<'_>,
        input: UpdateMangaCategoriesInput,
    ) -> async_graphql::Result<UpdateMangaCategoriesPayload> {
        let state = ctx.data::<GraphQLState>()?;
        apply_manga_categories_patch(state, &[input.id], &input.patch).await?;
        let row = fetch_manga_row(state, input.id).await?;
        Ok(UpdateMangaCategoriesPayload {
            client_mutation_id: input.client_mutation_id,
            manga: MangaType::from_row(&row),
        })
    }

    async fn update_mangas_categories(
        &self,
        ctx: &Context<'_>,
        input: UpdateMangasCategoriesInput,
    ) -> async_graphql::Result<UpdateMangasCategoriesPayload> {
        let state = ctx.data::<GraphQLState>()?;
        apply_manga_categories_patch(state, &input.ids, &input.patch).await?;
        let mut mangas = Vec::new();
        for id in &input.ids {
            mangas.push(MangaType::from_row(&fetch_manga_row(state, *id).await?));
        }
        Ok(UpdateMangasCategoriesPayload { client_mutation_id: input.client_mutation_id, mangas })
    }

    // ---- Category meta ----

    async fn set_category_meta(
        &self,
        ctx: &Context<'_>,
        input: SetCategoryMetaInput,
    ) -> async_graphql::Result<SetCategoryMetaPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let meta = input.meta;
        let mut by_ref: HashMap<i64, HashMap<String, String>> = HashMap::new();
        let mut m = HashMap::new();
        m.insert(meta.key.clone(), meta.value.clone());
        by_ref.insert(meta.category_id as i64, m);
        MetaService::new(state.db.clone())
            .modify(MetaTable::Category, &by_ref)
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(SetCategoryMetaPayload {
            client_mutation_id: input.client_mutation_id,
            meta: CategoryMetaType { key: meta.key, value: meta.value, category_id: meta.category_id },
        })
    }

    async fn set_category_metas(
        &self,
        ctx: &Context<'_>,
        input: SetCategoryMetasInput,
    ) -> async_graphql::Result<SetCategoryMetasPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let mut by_ref: HashMap<i64, HashMap<String, String>> = HashMap::new();
        let mut ids = Vec::new();
        for item in &input.items {
            ids.extend(item.category_ids.iter().copied());
            for cid in &item.category_ids {
                let m = by_ref.entry(*cid as i64).or_default();
                for meta in &item.metas {
                    m.insert(meta.key.clone(), meta.value.clone());
                }
            }
        }
        MetaService::new(state.db.clone())
            .modify(MetaTable::Category, &by_ref)
            .await
            .map_err(async_graphql::Error::from)?;
        let mut metas = Vec::new();
        for (cid, m) in by_ref {
            for (k, v) in m {
                metas.push(CategoryMetaType { key: k, value: v, category_id: cid as i32 });
            }
        }
        let mut categories = Vec::new();
        for id in &ids {
            categories.push(CategoryType::from(&fetch_category_row(state, *id).await?));
        }
        Ok(SetCategoryMetasPayload { categories, client_mutation_id: input.client_mutation_id, metas })
    }

    async fn delete_category_meta(
        &self,
        ctx: &Context<'_>,
        input: DeleteCategoryMetaInput,
    ) -> async_graphql::Result<DeleteCategoryMetaPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let old = fetch_category_meta(state, input.category_id, &input.key).await?;
        sqlx::query(bind_placeholders("DELETE FROM category_meta WHERE category_ref = ? AND meta_key = ?").as_str())
            .bind(input.category_id)
            .bind(&input.key)
            .execute(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let category = CategoryType::from(&fetch_category_row(state, input.category_id).await?);
        Ok(DeleteCategoryMetaPayload { category, client_mutation_id: input.client_mutation_id, meta: old })
    }

    async fn delete_category_metas(
        &self,
        ctx: &Context<'_>,
        input: DeleteCategoryMetasInput,
    ) -> async_graphql::Result<DeleteCategoryMetasPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let mut metas = Vec::new();
        let mut ids = Vec::new();
        for item in &input.items {
            ids.extend(item.category_ids.iter().copied());
            for cid in &item.category_ids {
                let deleted =
                    delete_meta_by_key_or_prefix(state, MetaTable::Category, *cid as i64, &item.keys, &item.prefixes)
                        .await?;
                for (k, v) in deleted {
                    metas.push(CategoryMetaType { key: k, value: v, category_id: *cid });
                }
            }
        }
        let mut categories = Vec::new();
        for id in &ids {
            categories.push(CategoryType::from(&fetch_category_row(state, *id).await?));
        }
        Ok(DeleteCategoryMetasPayload { categories, client_mutation_id: input.client_mutation_id, metas })
    }

    // ---- Global meta ----

    async fn set_global_meta(
        &self,
        ctx: &Context<'_>,
        input: SetGlobalMetaInput,
    ) -> async_graphql::Result<SetGlobalMetaPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let meta = input.meta;
        let mut m = HashMap::new();
        m.insert(meta.key.clone(), meta.value.clone());
        let mut by_ref = HashMap::new();
        by_ref.insert(0i64, m);
        MetaService::new(state.db.clone())
            .modify(MetaTable::Global, &by_ref)
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(SetGlobalMetaPayload {
            client_mutation_id: input.client_mutation_id,
            meta: GlobalMetaType { key: meta.key, value: meta.value },
        })
    }

    async fn set_global_metas(
        &self,
        ctx: &Context<'_>,
        input: SetGlobalMetasInput,
    ) -> async_graphql::Result<SetGlobalMetasPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let mut m = HashMap::new();
        for meta in &input.metas {
            m.insert(meta.key.clone(), meta.value.clone());
        }
        let mut by_ref = HashMap::new();
        by_ref.insert(0i64, m.clone());
        MetaService::new(state.db.clone())
            .modify(MetaTable::Global, &by_ref)
            .await
            .map_err(async_graphql::Error::from)?;
        let metas = m.into_iter().map(|(k, v)| GlobalMetaType { key: k, value: v }).collect();
        Ok(SetGlobalMetasPayload { client_mutation_id: input.client_mutation_id, metas })
    }

    async fn delete_global_meta(
        &self,
        ctx: &Context<'_>,
        input: DeleteGlobalMetaInput,
    ) -> async_graphql::Result<DeleteGlobalMetaPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let old = fetch_global_meta(state, &input.key).await?;
        sqlx::query(bind_placeholders("DELETE FROM global_meta WHERE meta_key = ?").as_str())
            .bind(&input.key)
            .execute(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(DeleteGlobalMetaPayload { client_mutation_id: input.client_mutation_id, meta: old })
    }

    async fn delete_global_metas(
        &self,
        ctx: &Context<'_>,
        input: DeleteGlobalMetasInput,
    ) -> async_graphql::Result<DeleteGlobalMetasPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let deleted = delete_meta_by_key_or_prefix(state, MetaTable::Global, 0, &input.keys, &input.prefixes).await?;
        let metas = deleted.into_iter().map(|(k, v)| GlobalMetaType { key: k, value: v }).collect();
        Ok(DeleteGlobalMetasPayload { client_mutation_id: input.client_mutation_id, metas })
    }

    // ---- Manga meta ----

    async fn set_manga_meta(
        &self,
        ctx: &Context<'_>,
        input: SetMangaMetaInput,
    ) -> async_graphql::Result<SetMangaMetaPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let meta = input.meta;
        let mut m = HashMap::new();
        m.insert(meta.key.clone(), meta.value.clone());
        let mut by_ref = HashMap::new();
        by_ref.insert(meta.manga_id as i64, m);
        MetaService::new(state.db.clone())
            .modify(MetaTable::Manga, &by_ref)
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(SetMangaMetaPayload {
            client_mutation_id: input.client_mutation_id,
            meta: MangaMetaType { key: meta.key, value: meta.value, manga_id: meta.manga_id },
        })
    }

    async fn set_manga_metas(
        &self,
        ctx: &Context<'_>,
        input: SetMangaMetasInput,
    ) -> async_graphql::Result<SetMangaMetasPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let mut by_ref: HashMap<i64, HashMap<String, String>> = HashMap::new();
        let mut ids = Vec::new();
        for item in &input.items {
            ids.extend(item.manga_ids.iter().copied());
            for mid in &item.manga_ids {
                let m = by_ref.entry(*mid as i64).or_default();
                for meta in &item.metas {
                    m.insert(meta.key.clone(), meta.value.clone());
                }
            }
        }
        MetaService::new(state.db.clone())
            .modify(MetaTable::Manga, &by_ref)
            .await
            .map_err(async_graphql::Error::from)?;
        let mut metas = Vec::new();
        for (mid, m) in by_ref {
            for (k, v) in m {
                metas.push(MangaMetaType { key: k, value: v, manga_id: mid as i32 });
            }
        }
        let mut mangas = Vec::new();
        for id in &ids {
            mangas.push(MangaType::from_row(&fetch_manga_row(state, *id).await?));
        }
        Ok(SetMangaMetasPayload { client_mutation_id: input.client_mutation_id, mangas, metas })
    }

    async fn delete_manga_meta(
        &self,
        ctx: &Context<'_>,
        input: DeleteMangaMetaInput,
    ) -> async_graphql::Result<DeleteMangaMetaPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let old = fetch_manga_meta(state, input.manga_id, &input.key).await?;
        sqlx::query(bind_placeholders("DELETE FROM manga_meta WHERE manga_ref = ? AND meta_key = ?").as_str())
            .bind(input.manga_id)
            .bind(&input.key)
            .execute(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let manga = MangaType::from_row(&fetch_manga_row(state, input.manga_id).await?);
        Ok(DeleteMangaMetaPayload { client_mutation_id: input.client_mutation_id, manga, meta: old })
    }

    async fn delete_manga_metas(
        &self,
        ctx: &Context<'_>,
        input: DeleteMangaMetasInput,
    ) -> async_graphql::Result<DeleteMangaMetasPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let mut metas = Vec::new();
        let mut ids = Vec::new();
        for item in &input.items {
            ids.extend(item.manga_ids.iter().copied());
            for mid in &item.manga_ids {
                let deleted =
                    delete_meta_by_key_or_prefix(state, MetaTable::Manga, *mid as i64, &item.keys, &item.prefixes)
                        .await?;
                for (k, v) in deleted {
                    metas.push(MangaMetaType { key: k, value: v, manga_id: *mid });
                }
            }
        }
        let mut mangas = Vec::new();
        for id in &ids {
            mangas.push(MangaType::from_row(&fetch_manga_row(state, *id).await?));
        }
        Ok(DeleteMangaMetasPayload { client_mutation_id: input.client_mutation_id, mangas, metas })
    }

    // ---- Chapter meta ----

    async fn set_chapter_meta(
        &self,
        ctx: &Context<'_>,
        input: SetChapterMetaInput,
    ) -> async_graphql::Result<SetChapterMetaPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let meta = input.meta;
        let mut m = HashMap::new();
        m.insert(meta.key.clone(), meta.value.clone());
        let mut by_ref = HashMap::new();
        by_ref.insert(meta.chapter_id as i64, m);
        MetaService::new(state.db.clone())
            .modify(MetaTable::Chapter, &by_ref)
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(SetChapterMetaPayload {
            client_mutation_id: input.client_mutation_id,
            meta: ChapterMetaType { key: meta.key, value: meta.value, chapter_id: meta.chapter_id },
        })
    }

    async fn set_chapter_metas(
        &self,
        ctx: &Context<'_>,
        input: SetChapterMetasInput,
    ) -> async_graphql::Result<SetChapterMetasPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let mut by_ref: HashMap<i64, HashMap<String, String>> = HashMap::new();
        let mut ids = Vec::new();
        for item in &input.items {
            ids.extend(item.chapter_ids.iter().copied());
            for cid in &item.chapter_ids {
                let m = by_ref.entry(*cid as i64).or_default();
                for meta in &item.metas {
                    m.insert(meta.key.clone(), meta.value.clone());
                }
            }
        }
        MetaService::new(state.db.clone())
            .modify(MetaTable::Chapter, &by_ref)
            .await
            .map_err(async_graphql::Error::from)?;
        let mut metas = Vec::new();
        for (cid, m) in by_ref {
            for (k, v) in m {
                metas.push(ChapterMetaType { key: k, value: v, chapter_id: cid as i32 });
            }
        }
        let mut chapters = Vec::new();
        for id in &ids {
            chapters.push(ChapterType::from_row(&fetch_chapter_row(state, *id).await?));
        }
        Ok(SetChapterMetasPayload { chapters, client_mutation_id: input.client_mutation_id, metas })
    }

    async fn delete_chapter_meta(
        &self,
        ctx: &Context<'_>,
        input: DeleteChapterMetaInput,
    ) -> async_graphql::Result<DeleteChapterMetaPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let old = fetch_chapter_meta(state, input.chapter_id, &input.key).await?;
        sqlx::query(bind_placeholders("DELETE FROM chapter_meta WHERE chapter_ref = ? AND meta_key = ?").as_str())
            .bind(input.chapter_id)
            .bind(&input.key)
            .execute(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let chapter = ChapterType::from_row(&fetch_chapter_row(state, input.chapter_id).await?);
        Ok(DeleteChapterMetaPayload { chapter, client_mutation_id: input.client_mutation_id, meta: old })
    }

    async fn delete_chapter_metas(
        &self,
        ctx: &Context<'_>,
        input: DeleteChapterMetasInput,
    ) -> async_graphql::Result<DeleteChapterMetasPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let mut metas = Vec::new();
        let mut ids = Vec::new();
        for item in &input.items {
            ids.extend(item.chapter_ids.iter().copied());
            for cid in &item.chapter_ids {
                let deleted =
                    delete_meta_by_key_or_prefix(state, MetaTable::Chapter, *cid as i64, &item.keys, &item.prefixes)
                        .await?;
                for (k, v) in deleted {
                    metas.push(ChapterMetaType { key: k, value: v, chapter_id: *cid });
                }
            }
        }
        let mut chapters = Vec::new();
        for id in &ids {
            chapters.push(ChapterType::from_row(&fetch_chapter_row(state, *id).await?));
        }
        Ok(DeleteChapterMetasPayload { chapters, client_mutation_id: input.client_mutation_id, metas })
    }

    // ---- Source meta ----

    async fn set_source_meta(
        &self,
        ctx: &Context<'_>,
        input: SetSourceMetaInput,
    ) -> async_graphql::Result<SetSourceMetaPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let meta = input.meta;
        let mut m = HashMap::new();
        m.insert(meta.key.clone(), meta.value.clone());
        let mut by_ref = HashMap::new();
        by_ref.insert(meta.source_id.0, m);
        MetaService::new(state.db.clone())
            .modify(MetaTable::Source, &by_ref)
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(SetSourceMetaPayload {
            client_mutation_id: input.client_mutation_id,
            meta: SourceMetaType { key: meta.key, value: meta.value, source_id: meta.source_id.0 },
        })
    }

    async fn set_source_metas(
        &self,
        ctx: &Context<'_>,
        input: SetSourceMetasInput,
    ) -> async_graphql::Result<SetSourceMetasPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let mut by_ref: HashMap<i64, HashMap<String, String>> = HashMap::new();
        let mut ids = Vec::new();
        for item in &input.items {
            for sid in &item.source_ids {
                ids.push(sid.0);
                let m = by_ref.entry(sid.0).or_default();
                for meta in &item.metas {
                    m.insert(meta.key.clone(), meta.value.clone());
                }
            }
        }
        MetaService::new(state.db.clone())
            .modify(MetaTable::Source, &by_ref)
            .await
            .map_err(async_graphql::Error::from)?;
        let mut metas = Vec::new();
        for (sid, m) in by_ref {
            for (k, v) in m {
                metas.push(SourceMetaType { key: k, value: v, source_id: sid });
            }
        }
        let mut sources = Vec::new();
        for id in &ids {
            sources.push(fetch_source_type(state, *id).await?);
        }
        Ok(SetSourceMetasPayload { client_mutation_id: input.client_mutation_id, metas, sources })
    }

    async fn delete_source_meta(
        &self,
        ctx: &Context<'_>,
        input: DeleteSourceMetaInput,
    ) -> async_graphql::Result<DeleteSourceMetaPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let old = fetch_source_meta(state, input.source_id.0, &input.key).await?;
        sqlx::query(bind_placeholders("DELETE FROM source_meta WHERE source_ref = ? AND meta_key = ?").as_str())
            .bind(input.source_id.0)
            .bind(&input.key)
            .execute(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let source = fetch_source_type(state, input.source_id.0).await.ok();
        Ok(DeleteSourceMetaPayload { client_mutation_id: input.client_mutation_id, meta: old, source })
    }

    // ---- B3: Manga / Chapter ----

    async fn update_manga(
        &self,
        ctx: &Context<'_>,
        input: UpdateMangaInput,
    ) -> async_graphql::Result<UpdateMangaPayload> {
        let state = ctx.data::<GraphQLState>()?;
        if let Some(in_library) = input.patch.in_library {
            if in_library {
                state.library.add_manga_to_library(input.id).await.map_err(async_graphql::Error::from)?;
            } else {
                state.library.remove_manga_from_library(input.id).await.map_err(async_graphql::Error::from)?;
            }
        }
        let manga = MangaType::from_row(&fetch_manga_row(state, input.id).await?);
        Ok(UpdateMangaPayload { client_mutation_id: input.client_mutation_id, manga })
    }

    async fn update_mangas(
        &self,
        ctx: &Context<'_>,
        input: UpdateMangasInput,
    ) -> async_graphql::Result<UpdateMangasPayload> {
        let state = ctx.data::<GraphQLState>()?;
        if let Some(in_library) = input.patch.in_library {
            for id in &input.ids {
                if in_library {
                    state.library.add_manga_to_library(*id).await.map_err(async_graphql::Error::from)?;
                } else {
                    state.library.remove_manga_from_library(*id).await.map_err(async_graphql::Error::from)?;
                }
            }
        }
        let mut mangas = Vec::new();
        for id in &input.ids {
            mangas.push(MangaType::from_row(&fetch_manga_row(state, *id).await?));
        }
        Ok(UpdateMangasPayload { client_mutation_id: input.client_mutation_id, mangas })
    }

    async fn update_chapter(
        &self,
        ctx: &Context<'_>,
        input: UpdateChapterInput,
    ) -> async_graphql::Result<UpdateChapterPayload> {
        let state = ctx.data::<GraphQLState>()?;
        apply_chapter_patch(state, &[input.id], &input.patch).await?;
        let chapter = ChapterType::from_row(&fetch_chapter_row(state, input.id).await?);
        Ok(UpdateChapterPayload { chapter, client_mutation_id: input.client_mutation_id })
    }

    async fn update_chapters(
        &self,
        ctx: &Context<'_>,
        input: UpdateChaptersInput,
    ) -> async_graphql::Result<UpdateChaptersPayload> {
        let state = ctx.data::<GraphQLState>()?;
        apply_chapter_patch(state, &input.ids, &input.patch).await?;
        let mut chapters = Vec::new();
        for id in &input.ids {
            chapters.push(ChapterType::from_row(&fetch_chapter_row(state, *id).await?));
        }
        Ok(UpdateChaptersPayload { chapters, client_mutation_id: input.client_mutation_id })
    }

    async fn fetch_manga(&self, ctx: &Context<'_>, input: FetchMangaInput) -> async_graphql::Result<FetchMangaPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let dc = state.manga.get_manga(input.id, true).await.map_err(async_graphql::Error::from)?;
        let manga = MangaType::from_row(&fetch_manga_row(state, dc.id).await?);
        Ok(FetchMangaPayload { client_mutation_id: input.client_mutation_id, manga })
    }

    async fn fetch_chapters(
        &self,
        ctx: &Context<'_>,
        input: FetchChaptersInput,
    ) -> async_graphql::Result<FetchChaptersPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let list = state.chapter.get_chapter_list(input.manga_id, true).await.map_err(async_graphql::Error::from)?;
        let chapters = list
            .iter()
            .map(|c| ChapterType {
                id: c.id,
                url: c.url.clone(),
                name: c.name.clone(),
                upload_date: c.upload_date,
                chapter_number: c.chapter_number,
                scanlator: c.scanlator.clone(),
                manga_id: c.manga_id,
                read: c.read,
                bookmarked: c.bookmarked,
                last_page_read: c.last_page_read,
                last_read_at: c.last_read_at,
                source_order: c.index,
                fetched_at: c.fetched_at,
                real_url: c.real_url.clone(),
                downloaded: c.downloaded,
                page_count: c.page_count,
            })
            .collect();
        Ok(FetchChaptersPayload { chapters, client_mutation_id: input.client_mutation_id })
    }

    async fn fetch_manga_and_chapters(
        &self,
        ctx: &Context<'_>,
        input: FetchMangaAndChaptersInput,
    ) -> async_graphql::Result<FetchMangaAndChaptersPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let manga = if input.fetch_manga {
            let dc = state.manga.get_manga(input.id, true).await.map_err(async_graphql::Error::from)?;
            MangaType::from_row(&fetch_manga_row(state, dc.id).await?)
        } else {
            MangaType::from_row(&fetch_manga_row(state, input.id).await?)
        };
        // Local source: seed chapters from disk so the chapter list populates
        // without a prior browse call.
        if input.fetch_chapters {
            if let Ok(row) = fetch_manga_row(state, input.id).await {
                if row.source == suwayomi_domain::source::LOCAL_SOURCE_ID {
                    let root = suwayomi_domain::source::local::local_source_root();
                    if let Some(dir) = suwayomi_domain::source::local::local_manga_dir(&root, &row.url) {
                        let chapters = suwayomi_domain::source::local::scan_local_chapters(&dir);
                        upsert_local_chapters(state, row.id, &chapters, Some(&dir)).await?;
                    }
                }
            }
        }
        let chapters = if input.fetch_chapters {
            let list = state.chapter.get_chapter_list(input.id, true).await.map_err(async_graphql::Error::from)?;
            list.iter()
                .map(|c| ChapterType {
                    id: c.id,
                    url: c.url.clone(),
                    name: c.name.clone(),
                    upload_date: c.upload_date,
                    chapter_number: c.chapter_number,
                    scanlator: c.scanlator.clone(),
                    manga_id: c.manga_id,
                    read: c.read,
                    bookmarked: c.bookmarked,
                    last_page_read: c.last_page_read,
                    last_read_at: c.last_read_at,
                    source_order: c.index,
                    fetched_at: c.fetched_at,
                    real_url: c.real_url.clone(),
                    downloaded: c.downloaded,
                    page_count: c.page_count,
                })
                .collect()
        } else {
            vec![]
        };
        Ok(FetchMangaAndChaptersPayload { chapters, client_mutation_id: input.client_mutation_id, manga })
    }

    async fn fetch_chapter_pages(
        &self,
        ctx: &Context<'_>,
        input: FetchChapterPagesInput,
    ) -> async_graphql::Result<FetchChapterPagesPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let chapter = ChapterType::from_row(&fetch_chapter_row(state, input.chapter_id).await?);
        // Local source: seed page rows from disk when missing.
        if let Ok(manga_row) = fetch_manga_row(state, chapter.manga_id).await {
            if manga_row.source == suwayomi_domain::source::LOCAL_SOURCE_ID {
                seed_local_pages(state, &manga_row, &chapter).await?;
            }
        }
        let sql = bind_placeholders("SELECT url, image_url FROM page WHERE chapter = ? ORDER BY index ASC");
        let rows = sqlx::query(&sql)
            .bind(input.chapter_id)
            .fetch_all(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let mut pages = Vec::new();
        for row in &rows {
            let url: String = row.try_get("url").unwrap_or_default();
            let image_url: Option<String> = row.try_get("image_url").ok();
            pages.push(image_url.unwrap_or(url));
        }
        Ok(FetchChapterPagesPayload {
            chapter,
            client_mutation_id: input.client_mutation_id,
            pages,
            sync_conflict: None,
        })
    }

    async fn fetch_source_manga(
        &self,
        ctx: &Context<'_>,
        input: FetchSourceMangaInput,
    ) -> async_graphql::Result<FetchSourceMangaPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let source_id = input.source.0;
        let page_num = input.page.max(1) as u32;

        // Resolve the manga rows for this page, then map to GraphQL types.
        let ids: Vec<i32>;
        let has_next_page: bool;
        if source_id == suwayomi_domain::source::LOCAL_SOURCE_ID {
            // Local source: scan `data/local/` (folders -> manga). Search
            // filters by title client-side; pagination is a single page.
            let root = suwayomi_domain::source::local::local_source_root();
            let mut mangas = suwayomi_domain::source::local::scan_local_source(&root);
            if let Some(q) = input.query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
                let needle = q.to_lowercase();
                mangas.retain(|m| m.title.to_lowercase().contains(&needle));
            }
            ids = state
                .manga_list
                .insert_or_update(suwayomi_domain::source::LOCAL_SOURCE_ID, &mangas)
                .await
                .map_err(async_graphql::Error::from)?;
            // Seed chapters from disk (idempotent upsert by (manga, url)),
            // applying archive metadata (meta.json / ComicInfo.xml).
            for (m, id) in mangas.iter().zip(ids.iter()) {
                if let Some(dir) = suwayomi_domain::source::local::local_manga_dir(&root, &m.url) {
                    let chapters = suwayomi_domain::source::local::scan_local_chapters(&dir);
                    upsert_local_chapters(state, *id, &chapters, Some(&dir)).await?;
                }
            }
            has_next_page = false;
        } else {
            let paged = match input.r#type {
                FetchSourceMangaType::Popular => {
                    state.manga_list.get_manga_list(source_id, page_num, true).await.map_err(async_graphql::Error::from)?
                }
                FetchSourceMangaType::Latest => {
                    state.manga_list.get_manga_list(source_id, page_num, false).await.map_err(async_graphql::Error::from)?
                }
                FetchSourceMangaType::Search => {
                    let query = input.query.as_deref().unwrap_or("").to_string();
                    let page = state
                        .manga_list
                        .fetcher()
                        .search_manga(source_id, &query, page_num)
                        .await
                        .map_err(async_graphql::Error::from)?;
                    state.manga_list.process_entries(source_id, &page).await.map_err(async_graphql::Error::from)?
                }
            };
            ids = paged.manga_list.iter().map(|m| m.id).collect();
            has_next_page = paged.has_next_page;
        }

        let mut mangas = Vec::with_capacity(ids.len());
        for id in ids {
            let sql = bind_placeholders("SELECT * FROM manga WHERE id = ?");
            let row = sqlx::query_as::<_, MangaRow>(&sql)
                .bind(id)
                .fetch_optional(state.db.pool())
                .await
                .map_err(async_graphql::Error::from)?;
            if let Some(r) = row {
                mangas.push(MangaType::from_row(&r));
            }
        }
        Ok(FetchSourceMangaPayload {
            client_mutation_id: input.client_mutation_id,
            has_next_page,
            mangas,
        })
    }

    async fn delete_source_metas(
        &self,
        ctx: &Context<'_>,
        input: DeleteSourceMetasInput,
    ) -> async_graphql::Result<DeleteSourceMetasPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let mut metas = Vec::new();
        let mut ids = Vec::new();
        for item in &input.items {
            for sid in &item.source_ids {
                ids.push(sid.0);
                let deleted =
                    delete_meta_by_key_or_prefix(state, MetaTable::Source, sid.0, &item.keys, &item.prefixes).await?;
                for (k, v) in deleted {
                    metas.push(SourceMetaType { key: k, value: v, source_id: sid.0 });
                }
            }
        }
        let mut sources = Vec::new();
        for id in &ids {
            if let Ok(s) = fetch_source_type(state, *id).await {
                sources.push(s);
            }
        }
        Ok(DeleteSourceMetasPayload { client_mutation_id: input.client_mutation_id, metas, sources })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn fetch_category_row(state: &GraphQLState, id: i32) -> async_graphql::Result<CategoryRow> {
    fetch_category_row_opt(state, id).await?.ok_or_else(|| async_graphql::Error::new("Category not found"))
}

async fn fetch_category_row_opt(state: &GraphQLState, id: i32) -> async_graphql::Result<Option<CategoryRow>> {
    let sql = bind_placeholders("SELECT * FROM category WHERE id = ?");
    sqlx::query_as::<_, CategoryRow>(&sql)
        .bind(id)
        .fetch_optional(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)
}

async fn fetch_manga_row(state: &GraphQLState, id: i32) -> async_graphql::Result<MangaRow> {
    let sql = bind_placeholders("SELECT * FROM manga WHERE id = ?");
    sqlx::query_as::<_, MangaRow>(&sql).bind(id).fetch_one(state.db.pool()).await.map_err(async_graphql::Error::from)
}

async fn fetch_chapter_row(state: &GraphQLState, id: i32) -> async_graphql::Result<ChapterRow> {
    let sql = bind_placeholders("SELECT * FROM chapter WHERE id = ?");
    sqlx::query_as::<_, ChapterRow>(&sql).bind(id).fetch_one(state.db.pool()).await.map_err(async_graphql::Error::from)
}

/// Idempotent upsert of local-source chapters by (manga, url).
async fn upsert_local_chapters(
    state: &GraphQLState,
    manga_id: i32,
    chapters: &[suwayomi_core::source::SChapter],
    manga_dir: Option<&std::path::Path>,
) -> async_graphql::Result<()> {
    use suwayomi_core::models::manga::now_epoch_secs;
    use suwayomi_domain::source::local::{ARCHIVE_EXTS, read_archive_meta};
    let now = now_epoch_secs();
    for (i, c) in chapters.iter().enumerate() {
        // sourceOrder is 1-based (Tachiyomi/WebUI convention): the reader
        // resolves the current chapter as `chapters[len - sourceOrder]`.
        let source_order = i as i32 + 1;
        // Archive chapters carry chapter metadata (`meta.json` /
        // `ComicInfo.xml` — mutually exclusive): apply date, scanlator and
        // chapter number. The chapter name always comes from the on-disk
        // file name (the SChapter name), never from embedded metadata.
        let meta = manga_dir
            .filter(|_| {
                let ext = c.url.rsplit('.').next().unwrap_or("").to_lowercase();
                ARCHIVE_EXTS.contains(&ext.as_str())
            })
            .and_then(|dir| read_archive_meta(&dir.join(&c.url)));
        let name = c.name.clone();
        let date_upload = meta.as_ref().and_then(|m| m.upload_date).unwrap_or(c.date_upload);
        let scanlator = meta.as_ref().and_then(|m| m.scanlator.clone()).or_else(|| c.scanlator.clone());
        let chapter_number = meta.as_ref().and_then(|m| m.number).unwrap_or(c.chapter_number);
        let sql = bind_placeholders("SELECT id FROM chapter WHERE manga = ? AND url = ?");
        let existing: Option<(i32,)> = sqlx::query_as(&sql)
            .bind(manga_id)
            .bind(&c.url)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        match existing {
            Some((id,)) => {
                let sql = bind_placeholders(
                    "UPDATE chapter SET name = ?, chapter_number = ?, source_order = ?, fetched_at = ?, last_modified_at = ?, date_upload = ?, scanlator = ? WHERE id = ?",
                );
                sqlx::query(&sql)
                    .bind(&name)
                    .bind(chapter_number)
                    .bind(source_order)
                    .bind(now)
                    .bind(now)
                    .bind(date_upload)
                    .bind(&scanlator)
                    .bind(id)
                    .execute(state.db.pool())
                    .await
                    .map_err(async_graphql::Error::from)?;
            }
            None => {
                let sql = bind_placeholders(
                    "INSERT INTO chapter (url, name, chapter_number, source_order, manga, fetched_at, last_modified_at, date_upload, scanlator) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                );
                sqlx::query(&sql)
                    .bind(&c.url)
                    .bind(&name)
                    .bind(chapter_number)
                    .bind(source_order)
                    .bind(manga_id)
                    .bind(now)
                    .bind(now)
                    .bind(date_upload)
                    .bind(&scanlator)
                    .execute(state.db.pool())
                    .await
                    .map_err(async_graphql::Error::from)?;
            }
        }
    }
    Ok(())
}

/// Seed page rows for a local-source chapter from disk (idempotent: only when
/// the chapter has no page rows yet).
async fn seed_local_pages(
    state: &GraphQLState,
    manga_row: &MangaRow,
    chapter: &ChapterType,
) -> async_graphql::Result<()> {
    use suwayomi_domain::source::local as local_src;
    let root = local_src::local_source_root();
    let Some(manga_dir) = local_src::local_manga_dir(&root, &manga_row.url) else {
        return Ok(());
    };
    let chapter_path = manga_dir.join(&chapter.url);
    if !chapter_path.exists() {
        return Ok(());
    }
    // Rebuild page rows from disk every time: keeps archive chapters (whose
    // page list depends on zip contents) and any legacy placeholder rows in
    // sync with the actual files.
    let sql = bind_placeholders("DELETE FROM page WHERE chapter = ?");
    sqlx::query(&sql)
        .bind(chapter.id)
        .execute(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
    // Root-relative prefix: the WebUI reader embeds page URLs directly in
    // <img src> (no base-url rewrite, unlike thumbnails), so a plain
    // relative `local/...` would resolve against the SPA route (e.g.
    // /manga/232/chapter/local/...). Leading `/` keeps it on the server root.
    let url_prefix = format!("/local/{}/{}", manga_row.url, chapter.url);
    let pages = local_src::scan_local_pages(&chapter_path, &url_prefix);
    if pages.is_empty() {
        return Ok(());
    }
    for p in &pages {
        let image_url = p.image_url.clone().unwrap_or_else(|| p.url.clone());
        let sql = bind_placeholders("INSERT INTO page (index, url, image_url, chapter) VALUES (?, ?, ?, ?)");
        sqlx::query(&sql)
            .bind(p.index)
            .bind(&p.url)
            .bind(&image_url)
            .bind(chapter.id)
            .execute(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
    }
    // Reflect the page count on the chapter row.
    let sql = bind_placeholders("UPDATE chapter SET page_count = ? WHERE id = ?");
    sqlx::query(&sql)
        .bind(pages.len() as i32)
        .bind(chapter.id)
        .execute(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
    Ok(())
}

async fn fetch_source_type(state: &GraphQLState, id: i64) -> async_graphql::Result<SourceType> {
    // 本地源（LOCAL_SOURCE_ID=0）不在 source 表，由 sources resolver 合成——
    // 直接返回合成条目，避免 fetch_one 对不存在的行报
    // "no rows returned"（如 WebUI 置顶源 isPinned 走 setSourceMetas）。
    if id == suwayomi_domain::source::LOCAL_SOURCE_ID {
        return Ok(SourceType::local_source());
    }
    let sql = bind_placeholders("SELECT * FROM source WHERE id = ?");
    let row = sqlx::query_as::<_, suwayomi_core::schema::SourceRow>(&sql)
        .bind(id)
        .fetch_one(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
    Ok(SourceType::from_row(&row))
}

async fn apply_category_patch(
    state: &GraphQLState,
    id: i32,
    patch: &UpdateCategoryPatchInput,
) -> async_graphql::Result<()> {
    state
        .category
        .update_category(
            id,
            patch.name.clone(),
            patch.default,
            patch.include_in_update.map(|v| v as i32),
            patch.include_in_download.map(|v| v as i32),
        )
        .await
        .map_err(async_graphql::Error::from)
}

async fn apply_chapter_patch(
    state: &GraphQLState,
    ids: &[i32],
    patch: &UpdateChapterPatchInput,
) -> async_graphql::Result<()> {
    state
        .chapter
        .modify_chapters_by_ids(ids, patch.is_read, patch.is_bookmarked, patch.last_page_read)
        .await
        .map_err(async_graphql::Error::from)
}

async fn apply_manga_categories_patch(
    state: &GraphQLState,
    ids: &[i32],
    patch: &UpdateMangaCategoriesPatchInput,
) -> async_graphql::Result<()> {
    if let Some(clear) = patch.clear_categories {
        if clear {
            for id in ids {
                state.category_manga.remove_manga_from_all_categories(*id).await.map_err(async_graphql::Error::from)?;
            }
        }
    }
    if let Some(add) = &patch.add_to_categories {
        state.category_manga.add_mangas_to_categories(ids, add).await.map_err(async_graphql::Error::from)?;
    }
    if let Some(remove) = &patch.remove_from_categories {
        for id in ids {
            for cid in remove {
                state.category_manga.remove_manga_from_category(*id, *cid).await.map_err(async_graphql::Error::from)?;
            }
        }
    }
    Ok(())
}

async fn fetch_category_meta(
    state: &GraphQLState,
    category_id: i32,
    key: &str,
) -> async_graphql::Result<Option<CategoryMetaType>> {
    let sql = bind_placeholders("SELECT meta_key, value FROM category_meta WHERE category_ref = ? AND meta_key = ?");
    let row = sqlx::query(&sql)
        .bind(category_id)
        .bind(key)
        .fetch_optional(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
    Ok(row.map(|r| CategoryMetaType {
        key: r.try_get("meta_key").unwrap_or_default(),
        value: r.try_get("value").unwrap_or_default(),
        category_id,
    }))
}

async fn fetch_global_meta(state: &GraphQLState, key: &str) -> async_graphql::Result<Option<GlobalMetaType>> {
    let sql = bind_placeholders("SELECT meta_key, value FROM global_meta WHERE meta_key = ?");
    let row = sqlx::query(&sql).bind(key).fetch_optional(state.db.pool()).await.map_err(async_graphql::Error::from)?;
    Ok(row.map(|r| GlobalMetaType {
        key: r.try_get("meta_key").unwrap_or_default(),
        value: r.try_get("value").unwrap_or_default(),
    }))
}

async fn fetch_manga_meta(
    state: &GraphQLState,
    manga_id: i32,
    key: &str,
) -> async_graphql::Result<Option<MangaMetaType>> {
    let sql = bind_placeholders("SELECT meta_key, value FROM manga_meta WHERE manga_ref = ? AND meta_key = ?");
    let row = sqlx::query(&sql)
        .bind(manga_id)
        .bind(key)
        .fetch_optional(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
    Ok(row.map(|r| MangaMetaType {
        key: r.try_get("meta_key").unwrap_or_default(),
        value: r.try_get("value").unwrap_or_default(),
        manga_id,
    }))
}

async fn fetch_chapter_meta(
    state: &GraphQLState,
    chapter_id: i32,
    key: &str,
) -> async_graphql::Result<Option<ChapterMetaType>> {
    let sql = bind_placeholders("SELECT meta_key, value FROM chapter_meta WHERE chapter_ref = ? AND meta_key = ?");
    let row = sqlx::query(&sql)
        .bind(chapter_id)
        .bind(key)
        .fetch_optional(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
    Ok(row.map(|r| ChapterMetaType {
        key: r.try_get("meta_key").unwrap_or_default(),
        value: r.try_get("value").unwrap_or_default(),
        chapter_id,
    }))
}

async fn fetch_source_meta(
    state: &GraphQLState,
    source_id: i64,
    key: &str,
) -> async_graphql::Result<Option<SourceMetaType>> {
    let sql = bind_placeholders("SELECT meta_key, value FROM source_meta WHERE source_ref = ? AND meta_key = ?");
    let row = sqlx::query(&sql)
        .bind(source_id)
        .bind(key)
        .fetch_optional(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
    Ok(row.map(|r| SourceMetaType {
        key: r.try_get("meta_key").unwrap_or_default(),
        value: r.try_get("value").unwrap_or_default(),
        source_id,
    }))
}

/// Deletes rows by exact keys and/or key prefixes; returns the deleted rows.
async fn delete_meta_by_key_or_prefix(
    state: &GraphQLState,
    table: MetaTable,
    ref_id: i64,
    keys: &Option<Vec<String>>,
    prefixes: &Option<Vec<String>>,
) -> async_graphql::Result<Vec<(String, String)>> {
    let table_name = table.table_name();
    let ref_column = table.ref_column();
    let pool = state.db.pool();
    let mut deleted = Vec::new();
    if let Some(keys) = keys {
        for key in keys {
            let sql = format!("SELECT meta_key, value FROM {table_name} WHERE {ref_column} = ? AND meta_key = ?");
            let row = sqlx::query(bind_placeholders(&sql).as_str())
                .bind(ref_id)
                .bind(key)
                .fetch_optional(pool)
                .await
                .map_err(async_graphql::Error::from)?;
            if let Some(r) = row {
                deleted.push((
                    r.try_get::<String, _>("meta_key").unwrap_or_default(),
                    r.try_get::<String, _>("value").unwrap_or_default(),
                ));
            }
            sqlx::query(
                bind_placeholders(&format!("DELETE FROM {table_name} WHERE {ref_column} = ? AND meta_key = ?"))
                    .as_str(),
            )
            .bind(ref_id)
            .bind(key)
            .execute(pool)
            .await
            .map_err(async_graphql::Error::from)?;
        }
    }
    if let Some(prefixes) = prefixes {
        for prefix in prefixes {
            let pattern = format!("{prefix}%");
            let sql = format!("SELECT meta_key, value FROM {table_name} WHERE {ref_column} = ? AND meta_key LIKE ?");
            let rows = sqlx::query(bind_placeholders(&sql).as_str())
                .bind(ref_id)
                .bind(&pattern)
                .fetch_all(pool)
                .await
                .map_err(async_graphql::Error::from)?;
            for r in &rows {
                deleted.push((
                    r.try_get::<String, _>("meta_key").unwrap_or_default(),
                    r.try_get::<String, _>("value").unwrap_or_default(),
                ));
            }
            sqlx::query(
                bind_placeholders(&format!("DELETE FROM {table_name} WHERE {ref_column} = ? AND meta_key LIKE ?"))
                    .as_str(),
            )
            .bind(ref_id)
            .bind(&pattern)
            .execute(pool)
            .await
            .map_err(async_graphql::Error::from)?;
        }
    }
    Ok(deleted)
}
