//! DB 连接管理：嵌入式 Oliphaunt（默认，进程内原生 PostgreSQL 18）或外部
//! PostgreSQL（fallback）。所有表在 `suwayomi` schema（同 M0054）。

use std::path::Path;
use std::sync::Arc;

use oliphaunt::Oliphaunt;
use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::db::migrator::MIGRATOR;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("embedded oliphaunt error: {0}")]
    Embedded(#[from] anyhow::Error),
}

/// Which database backend the server is running on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbMode {
    /// In-process Oliphaunt native server (default) — no external server required.
    Embedded,
    /// External PostgreSQL server via connection URL.
    External,
}

/// Database handle shared across the app (cloned per request via `Arc`).
#[derive(Clone)]
pub struct Db {
    pool: PgPool,
    /// 保持嵌入式 Oliphaunt server 存活（None=外部模式）。Arc 保证 Db 可廉价
    /// clone；最后一个 clone drop 时 server 随之关闭。
    _embedded: Option<Arc<Oliphaunt>>,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db")
            .field("mode", &self.mode())
            .finish_non_exhaustive()
    }
}

/// 池加固（同 DBManager.kt）：取出前 ping、回收闲置、封顶存活时长
fn hardened(options: PgPoolOptions) -> PgPoolOptions {
    options
        .test_before_acquire(true)
        .idle_timeout(std::time::Duration::from_secs(300))
        .max_lifetime(std::time::Duration::from_secs(3600))
        .acquire_timeout(std::time::Duration::from_secs(30))
}

impl Db {
    /// 连接外部 PostgreSQL（fallback）。每条连接 SET search_path=suwayomi（同 M0054）。
    pub async fn connect(url: &str) -> Result<Self, DbError> {
        // 池须 < Oliphaunt max_client_sessions(32)，否则并发 GraphQL 解析排队等连接挂死
        let pool = hardened(PgPoolOptions::new().max_connections(24))
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    sqlx::query("SET search_path TO suwayomi").execute(conn).await?;
                    Ok(())
                })
            })
            .connect(url)
            .await?;
        Ok(Self { pool, _embedded: None })
    }

    /// 注册 oliphaunt 原生资源：编译期 `OLIPHAUNT_RESOURCES_DIR`（本地构建）
    /// 存在则用，否则回退 exe 旁捆绑的 `oliphaunt-runtime/resources`
    /// （CI 包的编译路径是 runner 的、用户机器不存在；打包见 docs/release.md）。
    fn register_oliphaunt_resources() -> Result<(), DbError> {
        let Some(resources) = active_oliphaunt_resources_dir() else {
            return Err(DbError::Embedded(anyhow::anyhow!(
                "oliphaunt native resources unavailable: compiled path missing and no bundled oliphaunt-runtime/resources next to the executable"
            )));
        };
        let compiled_ok = option_env!("OLIPHAUNT_RESOURCES_DIR")
            .map(|dir| Path::new(dir).join("native-runtime").is_dir())
            .unwrap_or(false);
        if compiled_ok {
            oliphaunt::register_build_resources!()
                .map_err(|e| DbError::Embedded(anyhow::Error::new(e)))?;
        } else {
            oliphaunt::register_build_resources_dir(&resources)
                .map_err(|e| DbError::Embedded(anyhow::Error::new(e)))?;
            tracing::info!(
                "oliphaunt resources: compiled path missing, using bundled {}",
                resources.display()
            );
        }
        Ok(())
    }
    /// 连接嵌入式 Oliphaunt（默认）：Some(path)=持久库（initdb 首建后续复用），
    /// None=临时库（测试用）。原生 server 支持多会话，池可配多连接。
    pub async fn connect_embedded(data_dir: Option<&Path>) -> Result<Self, DbError> {
        Self::register_oliphaunt_resources()?;
        // 失败先重试一次：补拷 cache 里缺的 runtime DLL（Windows oliphaunt
        // 0.1.1 漏拷 bin/*.dll，initdb 报 STATUS_DLL_NOT_FOUND），并清掉上次
        // 非优雅退出残留的 postmaster（否则 postmaster.pid 锁阻塞下次启动）。
        let open = || async {
            // PG max_connections 需 ≥ 池(64)+内部会话，否则并发查询把 PG 打满
            let builder = Oliphaunt::builder().native_server().max_client_sessions(96);
            match data_dir {
                Some(dir) => builder.path(dir),
                None => builder.temporary(),
            }
            .open()
            .await
        };
        let server = match open().await {
            Ok(server) => server,
            Err(first) => {
                copy_runtime_bin_dlls_to_cache();
                cleanup_stale_postmaster(data_dir);
                open().await.map_err(|e| {
                    DbError::Embedded(anyhow::anyhow!(
                        "first attempt failed: {first}; retry after copying runtime DLLs and cleaning a stale postmaster failed: {e}"
                    ))
                })?
            }
        };
        let url = server
            .connection_string()
            .ok_or_else(|| DbError::Embedded(anyhow::anyhow!("embedded native server did not expose a connection string")))?;
        tracing::info!(%url, "embedded oliphaunt ready");
        let pool = hardened(PgPoolOptions::new().max_connections(64))
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    sqlx::query("SET search_path TO suwayomi").execute(conn).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await?;
        Ok(Self { pool, _embedded: Some(Arc::new(server)) })
    }

    /// Which backend this handle is running on.
    pub fn mode(&self) -> DbMode {
        if self._embedded.is_some() { DbMode::Embedded } else { DbMode::External }
    }

    /// Runs the schema migrations for the active backend.
    pub async fn migrate(&self) -> Result<(), DbError> {
        use sqlx::Executor;
        // 单连接串行迁移 + PG 咨询锁，避免并发 migrator 抢同一版本
        let mut conn = self.pool.acquire().await?;
        // 先建 schema：sqlx 的 _sqlx_migrations 非限定名经 search_path 解析
        conn.execute("CREATE SCHEMA IF NOT EXISTS suwayomi").await?;
        conn.execute("SELECT pg_advisory_lock(728232364)").await?;
        let r = MIGRATOR.run(&mut conn).await;
        // SyncYomi 版本触发的 PL/pgSQL（CREATE OR REPLACE，幂等）
        let f = conn
            .execute(include_str!("../../../../migrations/pg-only/0002_sync_functions.sql"))
            .await;
        let t = conn
            .execute(include_str!("../../../../migrations/pg-only/0002_sync_triggers.sql"))
            .await;
        conn.execute("SELECT pg_advisory_unlock(728232364)").await?;
        r?;
        f.and(t)?;
        Ok(())
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

/// 有效资源目录：编译期 OUT_DIR（本地构建）存在则用，否则 exe 上级
/// `oliphaunt-runtime/resources`（CI 包；server 在 bin/ 下，根=上上级）
fn active_oliphaunt_resources_dir() -> Option<std::path::PathBuf> {
    if let Some(dir) = option_env!("OLIPHAUNT_RESOURCES_DIR") {
        let p = Path::new(dir);
        if p.join("native-runtime").is_dir() {
            return Some(p.to_path_buf());
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = exe.parent().and_then(|b| b.parent()) {
            let bundled = root.join("oliphaunt-runtime").join("resources");
            if bundled.join("native-runtime").is_dir() {
                return Some(bundled);
            }
        }
    }
    None
}

/// Windows oliphaunt 0.1.1 补丁：materialize 的 runtime cache 缺 bin/*.dll
/// （postgres 链接所需）→ 首次 initdb STATUS_DLL_NOT_FOUND；把有效资源目录
/// 里的 DLL 拷进每个已有 cache bin/（幂等）。见 connect_embedded 的重试。
fn copy_runtime_bin_dlls_to_cache() {
    // DLL 源须与 register_oliphaunt_resources 同源解析（本地 OUT_DIR / CI 捆绑）
    let Some(resources_dir) = active_oliphaunt_resources_dir() else {
        return;
    };
    let src_bin = resources_dir.join("native-runtime/liboliphaunt-native/runtime/bin");
    if !src_bin.is_dir() {
        return;
    }
    let cache_root = std::env::var_os("OLIPHAUNT_RUNTIME_CACHE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("oliphaunt-runtime-cache"));
    let Ok(cache_entries) = std::fs::read_dir(&cache_root) else {
        return;
    };
    let Ok(dlls) = std::fs::read_dir(&src_bin) else {
        return;
    };
    let dlls: Vec<_> = dlls
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dll")))
        .collect();
    for entry in cache_entries.flatten() {
        let bin = entry.path().join("bin");
        if !bin.is_dir() {
            continue;
        }
        for dll in &dlls {
            let target = bin.join(dll.file_name().unwrap_or_default());
            if !target.exists() {
                let _ = std::fs::copy(dll, &target);
            }
        }
    }
}

/// 清掉上次非优雅退出残留的 postmaster（其子进程仍持 postmaster.pid 锁，
/// 阻塞下次启动）。只动可执行路径在 oliphaunt runtime cache 下的进程，绝不碰
/// 外部 PostgreSQL。
fn cleanup_stale_postmaster(data_dir: Option<&Path>) {
    use std::process::Command;

    let Some(dir) = data_dir else {
        return;
    };
    let pgdata = dir.join("pgdata");
    let pid_file = pgdata.join("postmaster.pid");
    let Ok(pid_text) = std::fs::read_to_string(&pid_file) else {
        return;
    };
    let Ok(pid) = pid_text.lines().next().unwrap_or("").trim().parse::<u32>() else {
        return;
    };
    // Windows: check the process image path contains our runtime cache.
    let image = Command::new("wmic")
        .args(["process", "where", &format!("ProcessId={pid}"), "get", "ExecutablePath", "/value"])
        .output()
        .ok();
    let is_ours = image
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .to_lowercase()
                .contains("oliphaunt-runtime-cache")
        })
        .unwrap_or(false);
    if !is_ours {
        tracing::warn!(pid, "postmaster.pid points to a foreign postgres; leaving it alone");
        return;
    }
    tracing::warn!(pid, "killing stale oliphaunt postmaster from an ungraceful shutdown");
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
    let _ = std::fs::remove_file(&pid_file);
    let _ = std::fs::remove_file(pgdata.join("postmaster.opts"));
}
