//! 追踪器凭据与偏好存储（`tracker_credential` 表）。
//!
//! 上游把这些值放在客户端侧的 SharedPreferences（`TrackerPreferences.kt`）；
//! 服务端没有等价物，改落库，重启后登录态还在。
//!
//! 各列对应上游的键：`username` ↔ `pref_mangasync_username_{id}`、`password` ↔
//! `pref_mangasync_password_{id}`、`token` ↔ `track_token_{id}`、
//! `token_expired` ↔ `track_token_expired_{id}`、`score_type` ↔
//! `score_type_{id}`。注意 OAuth 类追踪器把 **access token 存在 password 列**
//! （上游 `saveCredentials(username, oauth.accessToken)`），`token` 列存的是整份
//! OAuth JSON，供刷新用；MangaUpdates 的 session token 同样在 password 列。

use suwayomi_core::db::Db;

use crate::error::Result;

/// 一行凭据。缺失时各字段为空串 / false。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackerCredential {
    pub username: String,
    pub password: String,
    pub token: String,
    pub token_expired: bool,
    pub score_type: String,
    pub pkce_verifier: String,
}

#[derive(Debug, Clone)]
pub struct TrackerStore {
    db: Db,
}

impl TrackerStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub async fn get(&self, tracker_id: i32) -> Result<TrackerCredential> {
        let row = suwayomi_db::query_as::<CredentialRow>(
            "SELECT username, password, token, token_expired, score_type, pkce_verifier \
             FROM tracker_credential WHERE tracker_id = ?",
        )
        .bind(tracker_id)
        .fetch_optional(&self.db)
        .await?;
        Ok(row.map(|r| r.into()).unwrap_or_default())
    }

    pub async fn username(&self, tracker_id: i32) -> Result<String> {
        Ok(self.get(tracker_id).await?.username)
    }

    pub async fn password(&self, tracker_id: i32) -> Result<String> {
        Ok(self.get(tracker_id).await?.password)
    }

    pub async fn token(&self, tracker_id: i32) -> Result<String> {
        Ok(self.get(tracker_id).await?.token)
    }

    pub async fn token_expired(&self, tracker_id: i32) -> Result<bool> {
        Ok(self.get(tracker_id).await?.token_expired)
    }

    pub async fn score_type(&self, tracker_id: i32) -> Result<String> {
        Ok(self.get(tracker_id).await?.score_type)
    }

    pub async fn pkce_verifier(&self, tracker_id: i32) -> Result<String> {
        Ok(self.get(tracker_id).await?.pkce_verifier)
    }

    /// 对应上游 `TrackerPreferences.setTrackCredentials`：写用户名/密码，并把
    /// 过期标记清掉（重新登录后旧标记必须失效）。
    pub async fn set_credentials(&self, tracker_id: i32, username: &str, password: &str) -> Result<()> {
        suwayomi_db::query(
            "INSERT INTO tracker_credential (tracker_id, username, password, token, token_expired, score_type, pkce_verifier) \
             VALUES (?, ?, ?, '', FALSE, '', '') \
             ON CONFLICT (tracker_id) DO UPDATE SET username = ?, password = ?, token_expired = FALSE",
        )
        .bind(tracker_id)
        .bind(username)
        .bind(password)
        .bind(username)
        .bind(password)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// 对应 `setTrackToken`：空串等同于 upstream 的 `null`（清掉 token 并复位过期标记）。
    pub async fn set_token(&self, tracker_id: i32, token: &str) -> Result<()> {
        suwayomi_db::query(
            "UPDATE tracker_credential SET token = ?, token_expired = FALSE WHERE tracker_id = ?",
        )
        .bind(token)
        .bind(tracker_id)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// 对应 `setTrackTokenExpired`。行不存在时先建行，否则标记写不进去。
    pub async fn set_token_expired(&self, tracker_id: i32, expired: bool) -> Result<()> {
        suwayomi_db::query(
            "INSERT INTO tracker_credential (tracker_id, username, password, token, token_expired, score_type, pkce_verifier) \
             VALUES (?, '', '', '', ?, '', '') \
             ON CONFLICT (tracker_id) DO UPDATE SET token_expired = ?",
        )
        .bind(tracker_id)
        .bind(expired)
        .bind(expired)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn set_score_type(&self, tracker_id: i32, score_type: &str) -> Result<()> {
        suwayomi_db::query(
            "INSERT INTO tracker_credential (tracker_id, username, password, token, token_expired, score_type, pkce_verifier) \
             VALUES (?, '', '', '', FALSE, ?, '') \
             ON CONFLICT (tracker_id) DO UPDATE SET score_type = ?",
        )
        .bind(tracker_id)
        .bind(score_type)
        .bind(score_type)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn set_pkce_verifier(&self, tracker_id: i32, verifier: &str) -> Result<()> {
        suwayomi_db::query(
            "INSERT INTO tracker_credential (tracker_id, username, password, token, token_expired, score_type, pkce_verifier) \
             VALUES (?, '', '', '', FALSE, '', ?) \
             ON CONFLICT (tracker_id) DO UPDATE SET pkce_verifier = ?",
        )
        .bind(tracker_id)
        .bind(verifier)
        .bind(verifier)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// 对应上游 `Tracker.logout()` 的默认实现（只清凭据）。清完即未登录。
    pub async fn clear_credentials(&self, tracker_id: i32) -> Result<()> {
        suwayomi_db::query(
            "UPDATE tracker_credential SET username = '', password = '', token = '', token_expired = FALSE WHERE tracker_id = ?",
        )
        .bind(tracker_id)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn clear_pkce_verifier(&self, tracker_id: i32) -> Result<()> {
        suwayomi_db::query("UPDATE tracker_credential SET pkce_verifier = '' WHERE tracker_id = ?")
            .bind(tracker_id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    /// 备份用：整表导出（含凭据）。
    pub async fn dump_all(&self) -> Result<Vec<(i32, TrackerCredential)>> {
        let rows = suwayomi_db::query_as::<FullCredentialRow>(
            "SELECT tracker_id, username, password, token, token_expired, score_type, pkce_verifier \
             FROM tracker_credential ORDER BY tracker_id",
        )
        .fetch_all(&self.db)
        .await?;
        Ok(rows.into_iter().map(FullCredentialRow::into_parts).collect())
    }

    /// 备份恢复用：整行覆盖写。
    pub async fn restore(&self, tracker_id: i32, c: &TrackerCredential) -> Result<()> {
        suwayomi_db::query(
            "INSERT INTO tracker_credential (tracker_id, username, password, token, token_expired, score_type, pkce_verifier) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (tracker_id) DO UPDATE SET username = ?, password = ?, token = ?, token_expired = ?, score_type = ?, pkce_verifier = ?",
        )
        .bind(tracker_id)
        .bind(&c.username)
        .bind(&c.password)
        .bind(&c.token)
        .bind(c.token_expired)
        .bind(&c.score_type)
        .bind(&c.pkce_verifier)
        .bind(&c.username)
        .bind(&c.password)
        .bind(&c.token)
        .bind(c.token_expired)
        .bind(&c.score_type)
        .bind(&c.pkce_verifier)
        .execute(&self.db)
        .await?;
        Ok(())
    }
}

#[derive(Debug, Clone, suwayomi_db::FromRow)]
struct CredentialRow {
    username: String,
    password: String,
    token: String,
    token_expired: bool,
    score_type: String,
    pkce_verifier: String,
}

impl From<CredentialRow> for TrackerCredential {
    fn from(r: CredentialRow) -> Self {
        Self {
            username: r.username,
            password: r.password,
            token: r.token,
            token_expired: r.token_expired,
            score_type: r.score_type,
            pkce_verifier: r.pkce_verifier,
        }
    }
}

#[derive(Debug, Clone, suwayomi_db::FromRow)]
struct FullCredentialRow {
    tracker_id: i32,
    username: String,
    password: String,
    token: String,
    token_expired: bool,
    score_type: String,
    pkce_verifier: String,
}

impl FullCredentialRow {
    fn into_parts(self) -> (i32, TrackerCredential) {
        (
            self.tracker_id,
            TrackerCredential {
                username: self.username,
                password: self.password,
                token: self.token,
                token_expired: self.token_expired,
                score_type: self.score_type,
                pkce_verifier: self.pkce_verifier,
            },
        )
    }
}
