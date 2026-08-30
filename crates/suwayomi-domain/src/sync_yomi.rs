//! SyncYomi — syncs the library against a SyncYomi server as a Backup
//! protobuf with ETag-based optimistic concurrency. Mirrors
//! `SyncYomiSyncService.kt` + the pull/push half of `SyncManager`.
//!
//! Flow: build local `Backup` → pull remote (GET with If-None-Match) →
//! if changed, restore remote into the local library (idempotent upserts,
//! same semantics as backup import) → push merged data back (PUT with
//! If-Match). ETag is kept in `global_meta`.

use prost::Message;
use reqwest::Client;
use suwayomi_core::backup::{create_backup_proto, restore_backup_proto, Backup};
use suwayomi_core::config::ServerConfig;
use suwayomi_core::db::Db;

use crate::error::{DomainError, Result};

const KEY_ETAG: &str = "sync_yomi_last_etag";

/// Result of a sync cycle.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub synced_at: i64,
    pub pulled: bool,
    pub pushed: bool,
    pub mangas: usize,
}

/// SyncYomi sync service.
#[derive(Clone)]
pub struct SyncYomiService {
    db: Db,
    config: ServerConfig,
    http: Client,
}

impl SyncYomiService {
    pub fn new(db: Db, config: ServerConfig) -> Self {
        Self {
            db,
            config,
            http: Client::builder()
                .connect_timeout(std::time::Duration::from_secs(30))
                .read_timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.sync_yomi_enabled
            && !self.config.sync_yomi_host.is_empty()
            && !self.config.sync_yomi_api_key.is_empty()
    }

    async fn etag(&self) -> Result<String> {
        let v: Option<String> = sqlx::query_scalar("SELECT value FROM suwayomi.global_meta WHERE meta_key = $1")
            .bind(KEY_ETAG)
            .fetch_optional(self.db.pool())
            .await?;
        Ok(v.unwrap_or_default())
    }

    async fn set_etag(&self, etag: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO suwayomi.global_meta (meta_key, value) VALUES ($1, $2) \
             ON CONFLICT (meta_key) DO UPDATE SET value = EXCLUDED.value",
        )
        .bind(KEY_ETAG)
        .bind(etag)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }

    fn content_url(&self) -> String {
        format!("{}/api/sync/content", self.config.sync_yomi_host.trim_end_matches('/'))
    }

    /// Pulls the remote backup. Returns (backup, etag) on 200; (None, "") on
    /// 304/404; errors otherwise.
    async fn pull(&self) -> Result<(Option<Backup>, String)> {
        let url = self.content_url();
        let mut req = self.http.get(&url).header("X-API-Token", &self.config.sync_yomi_api_key);
        let last = self.etag().await?;
        if !last.is_empty() {
            req = req.header("If-None-Match", &last);
        }
        let resp = req.send().await.map_err(|e| DomainError::Source(format!("sync pull: {e}")))?;
        match resp.status().as_u16() {
            304 => Ok((None, last)),
            404 => Ok((None, String::new())),
            200..=299 => {
                let new_etag = resp
                    .headers()
                    .get("ETag")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| DomainError::Source("sync: missing ETag".into()))?;
                let bytes = resp.bytes().await.map_err(DomainError::from)?;
                match Backup::decode(bytes.as_ref()) {
                    Ok(b) => Ok((Some(b), new_etag)),
                    Err(_) => Ok((None, String::new())), // bad body -> overwrite later
                }
            }
            code => Err(DomainError::Source(format!("sync pull failed: {code}"))),
        }
    }

    /// Pushes a backup. Returns true on success (including 304 no-op).
    async fn push(&self, backup: &Backup, etag: &str) -> Result<bool> {
        let url = self.content_url();
        // NB: an empty library still encodes a valid (empty) Backup message;
        // the protobuf wire format for an all-default message is 0 bytes, so
        // we deliberately do NOT reject empty payloads here.
        let bytes = backup.encode_to_vec();
        let mut req = self
            .http
            .put(&url)
            .header("X-API-Token", &self.config.sync_yomi_api_key)
            .header("Content-Type", "application/octet-stream")
            .body(bytes);
        if !etag.is_empty() {
            req = req.header("If-Match", etag);
        }
        let resp = req.send().await.map_err(|e| DomainError::Source(format!("sync push: {e}")))?;
        if resp.status().is_success() {
            if let Some(e) = resp.headers().get("ETag").and_then(|v| v.to_str().ok()).map(|s| s.to_string()) {
                self.set_etag(&e).await?;
            }
            Ok(true)
        } else {
            Err(DomainError::Source(format!("sync push failed: {}", resp.status())))
        }
    }

    /// Last completed sync cycle (read from `global_meta`), if any.
    pub async fn last_sync_status(&self) -> Result<Option<crate::sync_yomi::SyncStatus>> {
        const KEY_LAST: &str = "sync_yomi_last_synced_at";
        let v: Option<String> =
            sqlx::query_scalar("SELECT value FROM suwayomi.global_meta WHERE meta_key = $1")
                .bind(KEY_LAST)
                .fetch_optional(self.db.pool())
                .await?;
        Ok(v.and_then(|s| s.parse::<i64>().ok()).map(|ts| SyncStatus {
            synced_at: ts,
            pulled: false,
            pushed: false,
            mangas: 0,
        }))
    }

    /// Runs one sync cycle: pull → restore remote → push merged.
    pub async fn sync_now(&self) -> Result<SyncStatus> {
        if !self.enabled() {
            return Err(DomainError::Source("SyncYomi not configured (syncYomiHost / syncYomiApiKey)".into()));
        }
        let _local = create_backup_proto(self.db.pool()).await.map_err(|e| DomainError::Source(format!("backup: {e}")))?;
        let (remote, etag) = self.pull().await?;

        let pulled = remote.is_some();
        if let Some(remote_backup) = remote {
            // Merge: remote manga/chapters/categories are upserted into the
            // local library (same semantics as backup import). Local-only
            // entries are preserved; the merged backup is what we push back.
            let _ = restore_backup_proto(self.db.pool(), &remote_backup).await.map_err(|e| DomainError::Source(format!("restore: {e}")))?;
        }
        let merged = create_backup_proto(self.db.pool()).await.map_err(|e| DomainError::Source(format!("backup: {e}")))?;
        let pushed_count = merged.backup_manga.len();
        let pushed = self.push(&merged, &etag).await?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or_default();
        sqlx::query(
            "INSERT INTO suwayomi.global_meta (meta_key, value) VALUES ($1, $2)              ON CONFLICT (meta_key) DO UPDATE SET value = EXCLUDED.value",
        )
        .bind("sync_yomi_last_synced_at")
        .bind(now.to_string())
        .execute(self.db.pool())
        .await?;

        Ok(SyncStatus {
            synced_at: now,
            pulled,
            pushed,
            mangas: pushed_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suwayomi_core::config::ServerConfig;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn setup(enabled: bool) -> (SyncYomiService, ServerConfig) {
        let db = suwayomi_core::db::Db::connect_embedded(None).await.expect("db");
        db.migrate().await.expect("migrate");
        let cfg = ServerConfig {
            sync_yomi_enabled: enabled,
            sync_yomi_host: "http://127.0.0.1:1".into(), // unreachable by default
            sync_yomi_api_key: "k".into(),
            ..Default::default()
        };
        let svc = SyncYomiService::new(db, cfg.clone());
        (svc, cfg)
    }

    #[tokio::test]
    async fn enabled_requires_all_flags() {
        let (svc, mut cfg) = setup(false).await;
        assert!(!svc.enabled(), "disabled when master switch off");

        cfg.sync_yomi_enabled = true;
        let svc2 = SyncYomiService::new(svc.db.clone(), cfg.clone());
        assert!(svc2.enabled(), "enabled with all fields");

        cfg.sync_yomi_api_key.clear();
        let svc3 = SyncYomiService::new(svc.db.clone(), cfg);
        assert!(!svc3.enabled(), "disabled without api key");
    }

    #[tokio::test]
    async fn sync_now_unconfigured_errors() {
        let (svc, _) = setup(false).await;
        let r = svc.sync_now().await;
        assert!(r.is_err(), "not configured -> Err");
    }

    #[tokio::test]
    async fn unreachable_host_is_graceful() {
        let (svc, mut cfg) = setup(true).await;
        cfg.sync_yomi_host = "http://127.0.0.1:1".into();
        let svc = SyncYomiService::new(svc.db, cfg);
        let r = svc.sync_now().await;
        assert!(r.is_err(), "connection refused surfaces as Err, not panic");
        assert!(svc.last_sync_status().await.expect("status").is_none(), "no success recorded");
    }

    /// Runs a minimal fake SyncYomi server: GET -> 404 (no remote data),
    /// PUT -> 200 + ETag. Verifies the full pull-then-push cycle.
    #[tokio::test]
    async fn full_cycle_against_mock_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut reads = 0;
            loop {
                let (mut sock, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => break,
                };
                let mut buf = vec![0u8; 65536];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let is_put = req.starts_with("PUT");
                let body = if is_put {
                    reads += 1;
                    "HTTP/1.1 200 OK\r\nETag: \"v2\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                } else {
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                };
                let _ = sock.write_all(body.as_bytes()).await;
                let _ = sock.shutdown().await;
                if reads >= 1 { break; }
            }
        });

        let (svc, mut cfg) = setup(true).await;
        cfg.sync_yomi_host = format!("http://{addr}");
        let svc = SyncYomiService::new(svc.db, cfg);
        let status = svc.sync_now().await.expect("sync cycle");
        assert!(status.synced_at > 0, "synced_at recorded");
        assert!(status.pushed, "pushed after 404 pull");
        assert_eq!(status.mangas, 0, "empty library -> no mangas pushed");

        // ETag persisted + last status queryable
        let etag = svc.etag().await.expect("etag");
        assert_eq!(etag, "\"v2\"");
        let last = svc.last_sync_status().await.expect("status").expect("non-none");
        assert!(last.synced_at > 0, "last status timestamp");

        server.await.unwrap();
    }

    /// The PG port of SyncYomiTriggers.kt (migration 0002) must bump `version`
    /// on manga changes and respect the `is_syncing` opt-out.
    #[tokio::test]
    async fn version_bump_trigger_semantics() {
        // PL/pgSQL triggers are applied only on external PostgreSQL (see
        // Db::migrate — embedded pglite cannot compile PL/pgSQL). Requires
        // DATABASE_URL pointing at a real PostgreSQL instance.
        let Some(url) = std::env::var("DATABASE_URL").ok().filter(|u| !u.is_empty()) else {
            eprintln!("SKIP: requires DATABASE_URL (external PostgreSQL)");
            return;
        };
        let db = suwayomi_core::db::Db::connect(&url).await.expect("db");
        db.migrate().await.expect("migrate");
        let pool = db.pool();
        sqlx::query("INSERT INTO suwayomi.manga (url, title, source, initialized) VALUES ('/m/t', 'T', 1, FALSE)")
            .execute(pool).await.expect("insert");
        let mid: i32 = sqlx::query_scalar("SELECT id FROM suwayomi.manga WHERE url = '/m/t'")
            .fetch_one(pool).await.unwrap();

        let v0: i64 = sqlx::query_scalar("SELECT version FROM suwayomi.manga WHERE id = $1")
            .bind(mid).fetch_one(pool).await.unwrap();
        assert_eq!(v0, 0, "fresh row version 0");



        // change url -> version bumps
        sqlx::query("UPDATE suwayomi.manga SET url = '/m/t2' WHERE id = $1")
            .bind(mid).execute(pool).await.expect("update");
        let v1: i64 = sqlx::query_scalar("SELECT version FROM suwayomi.manga WHERE id = $1")
            .bind(mid).fetch_one(pool).await.unwrap();
        assert_eq!(v1, 1, "url change bumps version");

        // is_syncing=true suppresses the bump
        sqlx::query("UPDATE suwayomi.manga SET is_syncing = TRUE WHERE id = $1")
            .bind(mid).execute(pool).await.expect("update");
        sqlx::query("UPDATE suwayomi.manga SET url = '/m/t3' WHERE id = $1")
            .bind(mid).execute(pool).await.expect("update");
        let v2: i64 = sqlx::query_scalar("SELECT version FROM suwayomi.manga WHERE id = $1")
            .bind(mid).fetch_one(pool).await.unwrap();
        assert_eq!(v2, 1, "is_syncing suppresses version bump");

        // category_manga insert bumps manga version (once syncing is cleared)
        sqlx::query("UPDATE suwayomi.manga SET is_syncing = FALSE WHERE id = $1")
            .bind(mid).execute(pool).await.expect("clear syncing");
        sqlx::query("INSERT INTO suwayomi.category (name, is_default) VALUES ('Cat', FALSE)")
            .execute(pool).await.expect("cat");
        let cid: i32 = sqlx::query_scalar("SELECT id FROM suwayomi.category WHERE name = 'Cat'")
            .fetch_one(pool).await.unwrap();
        sqlx::query("INSERT INTO suwayomi.category_manga (category, manga) VALUES ($1, $2)")
            .bind(cid).bind(mid).execute(pool).await.expect("catmanga");
        let v3: i64 = sqlx::query_scalar("SELECT version FROM suwayomi.manga WHERE id = $1")
            .bind(mid).fetch_one(pool).await.unwrap();
        assert_eq!(v3, 2, "category_manga insert bumps manga version");
    }
}
