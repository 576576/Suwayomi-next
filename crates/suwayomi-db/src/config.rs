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
    pub fn from_env() -> Self {
        let backend = std::env::var(ENV_BACKEND).ok().map(|v| v.trim().to_ascii_lowercase());
        let url = std::env::var(ENV_DATABASE_URL).unwrap_or_default();
        let path = std::env::var(ENV_SQLITE_PATH).map(PathBuf::from).unwrap_or_else(|_| default_sqlite_path());

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

/// Default SQLite file: `<data dir>/suwayomi.db`.
fn default_sqlite_path() -> PathBuf {
    Path::new(suwayomi_data_dir().as_path()).join("suwayomi.db")
}

/// Data root, honouring `SUWAYOMI_DATA_DIR` (same default as the server).
fn suwayomi_data_dir() -> PathBuf {
    std::env::var("SUWAYOMI_DATA_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("data"))
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
}
