//! Schema migrations, embedded at compile time so the binary never needs a
//! `migrations/` directory at runtime.
//!
//! Each backend has its own list: PostgreSQL takes the original SQL verbatim
//! (baseline + increment + the PL/pgSQL sync functions/triggers), SQLite takes
//! the translated baseline plus the trigger port.
//!
//! The list is first probed against the tracking table: when every version is
//! already recorded the migrator returns without issuing any DDL. Otherwise the
//! pending steps are concatenated into **one script** and executed as a single
//! `batch_execute` call. That is deliberate: the call runs on one pooled
//! connection, so PostgreSQL wraps it in a single implicit transaction and a
//! transaction-scoped advisory lock can serialise concurrent migrators (a
//! second server process, or the parallel integration tests).
//!
//! PostgreSQL statements are all idempotent (`IF NOT EXISTS` / `CREATE OR
//! REPLACE` / `DROP … IF EXISTS`), so a partially applied list is replayed from
//! the top. SQLite has no `ADD COLUMN IF NOT EXISTS`: there the script carries
//! only the steps that are still unrecorded, because replaying everything would
//! fail on a database that already has the column.
//!
//! Applied versions live in `_suwayomi_migrations`. This replaces sqlx's
//! `_sqlx_migrations`, so an existing PostgreSQL database re-applies everything
//! once — safe for the reason above.
//!
//! Because an already-recorded version is skipped, editing the SQL of a
//! *shipped* migration has no effect: give the file a new name instead (or drop
//! the row from `_suwayomi_migrations` while the change is still local).

use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::backend::{BackendKind, Db};
use crate::error::Result;

/// One schema step.
#[derive(Clone, Copy)]
struct Migration {
    /// Recorded in `_suwayomi_migrations`; must never change once shipped.
    version: &'static str,
    sql: &'static str,
}

/// Tracking table (both dialects).
const TRACKING_TABLE: &str =
    "CREATE TABLE IF NOT EXISTS _suwayomi_migrations (version TEXT PRIMARY KEY, applied_at INTEGER NOT NULL)";

/// PostgreSQL keeps every table in one schema and pins `search_path=suwayomi`
/// on each connection (see the backend). While that schema does not exist the
/// search path resolves to nothing and even `CREATE TABLE` fails with `3F000`,
/// so it is created first — a qualified name works on an empty search path.
const POSTGRES_PRELUDE: &str = "CREATE SCHEMA IF NOT EXISTS suwayomi";

/// Advisory lock key serialising concurrent migrators (same value the
/// pre-dual-backend migrator used).
const ADVISORY_LOCK_KEY: i64 = 728232364;

/// Default backend — the translated baseline already carries `alt_titles`, so
/// there is no separate increment step.
const SQLITE_MIGRATIONS: &[Migration] = &[
    Migration {
        version: "0001_schema_baseline",
        sql: include_str!("../../../migrations/sqlite/0001_schema_baseline.sql"),
    },
    Migration {
        version: "0002_sync_triggers",
        sql: include_str!("../../../migrations/sqlite/0002_sync_triggers.sql"),
    },
    Migration {
        version: "0003_add_source_flags",
        sql: include_str!("../../../migrations/sqlite/0003_add_source_flags.sql"),
    },
    Migration {
        version: "0004_add_source_urls",
        sql: include_str!("../../../migrations/sqlite/0004_add_source_urls.sql"),
    },
    Migration {
        version: "0005_add_tracker_credentials",
        sql: include_str!("../../../migrations/sqlite/0005_add_tracker_credentials.sql"),
    },
];

/// PostgreSQL — same files the server used before the dual-backend split.
const POSTGRES_MIGRATIONS: &[Migration] = &[
    Migration {
        version: "0001_schema_baseline",
        sql: include_str!("../../../migrations/0001_schema_baseline.sql"),
    },
    Migration {
        version: "0002_add_manga_alt_titles",
        sql: include_str!("../../../migrations/0002_add_manga_alt_titles.sql"),
    },
    Migration {
        version: "0003_sync_functions",
        sql: include_str!("../../../migrations/pg-only/0002_sync_functions.sql"),
    },
    Migration {
        version: "0004_sync_triggers",
        sql: include_str!("../../../migrations/pg-only/0002_sync_triggers.sql"),
    },
    Migration {
        version: "0005_add_source_flags",
        sql: include_str!("../../../migrations/0005_add_source_flags.sql"),
    },
    Migration {
        version: "0006_add_source_urls",
        sql: include_str!("../../../migrations/0006_add_source_urls.sql"),
    },
    Migration {
        version: "0007_add_tracker_credentials",
        sql: include_str!("../../../migrations/0007_add_tracker_credentials.sql"),
    },
];

/// Applies the schema for the active backend.
pub async fn migrate(db: &Db) -> Result<()> {
    let migrations = match db.kind() {
        BackendKind::Sqlite => SQLITE_MIGRATIONS,
        BackendKind::Postgres => POSTGRES_MIGRATIONS,
    };
    if is_up_to_date(db, migrations).await {
        tracing::debug!(backend = db.kind().as_str(), "database schema already up to date");
        return Ok(());
    }
    // SQLite 的 `ALTER TABLE ADD COLUMN` 没有 `IF NOT EXISTS`，整套重放会在已经
    // 升级过的库上撞 `duplicate column name`：只拼还没记账的步骤。
    let pending: Vec<Migration> = if db.kind() == BackendKind::Sqlite {
        let applied = applied_versions(db).await;
        migrations.iter().copied().filter(|m| !applied.iter().any(|v| v == m.version)).collect()
    } else {
        migrations.to_vec()
    };
    if pending.is_empty() {
        return Ok(());
    }
    tracing::info!(
        backend = db.kind().as_str(),
        steps = migrations.len(),
        pending = pending.len(),
        "applying database schema (idempotent)"
    );
    let script = build_script(db.kind(), &pending);
    if let Err(e) = db.batch_execute(&script).await {
        // SQLite has no implicit transaction around a multi-statement script, so
        // a failed step would leave the transaction open on the connection.
        if db.kind() == BackendKind::Sqlite {
            let _ = db.batch_execute("ROLLBACK").await;
        }
        return Err(e);
    }
    Ok(())
}

/// `true` when every migration in `migrations` is already recorded.
///
/// A read-only probe that runs *before* any DDL, so an up-to-date database
/// costs one small query at startup instead of re-issuing `CREATE TABLE IF NOT
/// EXISTS` for every relation in the schema. That is not only an optimisation:
/// as a single transaction the re-run holds `AccessExclusiveLock` on every
/// table until it commits, which blocks a live instance's readers and deadlocks
/// against a concurrent `TRUNCATE … CASCADE` (the integration tests truncate
/// the tables they own).
///
/// Any error means "not up to date" — the tracking table may not exist yet, and
/// on PostgreSQL neither may the whole schema.
async fn is_up_to_date(db: &Db, migrations: &[Migration]) -> bool {
    let applied = applied_versions(db).await;
    migrations.iter().all(|m| applied.iter().any(|v| v == m.version))
}

/// 已记账的迁移版本。追踪表还不存在（全新库）时返回空 —— 视为一步都没做过。
async fn applied_versions(db: &Db) -> Vec<String> {
    let Ok(rows) = crate::query::query("SELECT version FROM _suwayomi_migrations").fetch_all(db).await else {
        return Vec::new();
    };
    rows.iter().filter_map(|row| row.try_get::<String, _>(0usize).ok()).collect()
}

/// Concatenates the tracking table, every migration and its bookkeeping row.
///
/// `SET`-free and comment-safe: the SQL files are included verbatim, and each is
/// known to end with `;`.
fn build_script(kind: BackendKind, migrations: &[Migration]) -> String {
    let mut script = String::new();
    // SQLite needs the explicit transaction; PostgreSQL gets one implicitly
    // for the whole multi-statement message.
    if kind == BackendKind::Sqlite {
        script.push_str("BEGIN;\n");
    } else {
        // Transaction-scoped: released when this message's transaction ends, so
        // the pooled connection is handed back clean. Taken *before* the schema
        // creation — `CREATE SCHEMA IF NOT EXISTS` is not atomic against a
        // concurrent creator and both would race into a `pg_namespace`
        // unique-violation.
        let _ = writeln!(script, "SELECT pg_advisory_xact_lock({ADVISORY_LOCK_KEY});");
        script.push_str(POSTGRES_PRELUDE);
        script.push_str(";\n");
    }
    script.push_str(TRACKING_TABLE);
    script.push_str(";\n");

    let now = now_epoch_secs();
    for migration in migrations {
        script.push_str(migration.sql);
        script.push('\n');
        let _ = writeln!(
            script,
            "INSERT INTO _suwayomi_migrations (version, applied_at) VALUES ('{}', {now}) \
             ON CONFLICT DO NOTHING;",
            migration.version
        );
    }
    if kind == BackendKind::Sqlite {
        script.push_str("COMMIT;\n");
    }
    script
}

/// Current unix time in seconds.
fn now_epoch_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    #[test]
    fn postgres_script_locks_then_creates_the_schema_then_migrates() {
        let script = build_script(BackendKind::Postgres, POSTGRES_MIGRATIONS);
        let lock = script.find("pg_advisory_xact_lock").expect("advisory lock");
        let prelude = script.find("CREATE SCHEMA IF NOT EXISTS suwayomi").expect("schema prelude");
        let tracking = script.find("CREATE TABLE IF NOT EXISTS _suwayomi_migrations").expect("tracking table");
        let first = script.find("CREATE TABLE IF NOT EXISTS suwayomi.extension").expect("baseline");
        assert!(lock < prelude && prelude < tracking && tracking < first);
        // PostgreSQL wraps a multi-statement message in one implicit transaction.
        assert!(!script.contains("BEGIN;"));
        assert!(!script.contains("COMMIT;"));
        for migration in POSTGRES_MIGRATIONS {
            assert!(script.contains(migration.version));
        }
    }

    #[test]
    fn sqlite_script_is_wrapped_in_a_transaction() {
        let script = build_script(BackendKind::Sqlite, SQLITE_MIGRATIONS);
        assert!(script.starts_with("BEGIN;\n"));
        assert!(script.trim_end().ends_with("COMMIT;"));
        assert!(!script.contains("CREATE SCHEMA"));
        assert!(!script.contains("pg_advisory"));
    }

    /// 已有库的增量升级：sqlite 上只能补未记账的步骤。
    ///
    /// 整套重放会撞 `duplicate column name` —— SQLite 的 `ADD COLUMN` 没有
    /// `IF NOT EXISTS`，于是升级路径直接起不来。
    #[tokio::test]
    async fn sqlite_upgrade_only_replays_unrecorded_steps() {
        let db = Db::sqlite_in_memory().await.unwrap();
        // 上一个发布版的状态：最后一步还没做过。
        let previous = &SQLITE_MIGRATIONS[..SQLITE_MIGRATIONS.len() - 1];
        db.batch_execute(&build_script(BackendKind::Sqlite, previous)).await.unwrap();
        assert!(is_up_to_date(&db, previous).await);

        migrate(&db).await.expect("增量升级必须成功");
        assert!(is_up_to_date(&db, SQLITE_MIGRATIONS).await);

        // 新迁移加的列真的落地了。
        crate::query::query("SELECT base_url, home_url FROM source")
            .fetch_all(&db)
            .await
            .expect("base_url / home_url 列必须存在");
    }

    #[tokio::test]
    async fn migrate_skips_the_script_once_every_version_is_recorded() {
        let db = Db::sqlite_in_memory().await.unwrap();
        assert!(!is_up_to_date(&db, SQLITE_MIGRATIONS).await, "no tracking table yet");
        db.migrate().await.unwrap();
        assert!(is_up_to_date(&db, SQLITE_MIGRATIONS).await, "all versions recorded");

        // An unknown version still forces the script to run.
        let unknown = [Migration { version: "9999_future", sql: "SELECT 1;" }];
        assert!(!is_up_to_date(&db, &unknown).await);
    }

    #[tokio::test]
    async fn sqlite_migrations_are_idempotent_and_build_the_schema() {
        let db = Db::sqlite_in_memory().await.unwrap();
        db.migrate().await.unwrap();
        db.migrate().await.unwrap();

        let tables = db
            .fetch_sql("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name", &[])
            .await
            .unwrap();
        let names: Vec<String> = tables.iter().filter_map(|r| r.try_get::<String, _>(0usize).ok()).collect();
        assert!(names.contains(&"manga".to_owned()));
        assert!(names.contains(&"chapter".to_owned()));
        assert!(names.contains(&"_suwayomi_migrations".to_owned()));

        let versions = db
            .fetch_sql("SELECT version FROM _suwayomi_migrations ORDER BY version", &[])
            .await
            .unwrap();
        assert_eq!(versions.len(), SQLITE_MIGRATIONS.len());
    }

    #[tokio::test]
    async fn triggers_fire_on_sqlite() {
        let db = Db::sqlite_in_memory().await.unwrap();
        db.migrate().await.unwrap();
        db.execute_sql(
            "INSERT INTO manga (id, url, title, source, update_strategy) VALUES (?, ?, ?, ?, ?)",
            &[
                Value::Int(1),
                Value::Text("u".into()),
                Value::Text("t".into()),
                Value::Int(1),
                Value::Text("ALWAYS_UPDATE".into()),
            ],
        )
        .await
        .unwrap();
        db.execute_sql("UPDATE manga SET url = ? WHERE id = ?", &[Value::Text("u2".into()), Value::Int(1)])
            .await
            .unwrap();
        let version: i64 = crate::query::query_scalar("SELECT version FROM manga WHERE id = ?")
            .bind(1i32)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(version, 1);
    }
}
