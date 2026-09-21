//! `TrackerService` —— 上游 `Tracker` 抽象类（含 `DeletableTracker`）的对应物。
//!
//! 每个追踪器一个实现，负责把 `Track` 翻译成站点 API 的请求、把响应翻译回
//! `Track`/`TrackSearch`。凭据读写走 `ctx().store`，HTTP 走 `ctx().http`。
//!
//! `delete` 在 trait 上有默认空实现：只有 `supports_track_deletion()` 为真的
//! 追踪器（上游的 `DeletableTracker`）才需要覆写，调用方也只在为真时才调到。

use async_trait::async_trait;
use std::sync::{Arc, RwLock};

use crate::error::{DomainError, Result};

use super::oauth::{AppCredentials, TrackerOAuthApps};
use super::store::TrackerStore;
use super::{Track, TrackSearch};

/// 追踪器共用的依赖句柄。
#[derive(Clone)]
pub struct TrackerCtx {
    pub store: TrackerStore,
    pub http: reqwest::Client,
    /// 站点应用凭据。默认值即各模块内置的 `DEFAULT_*` 常量；生产路径由
    /// `TrackerManager::with_oauth` 换成 `trackers.json` 里读到的。用
    /// [`Self::oauth_app`] 取快照 —— 设置页改完凭据后，下一次调用就是新值。
    pub oauth: Arc<RwLock<TrackerOAuthApps>>,
}

impl TrackerCtx {
    pub fn new(store: TrackerStore, http: reqwest::Client) -> Self {
        Self { store, http, oauth: Arc::new(RwLock::new(TrackerOAuthApps::default())) }
    }

    pub fn with_oauth(
        store: TrackerStore,
        http: reqwest::Client,
        oauth: Arc<RwLock<TrackerOAuthApps>>,
    ) -> Self {
        Self { store, http, oauth }
    }

    /// 站点应用凭据的快照；非 OAuth 站点（MangaUpdates）为 `None`。
    pub fn oauth_app(&self, tracker_id: i32) -> Option<AppCredentials> {
        self.oauth.read().unwrap_or_else(|e| e.into_inner()).app(tracker_id).cloned()
    }

    /// 发一个带 `Authorization: Bearer` 的 GET。`401` 会置上「token 过期」
    /// 标记并返回 `TokenExpired`。
    pub async fn auth_get(&self, tracker_id: i32, name: &str, url: &str, token: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .get(url)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("{name} 请求失败：{e}")))?;
        check(self, tracker_id, name, resp).await
    }
}

#[async_trait]
pub trait TrackerService: Send + Sync {
    fn id(&self) -> i32;
    fn name(&self) -> &'static str;
    /// 图标 PNG 字节（编在二进制里）。
    fn logo(&self) -> &'static [u8];
    fn ctx(&self) -> &TrackerCtx;

    /// 站点应用凭据；非 OAuth 站点（MangaUpdates）为 `None`。
    fn oauth_app(&self) -> Option<AppCredentials> {
        self.ctx().oauth_app(self.id())
    }

    /// 同 [`Self::oauth_app`]，缺失时报错 —— OAuth 站点拿它取用。
    fn require_oauth_app(&self) -> Result<AppCredentials> {
        self.oauth_app()
            .ok_or_else(|| DomainError::tracker(format!("{}：没有可用的应用凭据", self.name())))
    }

    fn supports_reading_dates(&self) -> bool {
        false
    }
    fn supports_private_tracking(&self) -> bool {
        false
    }
    /// 对应上游 `DeletableTracker`。
    fn supports_track_deletion(&self) -> bool {
        false
    }

    /// 状态取值清单，顺序即界面上的顺序。
    fn status_list(&self) -> Vec<i32>;
    fn status_name(&self, status: i32) -> Option<&'static str>;
    fn reading_status(&self) -> i32;
    /// 站点没有「重读」状态时返回 `-1`。
    fn rereading_status(&self) -> i32 {
        -1
    }
    fn completion_status(&self) -> i32;

    /// 评分档位，用于 `update` 的 `scoreString`。
    async fn score_list(&self) -> Result<Vec<String>>;
    /// `score_list` 的下标 → 站点内部的评分值。
    async fn index_to_score(&self, index: i32) -> Result<f64>;
    /// 站点内部评分 → 展示串。
    async fn display_score(&self, track: &Track) -> Result<String>;

    /// 需要浏览器 OAuth 的站点返回授权 URL；用户名密码登录的返回 `None`。
    async fn auth_url(&self) -> Result<Option<String>> {
        Ok(None)
    }
    async fn auth_callback(&self, _url: &str) -> Result<()> {
        Err(DomainError::tracker(format!("{} 不支持 OAuth 回调", self.name())))
    }
    async fn login_impl(&self, username: &str, password: &str) -> Result<()>;
    /// 清登录态。上游默认只清凭据，带 token 的站点再清 token。
    async fn logout(&self) -> Result<()>;

    /// 上游 `isLoggedIn`：用户名与密码都非空。OAuth 站点的「密码」就是 access token。
    async fn is_logged_in(&self) -> Result<bool> {
        let c = self.ctx().store.get(self.id()).await?;
        Ok(!c.username.is_empty() && !c.password.is_empty())
    }
    async fn is_token_expired(&self) -> Result<bool> {
        self.ctx().store.token_expired(self.id()).await
    }

    /// 绑定：站上已有条目就拉回来覆盖本地，没有就新建。就地改写 `track`。
    async fn bind(&self, track: &mut Track, has_read_chapters: bool) -> Result<()>;
    /// 推送到站点。`did_read_chapter` 为真时按进度推进状态。
    async fn update(&self, track: &mut Track, did_read_chapter: bool) -> Result<()>;
    /// 从站点拉回最新条目，就地改写 `track`。
    async fn refresh(&self, track: &mut Track) -> Result<()>;
    async fn search(&self, query: &str) -> Result<Vec<TrackSearch>>;

    /// 重新同步站点上的用户级设置（如评分制），落库供 `score_list()` 等读。
    ///
    /// 对应 Mihon `BaseTracker.refreshUser()`：用户在站点上改过设置后，
    /// 不重新登录也能让本地的 `score_type` 跟上。默认无操作。
    async fn refresh_user(&self) -> Result<()> {
        Ok(())
    }

    /// 删除站点上的条目。只有 `supports_track_deletion()` 为真的站点会调到。
    async fn delete(&self, _track: &Track) -> Result<()> {
        Ok(())
    }
}

/// `401` 视为登录过期。其余非 2xx 直接报错，正文尽量带上。
pub(crate) async fn check(
    ctx: &TrackerCtx,
    tracker_id: i32,
    name: &str,
    resp: reqwest::Response,
) -> Result<serde_json::Value> {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        ctx.store.set_token_expired(tracker_id, true).await?;
        return Err(DomainError::token_expired(name));
    }
    if !status.is_success() {
        let head: String = body.chars().take(300).collect();
        return Err(DomainError::tracker(format!("{name} 返回 {status}：{head}")));
    }
    if body.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    Ok(serde_json::from_str(&body).unwrap_or(serde_json::Value::String(body)))
}

/// 解析回调地址里的某个键（对应上游 `String.extractToken`）。
///
/// 上游是按 `&` 切段后在每段里**搜索** `key=`，不是要求 key 正好是该段的键名 ——
/// AniList 的隐式流把 token 放在 fragment 里，回调地址形如
/// `https://host/#access_token=xxx&state=…`，第一段是 `https://host/#access_token=xxx`，
/// 只有子串匹配才取得到。
pub fn extract_token(url: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    url.split('&').find_map(|part| {
        let idx = part.find(&needle)?;
        Some(part[idx + needle.len()..].to_string())
    })
}

/// 上游 `isExpired()` 的统一形态：`created_at + expires_in - 3600 < now`。
/// 提前一小时刷新，避免请求正好撞在过期点上。
pub(crate) fn expires_soon(created_at: i64, expires_in: i64) -> bool {
    now_secs() > created_at + expires_in - 3600
}

pub(crate) fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

pub(crate) fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 对应上游 `PkceUtil.generateCodeVerifier()`：50 字节随机数的 base64url（无填充）。
/// 上游用 `SecureRandom`，这里用 `uuid` v4（同样走 getrandom）攒够字节数，
/// 免得到一个函数新引 `rand`。返回长度 67，与上游一致。
pub(crate) fn generate_code_verifier() -> String {
    use base64::Engine as _;
    let mut bytes = Vec::with_capacity(64);
    while bytes.len() < 50 {
        bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    }
    bytes.truncate(50);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes)
}

/// epoch 毫秒 → `yyyy-MM-dd`（本地时区，对应上游 `SimpleDateFormat` 的默认行为）。
pub(crate) fn format_date(ms: i64) -> Option<String> {
    use chrono::TimeZone as _;
    if ms == 0 {
        return None;
    }
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
}

/// `yyyy-MM-dd` → 本地当天零点的 epoch 毫秒。解析失败（如站点返回 `2016-00-00`）返回 0。
pub(crate) fn parse_date(s: &str) -> i64 {
    use chrono::TimeZone as _;
    let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") else {
        return 0;
    };
    let Some(naive) = date.and_hms_opt(0, 0, 0) else {
        return 0;
    };
    chrono::Local
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}
