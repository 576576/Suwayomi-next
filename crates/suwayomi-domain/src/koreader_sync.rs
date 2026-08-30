//! KOReader progress sync — mirrors `KoreaderSyncService.kt`.
//!
//! Syncs per-chapter reading progress against a KOReader sync server
//! (default https://sync.koreader.rocks/). Credentials are stored in
//! `global_meta`; chapter identity is an MD5 of the `<title> - <chapter>`
//! filename (FILENAME checksum) or of the downloaded archive (BINARY).
//! Conflicts resolve per the configured forward/backward strategies.

use md5::{Digest, Md5};
use reqwest::Client;
use suwayomi_core::config::{KoreaderSyncConflictStrategy, ServerConfig};
use suwayomi_core::db::Db;

use crate::error::{DomainError, Result};

const KEY_SERVER: &str = "koreader_sync_server_address";
const KEY_USERNAME: &str = "koreader_sync_username";
const KEY_USER_KEY: &str = "koreader_sync_user_key";
const KEY_DEVICE_ID: &str = "koreader_sync_device_id";

const DEFAULT_SERVER: &str = "https://sync.koreader.rocks/";

/// Mirrors `KoSyncStatusPayload`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KoSyncStatusPayload {
    pub is_logged_in: bool,
    pub server_address: Option<String>,
    pub username: Option<String>,
}

/// Result of a pull attempt — mirrors `KoreaderSyncService.SyncResult`.
#[derive(Debug, Clone)]
pub struct SyncResult {
    pub page_read: i32,
    pub timestamp: i64,
    pub device: String,
    pub should_update: bool,
    pub is_conflict: bool,
}

#[derive(serde::Serialize)]
struct ProgressPayload<'a> {
    document: &'a str,
    progress: String,
    percentage: f32,
    device: &'a str,
    device_id: &'a str,
}

#[derive(serde::Deserialize, Default)]
#[allow(dead_code)] // response fields kept for serde shape
struct ProgressResponse {
    document: Option<String>,
    progress: Option<String>,
    percentage: Option<f32>,
    updated_at: Option<i64>,
    device: Option<String>,
    device_id: Option<String>,
}

/// KOReader sync service bound to a database (for credentials) and config.
#[derive(Clone)]
pub struct KoreaderSyncService {
    db: Db,
    config: ServerConfig,
    http: Client,
}

impl KoreaderSyncService {
    pub fn new(db: Db, config: ServerConfig) -> Self {
        Self {
            db,
            config,
            http: Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("reqwest client"),
        }
    }

    // ---- credential storage (global_meta) --------------------------------

    async fn meta_get(&self, key: &str) -> Result<Option<String>> {
        let v: Option<String> = sqlx::query_scalar(
            "SELECT value FROM suwayomi.global_meta WHERE meta_key = $1",
        )
        .bind(key)
        .fetch_optional(self.db.pool())
        .await?;
        Ok(v)
    }

    async fn meta_set(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO suwayomi.global_meta (meta_key, value) VALUES ($1, $2) \
             ON CONFLICT (meta_key) DO UPDATE SET value = EXCLUDED.value",
        )
        .bind(key)
        .bind(value)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }

    async fn meta_delete(&self, key: &str) -> Result<()> {
        sqlx::query("DELETE FROM suwayomi.global_meta WHERE meta_key = $1")
            .bind(key)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    fn md5(s: &str) -> String {
        let mut h = Md5::new();
        h.update(s.as_bytes());
        format!("{:x}", h.finalize())
    }

    async fn get_or_generate_device_id(&self) -> Result<String> {
        if let Some(id) = self.meta_get(KEY_DEVICE_ID).await? {
            if !id.is_empty() {
                return Ok(id);
            }
        }
        let id = uuid::Uuid::new_v4().simple().to_string().to_uppercase();
        self.meta_set(KEY_DEVICE_ID, &id).await?;
        Ok(id)
    }

    /// Mirrors `getOrGenerateChapterHash`: FILENAME → md5("<title> - <name>"
    /// without extension); BINARY falls back to the same when the chapter is
    /// not downloaded (archive hashing is not supported in this build).
    async fn get_or_generate_chapter_hash(&self, chapter_id: i32) -> Result<Option<String>> {
        struct Row {
            koreader_hash: Option<String>,
            name: Option<String>,
            manga_title: Option<String>,
        }
        let row: Option<Row> = sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>)>(
            "SELECT c.koreader_hash, c.name, m.title FROM suwayomi.chapter c \
             JOIN suwayomi.manga m ON m.id = c.manga WHERE c.id = $1",
        )
        .bind(chapter_id)
        .fetch_optional(self.db.pool())
        .await?
        .map(|(h, n, t)| Row { koreader_hash: h, name: n, manga_title: t });

        let Some(r) = row else { return Ok(None) };
        if let Some(h) = &r.koreader_hash {
            if !h.is_empty() {
                return Ok(Some(h.clone()));
            }
        }

        // BINARY would hash the downloaded CBZ; this build uses the filename
        // checksum for both (the server-side default is FILENAME anyway).
        let base_filename = match (&r.manga_title, &r.name) {
            (Some(t), Some(n)) => {
                let joined = format!("{t} - {n}");
                joined.split('.').collect::<Vec<_>>().split_last().map(|(_, rest)| rest.join(".")).unwrap_or(joined)
            }
            _ => return Ok(None),
        };
        let hash = Self::md5(&base_filename);
        sqlx::query("UPDATE suwayomi.chapter SET koreader_hash = $1 WHERE id = $2")
            .bind(&hash)
            .bind(chapter_id)
            .execute(self.db.pool())
            .await?;
        Ok(Some(hash))
    }

    // ---- HTTP plumbing ----------------------------------------------------

    fn build_request(&self, url: &str, method: &str, username: &str, user_key: &str) -> Result<reqwest::RequestBuilder> {
        let base = self
            .http
            .request(
                reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| DomainError::Source(format!("bad method {method}")))?,
                url,
            )
            .header("Accept", "application/vnd.koreader.v1+json")
            .header("Connection", "close")
            .header("x-auth-user", username)
            .header("x-auth-key", user_key);
        Ok(base)
    }

    async fn authorize(&self, server: &str, username: &str, user_key: &str) -> Result<(bool, Option<String>)> {
        let url = format!("{}/users/auth", server.trim_end_matches('/'));
        let req = self.build_request(&url, "GET", username, user_key)?.build().map_err(DomainError::from)?;
        match self.http.execute(req).await {
            Ok(resp) => {
                let code = resp.status().as_u16();
                if resp.status().is_success() {
                    Ok((true, None))
                } else {
                    Ok((false, Some(format!("Authorization failed with code {code}"))))
                }
            }
            Err(e) => Ok((false, Some(e.to_string()))),
        }
    }

    async fn register(&self, server: &str, username: &str, user_key: &str) -> Result<(bool, Option<String>)> {
        let url = format!("{}/users/create", server.trim_end_matches('/'));
        let body = serde_json::json!({ "username": username, "password": user_key });
        let req = self
            .http
            .post(&url)
            .header("Accept", "application/vnd.koreader.v1+json")
            .header("Connection", "close")
            .json(&body)
            .build()
            .map_err(DomainError::from)?;
        match self.http.execute(req).await {
            Ok(resp) => {
                let code = resp.status().as_u16();
                if resp.status().is_success() {
                    Ok((true, None))
                } else {
                    Ok((false, Some(format!("Registration failed with code {code}"))))
                }
            }
            Err(e) => Ok((false, Some(e.to_string()))),
        }
    }

    // ---- public API -------------------------------------------------------

    /// Mirrors `connect`: authorize, fall back to registration for unknown users.
    pub async fn connect(&self, server_address: &str, username: &str, password: &str) -> Result<(String, KoSyncStatusPayload)> {
        let user_key = Self::md5(password);
        let server = server_address.trim_end_matches('/').to_string();
        let (ok, msg) = self.authorize(&server, username, &user_key).await?;
        if ok {
            self.meta_set(KEY_SERVER, &server).await?;
            self.meta_set(KEY_USERNAME, username).await?;
            self.meta_set(KEY_USER_KEY, &user_key).await?;
            return Ok((
                "Login successful.".into(),
                KoSyncStatusPayload {
                    is_logged_in: true,
                    server_address: Some(server),
                    username: Some(username.into()),
                },
            ));
        }
        // 401 → try to register a new account
        let (ok2, msg2) = self.register(&server, username, &user_key).await?;
        if ok2 {
            self.meta_set(KEY_SERVER, &server).await?;
            self.meta_set(KEY_USERNAME, username).await?;
            self.meta_set(KEY_USER_KEY, &user_key).await?;
            return Ok((
                "Registration successful.".into(),
                KoSyncStatusPayload {
                    is_logged_in: true,
                    server_address: Some(server),
                    username: Some(username.into()),
                },
            ));
        }
        Ok((
            msg2.or(msg).unwrap_or_else(|| "Authentication failed.".into()),
            KoSyncStatusPayload { is_logged_in: false, server_address: None, username: None },
        ))
    }

    pub async fn logout(&self) -> Result<()> {
        for k in [KEY_SERVER, KEY_USERNAME, KEY_USER_KEY, KEY_DEVICE_ID] {
            self.meta_delete(k).await?;
        }
        Ok(())
    }

    pub async fn get_status(&self) -> Result<KoSyncStatusPayload> {
        let server = self.meta_get(KEY_SERVER).await?.unwrap_or_else(|| DEFAULT_SERVER.into());
        let username = self.meta_get(KEY_USERNAME).await?.unwrap_or_default();
        let user_key = self.meta_get(KEY_USER_KEY).await?.unwrap_or_default();
        if username.is_empty() || user_key.is_empty() {
            return Ok(KoSyncStatusPayload { is_logged_in: false, server_address: None, username: None });
        }
        let (ok, _) = self.authorize(&server, &username, &user_key).await?;
        Ok(KoSyncStatusPayload {
            is_logged_in: ok,
            server_address: ok.then(|| server.clone()),
            username: ok.then(|| username.clone()),
        })
    }

    /// Mirrors `pushProgress`: PUT current page progress to the server.
    pub async fn push_progress(&self, chapter_id: i32) -> Result<()> {
        let fwd = self.config.koreader_sync_strategy_forward;
        let back = self.config.koreader_sync_strategy_backward;
        if fwd == KoreaderSyncConflictStrategy::KeepRemote && back == KoreaderSyncConflictStrategy::KeepRemote {
            return Ok(()); // receive-only mode
        }
        let Some((server, username, user_key)) = self.credentials().await? else { return Ok(()) };
        let Some(hash) = self.get_or_generate_chapter_hash(chapter_id).await? else { return Ok(()) };

        let row: Option<(i32, i32)> = sqlx::query_as(
            "SELECT last_page_read, page_count FROM suwayomi.chapter WHERE id = $1",
        )
        .bind(chapter_id)
        .fetch_optional(self.db.pool())
        .await?;
        let Some((last_page_read, page_count)) = row else { return Ok(()) };
        if page_count <= 0 {
            return Ok(());
        }

        let device_id = self.get_or_generate_device_id().await?;
        let payload = ProgressPayload {
            document: &hash,
            progress: (last_page_read + 1).to_string(),
            percentage: (last_page_read + 1) as f32 / page_count as f32,
            device: "Suwayomi (next)",
            device_id: &device_id,
        };
        let url = format!("{}/syncs/progress", server.trim_end_matches('/'));
        let req = self
            .build_request(&url, "PUT", &username, &user_key)?
            .json(&payload)
            .build()
            .map_err(DomainError::from)?;
        let _ = self.http.execute(req).await;
        Ok(())
    }

    /// Mirrors `checkAndPullProgress`: fetch remote progress and apply the
    /// conflict strategy; returns the decided update, if any.
    pub async fn pull_progress(&self, chapter_id: i32) -> Result<Option<SyncResult>> {
        let fwd = self.config.koreader_sync_strategy_forward;
        let back = self.config.koreader_sync_strategy_backward;
        if (fwd == KoreaderSyncConflictStrategy::Disabled && back == KoreaderSyncConflictStrategy::Disabled)
            || (fwd == KoreaderSyncConflictStrategy::KeepLocal && back == KoreaderSyncConflictStrategy::KeepLocal)
        {
            return Ok(None);
        }
        let Some((server, username, user_key)) = self.credentials().await? else { return Ok(None) };
        let Some(hash) = self.get_or_generate_chapter_hash(chapter_id).await? else { return Ok(None) };

        let url = format!("{}/syncs/progress/{hash}", server.trim_end_matches('/'));
        let req = self
            .build_request(&url, "GET", &username, &user_key)?
            .build()
            .map_err(DomainError::from)?;
        let resp = match self.http.execute(req).await {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };
        if !resp.status().is_success() {
            return Ok(None);
        }
        let body = resp.text().await.unwrap_or_default();
        if body.is_empty() || body == "{}" {
            return Ok(None);
        }
        let progress: ProgressResponse = match serde_json::from_str(&body) {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        let page_read = match progress.progress.as_deref().and_then(|p| p.parse::<i32>().ok()) {
            Some(p) => p - 1,
            None => return Ok(None),
        };
        let Some(timestamp) = progress.updated_at else { return Ok(None) };
        // XPath progress (non-numeric) is not supported
        if progress.progress.as_deref().map(|p| p.starts_with('/')).unwrap_or(false) {
            return Ok(None);
        }
        let device = progress.device.unwrap_or_else(|| "KOReader".into());

        let local: Option<(i64, i32, i32)> = sqlx::query_as(
            "SELECT last_read_at, last_page_read, page_count FROM suwayomi.chapter WHERE id = $1",
        )
        .bind(chapter_id)
        .fetch_optional(self.db.pool())
        .await?;
        let (local_ts, local_page, local_pages) = local.unwrap_or((0, 0, 0));
        let local_pct = if local_pages > 0 { (local_page + 1) as f32 / local_pages as f32 } else { 0.0 };
        let remote_pct = progress.percentage.unwrap_or(0.0);
        if (local_pct - remote_pct).abs() < self.config.koreader_sync_percentage_tolerance {
            return Ok(None);
        }
        let is_remote_newer = timestamp > local_ts;
        let strategy = if is_remote_newer { fwd } else { back };
        match strategy {
            KoreaderSyncConflictStrategy::Prompt => Ok(Some(SyncResult {
                page_read,
                timestamp,
                device,
                should_update: false,
                is_conflict: true,
            })),
            KoreaderSyncConflictStrategy::KeepRemote => Ok(Some(SyncResult {
                page_read,
                timestamp,
                device,
                should_update: true,
                is_conflict: false,
            })),
            _ => Ok(None),
        }
    }

    async fn credentials(&self) -> Result<Option<(String, String, String)>> {
        let server = self.meta_get(KEY_SERVER).await?.unwrap_or_else(|| DEFAULT_SERVER.into());
        let username = self.meta_get(KEY_USERNAME).await?.unwrap_or_default();
        let user_key = self.meta_get(KEY_USER_KEY).await?.unwrap_or_default();
        if username.is_empty() || user_key.is_empty() {
            Ok(None)
        } else {
            Ok(Some((server, username, user_key)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suwayomi_core::config::{KoreaderSyncConflictStrategy, ServerConfig};

    async fn setup() -> KoreaderSyncService {
        let db = suwayomi_core::db::Db::connect_embedded(None).await.expect("db");
        db.migrate().await.expect("migrate");
        let cfg = ServerConfig {
            koreader_sync_strategy_forward: KoreaderSyncConflictStrategy::KeepRemote,
            koreader_sync_strategy_backward: KoreaderSyncConflictStrategy::KeepLocal,
            ..Default::default()
        };
        KoreaderSyncService::new(db, cfg)
    }

    #[tokio::test]
    async fn filename_hash_matches_kotlin_md5() {
        let svc = setup().await;
        // Insert manga + chapter to compute a hash.
        sqlx::query("INSERT INTO suwayomi.manga (url, title, source, initialized) VALUES ('/m/1', 'Manga Title', 1, FALSE)")
            .execute(svc.db.pool())
            .await
            .expect("manga");
        let mid: i32 = sqlx::query_scalar("SELECT id FROM suwayomi.manga WHERE url = '/m/1'")
            .fetch_one(svc.db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO suwayomi.chapter (url, name, manga, source_order) VALUES ('/c/1', 'Chapter 01.cbz', $1, 1)")
            .bind(mid)
            .execute(svc.db.pool())
            .await
            .expect("chapter");
        let cid: i32 = sqlx::query_scalar("SELECT id FROM suwayomi.chapter WHERE url = '/c/1'")
            .fetch_one(svc.db.pool())
            .await
            .unwrap();

        let hash = svc.get_or_generate_chapter_hash(cid).await.expect("hash").expect("non-null");
        // md5("Manga Title - Chapter 01")
        let mut h = Md5::new();
        h.update(b"Manga Title - Chapter 01");
        let expected = format!("{:x}", h.finalize());
        assert_eq!(hash, expected, "FILENAME checksum");
        // persisted
        let stored: String = sqlx::query_scalar("SELECT koreader_hash FROM suwayomi.chapter WHERE id = $1")
            .bind(cid)
            .fetch_one(svc.db.pool())
            .await
            .unwrap();
        assert_eq!(stored, hash, "hash persisted");
    }

    #[tokio::test]
    async fn credentials_roundtrip_and_idempotent_status() {
        let svc = setup().await;
        assert!(!svc.get_status().await.expect("status").is_logged_in, "no creds -> logged out");

        svc.meta_set(KEY_SERVER, "https://example.invalid/").await.expect("set");
        svc.meta_set(KEY_USERNAME, "alice").await.expect("set");
        svc.meta_set(KEY_USER_KEY, "key").await.expect("set");

        let status = svc.get_status().await.expect("status");
        // authorize against an unreachable server -> logged out, no crash
        assert!(!status.is_logged_in, "unreachable server -> logged out");

        svc.logout().await.expect("logout");
        assert_eq!(svc.meta_get(KEY_USERNAME).await.expect("get"), None, "creds cleared");
    }

    #[tokio::test]
    async fn push_without_credentials_is_noop() {
        let svc = setup().await;
        // no credentials -> should return Ok without any network
        let r = svc.push_progress(1).await;
        assert!(r.is_ok(), "push without creds is a noop");
        let r = svc.pull_progress(1).await;
        assert!(r.is_ok() && r.unwrap().is_none(), "pull without creds is a noop");
    }
}
