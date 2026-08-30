//! Server configuration — mirrors `suwayomi.tachidesk.server.ServerConfig`
//! and `server-config` module. Full settings registry lands with the
//! settings subsystem (Phase 3); this is the minimal core used by the DB layer.

use serde::{Deserialize, Serialize};

/// Mirrors `graphql/types/DatabaseType.kt`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatabaseType {
    H2,
    Postgresql,
}

/// Mirrors the core `ServerConfig` settings consumed at startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerConfig {
    pub ip: String,
    pub port: i32,
    pub database_type: DatabaseType,
    pub database_url: String,
    pub database_username: String,
    pub database_password: String,
    pub use_hikari_connection_pool: bool,
    pub initial_open_in_browser_enabled: bool,
    pub auth_mode: String,
    pub auth_username: String,
    pub auth_password: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            ip: "0.0.0.0".into(),
            port: 4567,
            database_type: DatabaseType::H2,
            database_url: String::new(),
            database_username: String::new(),
            database_password: String::new(),
            use_hikari_connection_pool: false,
            initial_open_in_browser_enabled: true,
            auth_mode: "DISABLED".into(),
            auth_username: String::new(),
            auth_password: String::new(),
        }
    }
}
