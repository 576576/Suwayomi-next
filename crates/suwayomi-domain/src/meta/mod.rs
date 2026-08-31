//! Meta key-value system — mirrors the batch-upsert semantics of
//! `Manga.modifyMangasMetas`, `Category.modifyCategoriesMetas` and
//! `Chapter.modifyChaptersMetas` (identical logic over different tables).

use std::collections::HashMap;

use sqlx::Row;
use suwayomi_core::db::Db;

use crate::error::Result;
use crate::sql::bind_placeholders;

/// Which meta table to operate on (all share `meta_key`/`value` columns).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaTable {
    Manga,
    Chapter,
    Category,
    Source,
    Global,
}

impl MetaTable {
    pub fn table_name(&self) -> &'static str {
        match self {
            Self::Manga => "manga_meta",
            Self::Chapter => "chapter_meta",
            Self::Category => "category_meta",
            Self::Source => "source_meta",
            Self::Global => "global_meta",
        }
    }

    pub fn ref_column(&self) -> &'static str {
        match self {
            Self::Manga => "manga_ref",
            Self::Chapter => "chapter_ref",
            Self::Category => "category_ref",
            Self::Source => "source_ref",
            // global_meta has no ref column; id = 0 is used as the only row scope
            Self::Global => "id",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MetaService {
    pub db: Db,
}

impl MetaService {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// getMangaMetaMap / getChapterMetaMap / getCategoryMetaMap / getSourceMetaMap / global
    pub async fn get_map(&self, table: MetaTable, ref_id: i64) -> Result<HashMap<String, String>> {
        let sql = if table == MetaTable::Global {
            "SELECT meta_key, value FROM global_meta".to_string()
        } else {
            bind_placeholders(&format!(
                "SELECT meta_key, value FROM {} WHERE {} = ?",
                table.table_name(),
                table.ref_column()
            ))
        };

        let mut out = HashMap::new();
        {
            let rows = if table == MetaTable::Global {
                sqlx::query(&sql).fetch_all(self.db.pool()).await?
            } else {
                sqlx::query(&sql).bind(ref_id).fetch_all(self.db.pool()).await?
            };
            for row in rows {
                out.insert(row.try_get("meta_key")?, row.try_get("value")?);
            }
        }
        Ok(out)
    }

    pub async fn get_maps(&self, table: MetaTable, ref_ids: &[i64]) -> Result<HashMap<i64, HashMap<String, String>>> {
        let mut out = HashMap::new();
        for id in ref_ids {
            out.insert(*id, self.get_map(table, *id).await?);
        }
        Ok(out)
    }

    /// Batch upsert: existing (ref, key) rows get their value updated; missing rows inserted.
    /// Mirrors `modifyMangasMetas` / `modifyCategoriesMetas` / `modifyChaptersMetas`.
    pub async fn modify(&self, table: MetaTable, metas_by_ref: &HashMap<i64, HashMap<String, String>>) -> Result<()> {
        for (&ref_id, metas) in metas_by_ref {
            let existing = self.find_existing(table, ref_id).await?; // key -> row id
            for (key, value) in metas {
                if let Some(row_id) = existing.get(key) {
                    self.exec_update(table, value, *row_id as i64).await?;
                } else {
                    self.exec_insert(table, key, value, ref_id).await?;
                }
            }
        }
        Ok(())
    }

    /// (key -> row id) for all existing rows of one ref (or all rows for global).
    async fn find_existing(&self, table: MetaTable, ref_id: i64) -> Result<HashMap<String, i32>> {
        let sql = if table == MetaTable::Global {
            "SELECT id, meta_key FROM global_meta".to_string()
        } else {
            bind_placeholders(&format!(
                "SELECT id, meta_key FROM {} WHERE {} = ?",
                table.table_name(),
                table.ref_column()
            ))
        };

        let mut out = HashMap::new();
        {
            let rows = if table == MetaTable::Global {
                sqlx::query(&sql).fetch_all(self.db.pool()).await?
            } else {
                sqlx::query(&sql).bind(ref_id).fetch_all(self.db.pool()).await?
            };
            for row in rows {
                out.insert(row.try_get("meta_key")?, row.try_get("id")?);
            }
        }
        Ok(out)
    }

    async fn exec_update(&self, table: MetaTable, value: &str, row_id: i64) -> Result<()> {
        let table_name = table.table_name();
        let sql = bind_placeholders(&format!("UPDATE {table_name} SET value = ? WHERE id = ?"));
        {
            sqlx::query(&sql).bind(value).bind(row_id).execute(self.db.pool()).await?;
        }
        Ok(())
    }

    async fn exec_insert(&self, table: MetaTable, key: &str, value: &str, ref_id: i64) -> Result<()> {
        if table == MetaTable::Global {
            // global_meta has no ref column; the id must be left to the
            // IDENTITY sequence. Binding id=0 explicitly collides once a row
            // with id 0 exists (e.g. the pre-seeded webUI_migration row).
            let sql = bind_placeholders("INSERT INTO global_meta (meta_key, value) VALUES (?, ?)");
            {
                sqlx::query(&sql).bind(key).bind(value).execute(self.db.pool()).await?;
            }
        } else {
            let ref_col = table.ref_column();
            let table_name = table.table_name();
            let sql = bind_placeholders(&format!("INSERT INTO {table_name} (meta_key, value, {ref_col}) VALUES (?, ?, ?)"));
            {
                sqlx::query(&sql).bind(key).bind(value).bind(ref_id).execute(self.db.pool()).await?;
            }
        }
        Ok(())
    }
}
