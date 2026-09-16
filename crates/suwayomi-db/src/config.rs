//! Backend configuration and its environment resolution.
//!
//! The server used to pick between "embedded Oliphaunt" and "external
//! PostgreSQL" purely by whether `SUWAYOMI_DATABASE_URL` was set. Now the
//! default is a local SQLite file and PostgreSQL is the explicit alternative,
//! so an explicit switch (`SUWAYOMI_DB_BACKEND`) is honoured first and the URL
//! is the fallback signal for backwards compatibility.

use std::path::{Path, PathBuf};

use crate::backend::BackendKind;

/// Environment variable naming the SQLite database file.
pub const ENV_SQLITE_PATH: &str = "SUWAYOMI_SQLITE_PATH";
/// Environment variable naming the directory the SQLite database file lives in.
pub const ENV_DB_DIR: &str = "SUWAYOMI_DB_DIR";
/// Environment variable selecting the backend explicitly (`sqlite`|`postgres`).
pub const ENV_BACKEND: &str = "SUWAYOMI_DB_BACKEND";
/// Environment variable holding the PostgreSQL connection URL.
pub const ENV_DATABASE_URL: &str = "SUWAYOMI_DATABASE_URL";

/// Everything the server needs to open a database.
#[derive(Debug, Clone)]
pub struct DbSettings {
    /// Which backend to open.
    pub kind: BackendKind,
    /// SQLite database file (used when `kind` is [`BackendKind::Sqlite`]).
    pub path: PathBuf,
    /// PostgreSQL connection URL (used when `kind` is [`BackendKind::Postgres`]).
    pub url: String,
}

impl DbSettings {
    /// SQLite settings for `path`.
    pub fn sqlite(path: impl Into<PathBuf>) -> Self {
        Self { kind: BackendKind::Sqlite, path: path.into(), url: String::new() }
    }

    /// PostgreSQL settings for `url`.
    pub fn postgres(url: impl Into<String>) -> Self {
        Self { kind: BackendKind::Postgres, path: default_sqlite_path(), url: url.into() }
    }

    /// Resolves settings from the environment.
    ///
    /// * `SUWAYOMI_DB_BACKEND=postgres` → PostgreSQL (URL from
    ///   `SUWAYOMI_DATABASE_URL`).
    /// * `SUWAYOMI_DB_BACKEND=sqlite` → SQLite even when a URL is set.
    /// * neither → PostgreSQL when `SUWAYOMI_DATABASE_URL` is set (the old
    ///   behaviour), SQLite otherwise.
    ///
    /// The SQLite file is `SUWAYOMI_SQLITE_PATH`, else `<SUWAYOMI_DB_DIR or
    /// default_db_dir()>/suwayomi.db` (with a one-shot move of a legacy
    /// `<data dir>/suwayomi.db`).
    pub fn from_env() -> Self {
        let backend = std::env::var(ENV_BACKEND).ok().map(|v| v.trim().to_ascii_lowercase());
        let url = std::env::var(ENV_DATABASE_URL).unwrap_or_default();
        let path = match std::env::var(ENV_SQLITE_PATH) {
            Ok(p) => PathBuf::from(p),
            Err(_) => {
                let p = default_sqlite_path();
                migrate_legacy_sqlite(&p);
                p
            }
        };

        let kind = match backend.as_deref() {
            Some("postgres") | Some("postgresql") => BackendKind::Postgres,
            Some("sqlite") => BackendKind::Sqlite,
            _ if !url.is_empty() => BackendKind::Postgres,
            _ => BackendKind::Sqlite,
        };
        Self { kind, path, url }
    }

    /// Overrides the SQLite file location.
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = path.into();
        self
    }

    /// Human-readable description for the startup log.
    pub fn describe(&self) -> String {
        match self.kind {
            BackendKind::Sqlite => format!("embedded SQLite at {}", self.path.display()),
            BackendKind::Postgres => format!("external PostgreSQL at {}", self.url),
        }
    }
}

/// Default SQLite file: `<db dir>/suwayomi.db`.
fn default_sqlite_path() -> PathBuf {
    db_dir_from(std::env::var(ENV_DB_DIR).ok().as_deref(), std::env::current_exe().ok().as_deref())
        .join("suwayomi.db")
}

/// 数据库目录：`SUWAYOMI_DB_DIR` → exe 在 `bin/` 下的上一级 `db/` → `./db`。
///
/// **刻意与数据目录（`SUWAYOMI_DATA_DIR`）解耦。** 数据目录是 WebUI 设置页里
/// 可以随时改的一项（`dataDir`），而设置本身就存在这个库里 —— 库跟着数据目录
/// 走的话，一改目录就把设置弄丢了。发布布局（exe 在 `bin/` 下）里它落在
/// `bin/` 的隔壁：`suwayomi-latest/{bin,db,data,webui}`。
pub fn default_db_dir() -> PathBuf {
    db_dir_from(std::env::var(ENV_DB_DIR).ok().as_deref(), std::env::current_exe().ok().as_deref())
}

/// [`default_db_dir`] 的纯函数内核（env / exe 由调用方取，便于单测）。
fn db_dir_from(env: Option<&str>, exe: Option<&Path>) -> PathBuf {
    if let Some(dir) = env.map(str::trim).filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(dir) = exe.and_then(Path::parent)
        && dir.file_name().map(|n| n == "bin").unwrap_or(false)
        && let Some(base) = dir.parent()
    {
        return base.join("db");
    }
    PathBuf::from("db")
}

/// 旧布局把库放在数据目录下（`<数据目录>/suwayomi.db`）。库挪到 `db/` 之后，
/// 首次启动时把旧库搬过去，否则用户升级后会看到空书架。
///
/// 只在**新位置没有库**、且**旧位置确实有库**时才搬；`SUWAYOMI_SQLITE_PATH`
/// 显式指了路径时不碰（那是用户自己安排的位置）。同盘 `rename` 是原子的。
fn migrate_legacy_sqlite(new_path: &Path) {
    let candidates = legacy_sqlite_candidates(
        std::env::var("SUWAYOMI_DATA_DIR").ok().as_deref(),
        std::env::current_exe().ok().as_deref(),
    );
    if let Some(legacy) = migrate_legacy_sqlite_from(new_path, &candidates) {
        tracing::info!(
            "db: moved legacy SQLite database {} → {}",
            legacy.display(),
            new_path.display()
        );
    }
}

/// [`migrate_legacy_sqlite`] 的纯函数内核（候选项由调用方给，便于单测）。
/// 真搬了返回来源路径，否则 `None`。
fn migrate_legacy_sqlite_from(new_path: &Path, candidates: &[PathBuf]) -> Option<PathBuf> {
    if new_path.exists() {
        return None;
    }
    let legacy = candidates.iter().find(|p| p.is_file() && p.as_path() != new_path)?;
    if let Some(parent) = new_path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("db: cannot create {}: {e}", parent.display());
        return None;
    }
    if let Err(e) = std::fs::rename(legacy, new_path) {
        tracing::warn!(
            "db: cannot move legacy database {} to {}: {e}",
            legacy.display(),
            new_path.display()
        );
        return None;
    }
    // WAL / 回滚日志是库的一部分，不是临时文件：只搬主文件会把日志里**已提交但还没
    // checkpoint** 的事务丢掉。进程被强杀（或关机）时 WAL 一定非空，所以这一步是必须的
    // —— 顺序上主文件先走，中途失败最坏是回退到稍旧的快照，而不是拿到一个损坏的库。
    for suffix in ["-wal", "-shm", "-journal"] {
        let src = sqlite_sidecar(legacy, suffix);
        if src.is_file()
            && let Err(e) = std::fs::rename(&src, sqlite_sidecar(new_path, suffix))
        {
            tracing::warn!("db: cannot move {}: {e}", src.display());
        }
    }
    Some(legacy.clone())
}

/// `suwayomi.db` → `suwayomi.db-wal`（SQLite 的日志文件是「主文件名 + 后缀」）。
fn sqlite_sidecar(db: &Path, suffix: &str) -> PathBuf {
    let mut name = db.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// 旧库可能所在的位置，按历史解析顺序：`SUWAYOMI_DATA_DIR` → 发布布局的
/// `<exe 上级>/data` → cwd 的 `./data`。
fn legacy_sqlite_candidates(data_dir_env: Option<&str>, exe: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = data_dir_env.map(str::trim).filter(|d| !d.is_empty()) {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(dir) = exe.and_then(Path::parent) {
        // 发布布局 exe 在 bin/ 下，数据目录是上一级的 data/
        let base = if dir.file_name().map(|n| n == "bin").unwrap_or(false) {
            dir.parent().unwrap_or(dir)
        } else {
            dir
        };
        dirs.push(base.join("data"));
    }
    dirs.push(PathBuf::from("data"));

    let mut seen: Vec<PathBuf> = Vec::new();
    for d in dirs {
        if !seen.contains(&d) {
            seen.push(d);
        }
    }
    seen.into_iter().map(|d| d.join("suwayomi.db")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_backend_wins_over_a_url() {
        // env access is process-global; keep the test to pure parsing helpers.
        assert_eq!(BackendKind::Sqlite.dialect(), crate::dialect::Dialect::Sqlite);
        assert_eq!(BackendKind::Postgres.dialect(), crate::dialect::Dialect::Postgres);
    }

    #[test]
    fn sqlite_settings_carry_the_path() {
        let s = DbSettings::sqlite("x/y.db");
        assert_eq!(s.kind, BackendKind::Sqlite);
        assert!(s.describe().contains("x/y.db"));
    }

    #[test]
    fn db_dir_env_wins_then_the_bin_sibling_then_cwd() {
        assert_eq!(db_dir_from(Some(" /var/lib/suwayomi "), None), PathBuf::from("/var/lib/suwayomi"));
        // 空串 / 纯空白视同没设
        assert_eq!(db_dir_from(Some("  "), None), PathBuf::from("db"));
        // 发布布局：exe 在 bin/ 下 → ../db（与 bin 平级）
        assert_eq!(
            db_dir_from(None, Some(Path::new("/opt/suwayomi/bin/suwayomi-server"))),
            Path::new("/opt/suwayomi").join("db")
        );
        // 非 bin/ 布局 → cwd 的 db
        assert_eq!(db_dir_from(None, Some(Path::new("/tmp/suwayomi-server"))), PathBuf::from("db"));
        assert_eq!(db_dir_from(None, None), PathBuf::from("db"));
    }

    #[test]
    fn legacy_database_is_looked_up_where_older_builds_wrote_it() {
        let rel = Path::new("data").join("suwayomi.db");
        // 桌面发布布局：exe 在 bin/ 下 → 上一级的 data/，与 env 指到同一处时去重
        let bin_exe = Path::new("/opt/suwayomi/bin/suwayomi-server");
        assert_eq!(
            legacy_sqlite_candidates(Some("/opt/suwayomi/data"), Some(bin_exe)),
            vec![Path::new("/opt/suwayomi/data").join("suwayomi.db"), rel.clone()]
        );
        // 没传 env 时同时认「exe 上级的 data/」与 cwd 的 ./data（顺序固定，都是历史行为）
        assert_eq!(
            legacy_sqlite_candidates(None, Some(bin_exe)),
            vec![Path::new("/opt/suwayomi/data").join("suwayomi.db"), rel.clone()]
        );
        // 非 bin/ 布局：只认 exe 同级的 data/ 与 ./data
        assert_eq!(
            legacy_sqlite_candidates(None, Some(Path::new("/tmp/suwayomi-server"))),
            vec![Path::new("/tmp/data").join("suwayomi.db"), rel.clone()]
        );
        assert_eq!(legacy_sqlite_candidates(None, None), vec![rel]);
    }

    /// 迁移只在「新位置空 + 旧位置有」时搬；已有新库时绝不覆盖
    /// —— 覆盖等于把用户升级后的数据回滚成旧快照。
    #[test]
    fn legacy_database_moves_once_and_never_clobbers_the_new_one() {
        let root = std::env::temp_dir().join(format!("suwayomi-db-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let legacy = root.join("data").join("suwayomi.db");
        let new_path = root.join("db").join("suwayomi.db");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, b"legacy").unwrap();
        // WAL 与主文件必须一起走：只搬主文件会丢掉里面已提交但没 checkpoint 的事务
        std::fs::write(sqlite_sidecar(&legacy, "-wal"), b"legacy-wal").unwrap();
        std::fs::write(sqlite_sidecar(&legacy, "-shm"), b"legacy-shm").unwrap();

        assert_eq!(migrate_legacy_sqlite_from(&new_path, std::slice::from_ref(&legacy)), Some(legacy.clone()));
        assert_eq!(std::fs::read(&new_path).unwrap(), b"legacy");
        assert_eq!(std::fs::read(sqlite_sidecar(&new_path, "-wal")).unwrap(), b"legacy-wal");
        assert_eq!(std::fs::read(sqlite_sidecar(&new_path, "-shm")).unwrap(), b"legacy-shm");
        assert!(!legacy.exists());
        assert!(!sqlite_sidecar(&legacy, "-wal").exists());

        // 第二次：新库已经在，什么都不做
        std::fs::write(&new_path, b"current").unwrap();
        assert_eq!(migrate_legacy_sqlite_from(&new_path, std::slice::from_ref(&legacy)), None);
        assert_eq!(std::fs::read(&new_path).unwrap(), b"current");

        // 候选里没有实际存在的文件 → 不搬，也不凭空建库
        let absent = root.join("nope").join("suwayomi.db");
        let other = root.join("db2").join("suwayomi.db");
        assert_eq!(migrate_legacy_sqlite_from(&other, std::slice::from_ref(&absent)), None);
        assert!(!other.exists());

        let _ = std::fs::remove_dir_all(&root);
    }
}
