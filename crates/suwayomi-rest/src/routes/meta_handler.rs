//! Global meta helpers (used by GlobalAPI and shared with other routes).

use std::collections::HashMap;

use suwayomi_domain::error::{DomainError, Result};
use suwayomi_domain::sql::bind_placeholders;

use crate::state::AppState;
use sqlx::Row;

/// GET /api/v1/meta — all global_meta entries.
pub async fn get_global_meta(s: &AppState) -> Result<HashMap<String, String>> {
    let rows = sqlx::query("SELECT meta_key, value FROM global_meta").fetch_all(s.db.pool()).await?;
    let mut out = HashMap::new();
    for row in rows {
        out.insert(row.try_get("meta_key")?, row.try_get("value")?);
    }
    Ok(out)
}

/// PATCH /api/v1/meta — upsert a single key/value.
pub async fn set_global_meta(s: &AppState, key: String, value: String) -> Result<()> {
    let existing = bind_placeholders("SELECT id FROM global_meta WHERE meta_key = ?");
    let row = sqlx::query(&existing).bind(&key).fetch_optional(s.db.pool()).await?;
    if let Some(row) = row {
        let id: i32 = row.try_get("id")?;
        let sql = bind_placeholders("UPDATE global_meta SET value = ? WHERE id = ?");
        sqlx::query(&sql).bind(&value).bind(id).execute(s.db.pool()).await?;
    } else {
        let sql = bind_placeholders("INSERT INTO global_meta (meta_key, value) VALUES (?, ?)");
        sqlx::query(&sql).bind(&key).bind(&value).execute(s.db.pool()).await?;
    }
    Ok(())
}

#[allow(dead_code)]
fn _err() -> DomainError {
    DomainError::NotFound("unused".into())
}
