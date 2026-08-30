//! Category service — mirrors `suwayomi.tachidesk.manga.impl.Category`.

pub mod category_manga;

use std::collections::HashMap;

use suwayomi_core::db::Db;
use suwayomi_core::models::{CategoryDataClass, IncludeOrExclude};
use suwayomi_core::schema::CategoryRow;

use crate::error::Result;
use crate::meta::{MetaService, MetaTable};
use crate::sql::bind_placeholders;

#[derive(Clone)]
pub struct CategoryService {
    pub db: Db,
}

impl CategoryService {
    pub const DEFAULT_CATEGORY_ID: i32 = 0;
    pub const DEFAULT_CATEGORY_NAME: &'static str = "Default";

    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub fn meta(&self) -> MetaService {
        MetaService::new(self.db.clone())
    }

    fn row_to_dc(row: &CategoryRow) -> CategoryDataClass {
        CategoryDataClass {
            id: row.id,
            order: row.sort_order,
            name: row.name.clone(),
            default: row.is_default,
            include_in_update: IncludeOrExclude::from_i32(row.include_in_update),
            include_in_download: IncludeOrExclude::from_i32(row.include_in_download),
            version: row.version,
            uid: row.uid,
            last_modified_at: row.last_modified_at,
        }
    }

    /// Mirrors `createCategory`/`createCategories` (dedupe, illegal "Default" name).
    pub async fn create_categories(&self, names: &[String]) -> Result<Vec<i32>> {
        let existing = self.get_category_list().await?;
        let existing_names: Vec<String> = existing.iter().map(|c| c.name.to_lowercase()).collect();
        let mut created_by_name: HashMap<String, i32> = HashMap::new();

        let mut out = Vec::new();
        for name in names {
            if name.eq_ignore_ascii_case(Self::DEFAULT_CATEGORY_NAME) {
                out.push(Self::DEFAULT_CATEGORY_ID);
                continue;
            }
            let lower = name.to_lowercase();
            if existing_names.contains(&lower) {
                let id = existing.iter().find(|c| c.name.to_lowercase() == lower).map(|c| c.id).unwrap();
                out.push(id);
                continue;
            }
            if let Some(id) = created_by_name.get(&lower) {
                out.push(*id);
                continue;
            }
            let id = self.insert_category(name).await?;
            created_by_name.insert(lower, id);
            out.push(id);
        }
        self.normalize_categories().await?;
        Ok(out)
    }

    async fn insert_category(&self, name: &str) -> Result<i32> {
        let sql = bind_placeholders("INSERT INTO category (name, sort_order) VALUES (?, ?)");
        {
            let sql = format!("{sql} RETURNING id");
            let (id,): (i32,) = sqlx::query_as(&sql).bind(name).bind(i32::MAX).fetch_one(self.db.pool()).await?;
            Ok(id)
        }
    }

    /// Mirrors `updateCategory`.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_category(
        &self,
        category_id: i32,
        name: Option<String>,
        is_default: Option<bool>,
        include_in_update: Option<i32>,
        include_in_download: Option<i32>,
    ) -> Result<()> {
        // resolve which fields actually change
        let (name, is_default) = if category_id == Self::DEFAULT_CATEGORY_ID {
            (None, None)
        } else {
            let n = name.and_then(|n| if n.eq_ignore_ascii_case(Self::DEFAULT_CATEGORY_NAME) { None } else { Some(n) });
            (n, is_default)
        };
        if name.is_none() && is_default.is_none() && include_in_update.is_none() && include_in_download.is_none() {
            return Ok(());
        }

        // build SET clause with positional binds (reconstructed per backend)
        let mut sets: Vec<&str> = Vec::new();
        if name.is_some() {
            sets.push("name = ?");
        }
        if is_default.is_some() {
            sets.push("is_default = ?");
        }
        if include_in_update.is_some() {
            sets.push("include_in_update = ?");
        }
        if include_in_download.is_some() {
            sets.push("include_in_download = ?");
        }
        let sql = bind_placeholders(&format!("UPDATE category SET {} WHERE id = ?", sets.join(", ")));
        {
            let mut q = sqlx::query(&sql);
            if let Some(n) = &name {
                q = q.bind(n);
            }
            if let Some(v) = is_default {
                q = q.bind(v);
            }
            if let Some(v) = include_in_update {
                q = q.bind(v);
            }
            if let Some(v) = include_in_download {
                q = q.bind(v);
            }
            q.bind(category_id).execute(self.db.pool()).await?;
        }
        Ok(())
    }

    /// Mirrors `reorderCategory(from, to)` (1-based positions).
    pub async fn reorder_category(&self, from: i32, to: i32) -> Result<()> {
        if from == 0 || to == 0 {
            return Ok(());
        }
        let mut rows = self.list_rows_excluding_default().await?;
        if from > rows.len() as i32 || to > rows.len() as i32 + 1 {
            return Ok(());
        }
        let removed = rows.remove((from - 1) as usize);
        let insert_at = (to - 1).clamp(0, rows.len() as i32) as usize;
        rows.insert(insert_at, removed);
        for (i, row) in rows.iter().enumerate() {
            let sql = bind_placeholders("UPDATE category SET sort_order = ? WHERE id = ?");
            {
                sqlx::query(&sql).bind((i + 1) as i32).bind(row.id).execute(self.db.pool()).await?;
            }
        }
        self.normalize_categories().await
    }

    /// Mirrors `removeCategory`.
    pub async fn remove_category(&self, category_id: i32) -> Result<()> {
        if category_id == Self::DEFAULT_CATEGORY_ID {
            return Ok(());
        }
        let sql = bind_placeholders("DELETE FROM category WHERE id = ?");
        {
            sqlx::query(&sql).bind(category_id).execute(self.db.pool()).await?;
        }
        self.normalize_categories().await
    }

    /// Mirrors `normalizeCategories` — order starts from 0 (or 1 after reorder),
    /// default category first.
    pub async fn normalize_categories(&self) -> Result<()> {
        let mut rows = self.list_rows_all().await?;
        rows.sort_by_key(|r| (r.id != 0, r.sort_order));
        for (i, row) in rows.iter().enumerate() {
            let sql = bind_placeholders("UPDATE category SET sort_order = ? WHERE id = ?");
            {
                sqlx::query(&sql).bind(i as i32).bind(row.id).execute(self.db.pool()).await?;
            }
        }
        Ok(())
    }

    /// Mirrors `getCategoryList` — default category only when needed (manga in
    /// library without any category).
    pub async fn get_category_list(&self) -> Result<Vec<CategoryDataClass>> {
        let needs_default = self.needs_default_category().await?;
        let rows = self.list_rows_all().await?;
        let mut out: Vec<CategoryDataClass> =
            rows.iter().filter(|r| needs_default || r.id != Self::DEFAULT_CATEGORY_ID).map(Self::row_to_dc).collect();
        out.sort_by_key(|c| c.order);
        Ok(out)
    }

    pub async fn get_category_by_id(&self, category_id: i32) -> Result<Option<CategoryDataClass>> {
        let sql = bind_placeholders("SELECT * FROM category WHERE id = ?");
        let row = sqlx::query_as::<_, CategoryRow>(&sql).bind(category_id).fetch_optional(self.db.pool()).await?;
        Ok(row.map(|r| Self::row_to_dc(&r)))
    }

    pub async fn get_category_size(&self, category_id: i32) -> Result<i64> {
        let sql = bind_placeholders(
            "SELECT count(*) FROM category_manga cm WHERE cm.category = ? AND EXISTS (SELECT 1 FROM manga m WHERE m.id = cm.manga AND m.in_library = TRUE)");
        let n = sqlx::query_scalar::<_, i64>(&sql).bind(category_id).fetch_one(self.db.pool()).await?;
        Ok(n)
    }

    pub async fn get_meta_map(&self, category_id: i32) -> Result<HashMap<String, String>> {
        self.meta().get_map(MetaTable::Category, category_id as i64).await
    }

    pub async fn modify_metas(&self, metas_by_category_id: &HashMap<i32, HashMap<String, String>>) -> Result<()> {
        let by_ref = metas_by_category_id.iter().map(|(k, v)| (*k as i64, v.clone())).collect::<HashMap<_, _>>();
        self.meta().modify(MetaTable::Category, &by_ref).await
    }

    async fn needs_default_category(&self) -> Result<bool> {
        let sql = "SELECT count(*) FROM manga m LEFT JOIN category_manga cm ON cm.manga = m.id WHERE m.in_library = TRUE AND cm.manga IS NULL";
        let n: i64 = sqlx::query_scalar(sql).fetch_one(self.db.pool()).await?;
        Ok(n > 0)
    }

    async fn list_rows_all(&self) -> Result<Vec<CategoryRow>> {
        let sql = "SELECT * FROM category ORDER BY sort_order ASC";
        let rows = sqlx::query_as::<_, CategoryRow>(sql).fetch_all(self.db.pool()).await?;
        Ok(rows)
    }

    async fn list_rows_excluding_default(&self) -> Result<Vec<CategoryRow>> {
        let sql = bind_placeholders("SELECT * FROM category WHERE id != ? ORDER BY sort_order ASC");
        let rows =
            { sqlx::query_as::<_, CategoryRow>(&sql).bind(Self::DEFAULT_CATEGORY_ID).fetch_all(self.db.pool()).await? };
        Ok(rows)
    }
}
