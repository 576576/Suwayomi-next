//! Backend configuration and its environment resolution.
//!
//! The default is a local SQLite file; PostgreSQL is the explicit alternative
//! selected by `SUWAYOMI_DB_BACKEND`. `SUWAYOMI_DB_URL` only says *where*
//! PostgreSQL is — a URL on its own never changes the backend, so a leftover
//! variable in a deployment's environment cannot silently point the server at
//! another database.

use std::path::{Path, PathBuf};

use crate::backend::BackendKind;

/// Environment variable selecting the backend explicitly (`sqlite`|`postgres`).
pub const ENV_BACKEND: &str = "SUWAYOMI_DB_BACKEND";
/// Environment variable holding the PostgreSQL connection URL.
pub const ENV_URL: &str = "SUWAYOMI_DB_URL";
/// Name of the SQLite database file (the directory comes from the appdata root,
/// see `AppPaths::db`).
pub const SQLITE_FILE_NAME: &str = "suwayomi.db";

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
        Self { kind: BackendKind::Postgres, path: PathBuf::new(), url: url.into() }
    }

    /// Resolves settings from the environment.
    ///
    /// * `SUWAYOMI_DB_BACKEND=postgres` → PostgreSQL (URL from `SUWAYOMI_DB_URL`).
    /// * anything else, including unset → SQLite.
    ///
    /// The SQLite file is always `<db_dir>/suwayomi.db`. `db_dir` is supplied by
    /// the caller (the appdata root's `db/`, or the Android host's private dir) —
    /// the database location has no environment variable of its own.
    pub fn from_env(db_dir: &Path) -> Self {
        let backend = std::env::var(ENV_BACKEND).ok();
        let kind = kind_from(backend.as_deref());
        let url = resolve_url();
        if kind == BackendKind::Sqlite && !url.is_empty() {
            tracing::warn!("{ENV_URL} is set but {ENV_BACKEND} is not `postgres`; ignoring the URL");
        }
        if kind == BackendKind::Postgres && url.is_empty() {
            tracing::warn!("{ENV_BACKEND}=postgres but {ENV_URL} is empty; there is nothing to connect to");
        }
        Self { kind, path: db_dir.join(SQLITE_FILE_NAME), url }
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

/// `SUWAYOMI_DB_BACKEND` → backend. Anything but an explicit `postgres` /
/// `postgresql` (including unset, empty and unknown words) means SQLite: a
/// typo'd switch must not silently move the library to another database.
fn kind_from(backend: Option<&str>) -> BackendKind {
    match backend.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("postgres" | "postgresql") => BackendKind::Postgres,
        _ => BackendKind::Sqlite,
    }
}

/// `SUWAYOMI_DB_URL`, trimmed; empty when unset.
fn resolve_url() -> String {
    std::env::var(ENV_URL).unwrap_or_default().trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_backend_maps_to_its_dialect() {
        assert_eq!(BackendKind::Sqlite.dialect(), crate::dialect::Dialect::Sqlite);
        assert_eq!(BackendKind::Postgres.dialect(), crate::dialect::Dialect::Postgres);
    }

    #[test]
    fn the_backend_comes_from_the_switch_and_nothing_else() {
        assert_eq!(kind_from(Some("postgres")), BackendKind::Postgres);
        assert_eq!(kind_from(Some("POSTGRESQL")), BackendKind::Postgres);
        assert_eq!(kind_from(Some(" postgres ")), BackendKind::Postgres);
        assert_eq!(kind_from(Some("sqlite")), BackendKind::Sqlite);
        // 写错的后端名与未设置同路：一个 typo 不该把库换到别处
        assert_eq!(kind_from(Some("mysql")), BackendKind::Sqlite);
        assert_eq!(kind_from(Some("")), BackendKind::Sqlite);
        assert_eq!(kind_from(None), BackendKind::Sqlite);
    }

    #[test]
    fn sqlite_settings_carry_the_path() {
        let s = DbSettings::sqlite("x/y.db");
        assert_eq!(s.kind, BackendKind::Sqlite);
        assert!(s.describe().contains("x/y.db"));
    }

    #[test]
    fn the_sqlite_file_lands_directly_under_the_given_dir() {
        // Only the path is asserted: the backend and URL come from the process
        // environment and do not affect where the file lives.
        let settings = DbSettings::from_env(Path::new("E:/suwayomi/appdata/db"));
        assert_eq!(settings.path, Path::new("E:/suwayomi/appdata/db/suwayomi.db"));
    }
}
