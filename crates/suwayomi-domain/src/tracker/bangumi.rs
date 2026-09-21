//! Bangumi —— 上游 `tracker/bangumi/Bangumi.kt` + `BangumiApi.kt`。
//!
//! OAuth2 授权码 + client_secret。条目用 `POST/PATCH /v0/users/-/collections/{id}`
//! 写入（新增 202、更新 204，都没有响应体）。站点没有读书记录日期，
//! `supports_reading_dates` 为假。

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::error::{DomainError, Result};

use super::service::{TrackerCtx, TrackerService, check, expires_soon, extract_token, now_secs};
use super::{BANGUMI, Track, TrackSearch};

/// 内置默认值 = 上游 Suwayomi 在 Bangumi 注册的应用；`trackers.json` 缺键时用它。
pub(super) const DEFAULT_CLIENT_ID: &str = "bgm376667faf473119bb";
pub(super) const DEFAULT_CLIENT_SECRET: &str = "d74caf0b874ddd18e6c6e7fb86d77a06";
const API_URL: &str = "https://api.bgm.tv";
const OAUTH_URL: &str = "https://bgm.tv/oauth/access_token";
const LOGIN_URL: &str = "https://bgm.tv/oauth/authorize";
pub(super) const DEFAULT_REDIRECT_URL: &str = "https://suwayomi.org/tracker-oauth";

const PLAN_TO_READ: i32 = 1;
const COMPLETED: i32 = 2;
const READING: i32 = 3;
const ON_HOLD: i32 = 4;
const DROPPED: i32 = 5;

pub struct Bangumi {
    ctx: TrackerCtx,
}

impl Bangumi {
    pub fn new(ctx: TrackerCtx) -> Self {
        Self { ctx }
    }

    async fn save_token(&self, oauth: Option<&BgmOAuth>) -> Result<()> {
        let token = match oauth {
            Some(o) => serde_json::to_string(o).unwrap_or_default(),
            None => String::new(),
        };
        self.ctx.store.set_token(BANGUMI, &token).await
    }

    async fn load_token(&self) -> Option<BgmOAuth> {
        let raw = self.ctx.store.token(BANGUMI).await.ok()?;
        serde_json::from_str(&raw).ok()
    }

    async fn bearer(&self) -> Result<String> {
        let Some(mut oauth) = self.load_token().await else {
            return Err(DomainError::tracker("Bangumi：尚未认证"));
        };
        if expires_soon(oauth.created_at, oauth.expires_in) {
            oauth = self.refresh(&oauth).await?;
        }
        Ok(oauth.access_token)
    }

    async fn refresh(&self, oauth: &BgmOAuth) -> Result<BgmOAuth> {
        let refresh_token = oauth
            .refresh_token
            .clone()
            .ok_or_else(|| DomainError::token_expired(self.name()))?;
        let app = self.require_oauth_app()?;
        let resp = self
            .ctx
            .http
            .post(OAUTH_URL)
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", app.client_id(self.name())?),
                ("client_secret", app.client_secret(self.name())?),
                ("refresh_token", refresh_token.as_str()),
                ("redirect_uri", app.redirect_uri(self.name())?),
            ])
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("Bangumi 刷新 token 失败：{e}")))?;
        let value = check(&self.ctx, BANGUMI, self.name(), resp).await?;
        let mut fresh: BgmOAuth = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Bangumi 刷新 token 响应无法解析：{e}")))?;
        // 站点不返回 user_id，沿用旧值（上游 newAuth 也是这么保的）。
        fresh.user_id = fresh.user_id.or(oauth.user_id);
        self.save_token(Some(&fresh)).await?;
        Ok(fresh)
    }

    /// 站点用户名（没设过用户名时就是用户 id 的字符串形式）。
    async fn username(&self) -> Result<String> {
        self.ctx.store.username(BANGUMI).await
    }

    fn collection_url(&self, remote_id: i64) -> String {
        format!("{API_URL}/v0/users/-/collections/{remote_id}")
    }

    fn collection_body(&self, track: &Track) -> Result<serde_json::Value> {
        Ok(json!({
            "type": to_api_status(track.status)?,
            "rate": (track.score as i32).clamp(0, 10),
            "ep_status": track.last_chapter_read as i32,
            "private": track.private,
        }))
    }

    /// 新增条目。站点返回 202，无响应体。
    async fn add_lib_manga(&self, track: &Track) -> Result<()> {
        let token = self.bearer().await?;
        let resp = self
            .ctx
            .http
            .post(self.collection_url(track.remote_id))
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .json(&self.collection_body(track)?)
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("Bangumi 新增条目失败：{e}")))?;
        check(&self.ctx, BANGUMI, self.name(), resp).await?;
        Ok(())
    }

    /// 更新条目。站点返回 204，无响应体。
    async fn update_lib_manga(&self, track: &Track) -> Result<()> {
        let token = self.bearer().await?;
        let resp = self
            .ctx
            .http
            .patch(self.collection_url(track.remote_id))
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .json(&self.collection_body(track)?)
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("Bangumi 推送失败：{e}")))?;
        check(&self.ctx, BANGUMI, self.name(), resp).await?;
        Ok(())
    }

    /// 对应上游 `statusLibManga`：没收藏时站点返回 404，这里转成 `None`。
    async fn status_lib_manga(&self, remote_id: i64) -> Result<Option<Track>> {
        let username = self.username().await?;
        let token = self.bearer().await?;
        let resp = self
            .ctx
            .http
            .get(format!("{API_URL}/v0/users/{username}/collections/{remote_id}"))
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("Bangumi 读取收藏失败：{e}")))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let value = check(&self.ctx, BANGUMI, self.name(), resp).await?;
        let collection: BgmCollectionResponse = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Bangumi 收藏响应无法解析：{e}")))?;

        let mut track = Track::create(BANGUMI);
        track.remote_id = remote_id;
        track.status = from_api_status(collection.kind)?;
        track.last_chapter_read = collection.ep_status.unwrap_or(0) as f64;
        track.score = collection.rate.unwrap_or(0) as f64;
        track.total_chapters = collection.subject.as_ref().and_then(|s| s.eps).unwrap_or(0);
        track.private = collection.private;
        Ok(Some(track))
    }

    /// 对应上游 `Bangumi.login(code)`。
    async fn login(&self, code: &str) -> Result<()> {
        let app = self.require_oauth_app()?;
        let resp = self
            .ctx
            .http
            .post(OAUTH_URL)
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", app.client_id(self.name())?),
                ("client_secret", app.client_secret(self.name())?),
                ("code", code),
                ("redirect_uri", app.redirect_uri(self.name())?),
            ])
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("Bangumi 换取 token 失败：{e}")))?;
        let value = check(&self.ctx, BANGUMI, self.name(), resp).await?;
        let oauth: BgmOAuth = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Bangumi 换取 token 响应无法解析：{e}")))?;
        self.save_token(Some(&oauth)).await?;

        let username = self.get_username(&oauth.access_token).await?;
        self.ctx.store.set_credentials(BANGUMI, &username, &oauth.access_token).await?;
        Ok(())
    }

    async fn get_username(&self, token: &str) -> Result<String> {
        let value = self.ctx.auth_get(BANGUMI, self.name(), &format!("{API_URL}/v0/me"), token).await?;
        let user: BgmUser = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Bangumi 用户信息无法解析：{e}")))?;
        Ok(user.username)
    }
}

#[async_trait]
impl TrackerService for Bangumi {
    fn id(&self) -> i32 {
        BANGUMI
    }
    fn name(&self) -> &'static str {
        "Bangumi"
    }
    fn logo(&self) -> &'static [u8] {
        super::logos::BANGUMI
    }
    fn ctx(&self) -> &TrackerCtx {
        &self.ctx
    }
    fn supports_private_tracking(&self) -> bool {
        true
    }

    fn status_list(&self) -> Vec<i32> {
        vec![READING, COMPLETED, ON_HOLD, DROPPED, PLAN_TO_READ]
    }
    fn status_name(&self, status: i32) -> Option<&'static str> {
        match status {
            READING => Some("Reading"),
            PLAN_TO_READ => Some("Plan to read"),
            COMPLETED => Some("Completed"),
            ON_HOLD => Some("On hold"),
            DROPPED => Some("Dropped"),
            _ => None,
        }
    }
    fn reading_status(&self) -> i32 {
        READING
    }
    fn completion_status(&self) -> i32 {
        COMPLETED
    }

    async fn score_list(&self) -> Result<Vec<String>> {
        Ok((0..=10).map(|i| i.to_string()).collect())
    }
    async fn index_to_score(&self, index: i32) -> Result<f64> {
        Ok(index as f64)
    }
    async fn display_score(&self, track: &Track) -> Result<String> {
        Ok((track.score as i32).to_string())
    }

    async fn auth_url(&self) -> Result<Option<String>> {
        let app = self.require_oauth_app()?;
        Ok(Some(format!(
            "{LOGIN_URL}?client_id={}&response_type=code&redirect_uri={}",
            app.client_id(self.name())?,
            app.redirect_uri(self.name())?
        )))
    }

    async fn auth_callback(&self, url: &str) -> Result<()> {
        let code = extract_token(url, "code")
            .ok_or_else(|| DomainError::tracker("Bangumi：回调地址里没有 code"))?;
        self.login(&code).await
    }

    async fn login_impl(&self, _username: &str, password: &str) -> Result<()> {
        self.login(password).await
    }

    async fn logout(&self) -> Result<()> {
        self.ctx.store.clear_credentials(BANGUMI).await
    }

    async fn bind(&self, track: &mut Track, has_read_chapters: bool) -> Result<()> {
        match self.status_lib_manga(track.remote_id).await? {
            Some(status_track) => {
                track.copy_personal_from(&status_track, false);
                track.score = status_track.score;
                track.last_chapter_read = status_track.last_chapter_read;
                track.total_chapters = status_track.total_chapters;
                track.private = status_track.private;
                if track.status != COMPLETED {
                    track.status = if has_read_chapters { READING } else { status_track.status };
                }
                self.update(track, false).await
            }
            None => {
                track.status = if has_read_chapters { READING } else { PLAN_TO_READ };
                track.score = 0.0;
                self.add_lib_manga(track).await
            }
        }
    }

    async fn update(&self, track: &mut Track, did_read_chapter: bool) -> Result<()> {
        if track.status != COMPLETED && did_read_chapter {
            if track.last_chapter_read as i32 == track.total_chapters && track.total_chapters > 0 {
                track.status = COMPLETED;
            } else {
                track.status = READING;
            }
        }
        self.update_lib_manga(track).await
    }

    async fn refresh(&self, track: &mut Track) -> Result<()> {
        let remote = self
            .status_lib_manga(track.remote_id)
            .await?
            .ok_or_else(|| DomainError::tracker("Bangumi：用户收藏里找不到该作品"))?;
        track.copy_personal_from(&remote, true);
        Ok(())
    }

    async fn search(&self, query: &str) -> Result<Vec<TrackSearch>> {
        let token = self.bearer().await?;
        let body = json!({
            "keyword": query,
            "sort": "match",
            // 1 = 书籍类型（漫画在 Bangumi 归在书籍下）
            "filter": { "type": [1] },
        });
        let resp = self
            .ctx
            .http
            .post(format!("{API_URL}/v0/search/subjects?limit=20"))
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .json(&body)
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("Bangumi 搜索失败：{e}")))?;
        let value = check(&self.ctx, BANGUMI, self.name(), resp).await?;
        let result: BgmSearchResult = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Bangumi 搜索响应无法解析：{e}")))?;
        Ok(result
            .data
            .iter()
            .filter(|s| s.platform.as_deref().map(|p| p == "漫画").unwrap_or(true))
            .map(BgmSubject::to_track_search)
            .collect())
    }
}

fn to_api_status(status: i32) -> Result<i32> {
    match status {
        PLAN_TO_READ => Ok(1),
        COMPLETED => Ok(2),
        READING => Ok(3),
        ON_HOLD => Ok(4),
        DROPPED => Ok(5),
        other => Err(DomainError::tracker(format!("Bangumi：未知状态 {other}"))),
    }
}

fn from_api_status(kind: Option<i32>) -> Result<i32> {
    match kind {
        Some(1) => Ok(PLAN_TO_READ),
        Some(2) => Ok(COMPLETED),
        Some(3) => Ok(READING),
        Some(4) => Ok(ON_HOLD),
        Some(5) => Ok(DROPPED),
        other => Err(DomainError::tracker(format!("Bangumi：未知收藏类型 {other:?}"))),
    }
}

// ---- DTO ----

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
struct BgmOAuth {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default = "now_secs")]
    created_at: i64,
    #[serde(default)]
    expires_in: i64,
    #[serde(default)]
    user_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
struct BgmUser {
    #[serde(default)]
    username: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct BgmCollectionResponse {
    #[serde(default)]
    rate: Option<i32>,
    /// 收藏类型，对应状态码 1..5。
    #[serde(default, rename = "type")]
    kind: Option<i32>,
    #[serde(default)]
    ep_status: Option<i32>,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    subject: Option<BgmSlimSubject>,
}

#[derive(Debug, Clone, Deserialize)]
struct BgmSlimSubject {
    #[serde(default)]
    eps: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct BgmSearchResult {
    #[serde(default)]
    data: Vec<BgmSubject>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct BgmSubject {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    name_cn: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    images: Option<BgmImages>,
    #[serde(default)]
    eps: i32,
    #[serde(default)]
    rating: Option<BgmRating>,
    #[serde(default)]
    platform: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct BgmImages {
    #[serde(default)]
    common: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct BgmRating {
    #[serde(default)]
    score: Option<f64>,
}

impl BgmSubject {
    fn to_track_search(&self) -> TrackSearch {
        let name_cn = self.name_cn.trim();
        let summary = self.summary.as_deref().unwrap_or_default();
        let summary = if name_cn.is_empty() {
            summary.trim().to_string()
        } else {
            let body = if summary.trim().is_empty() { String::new() } else { format!("\n{}", summary.trim()) };
            format!("作品原名：{}{}", self.name, body)
        };
        TrackSearch {
            tracker_id: BANGUMI,
            remote_id: self.id,
            title: if name_cn.is_empty() { self.name.clone() } else { self.name_cn.clone() },
            cover_url: self.images.as_ref().and_then(|i| i.common.clone()).unwrap_or_default(),
            summary,
            score: self.rating.as_ref().and_then(|r| r.score).unwrap_or(-1.0),
            tracking_url: format!("https://bangumi.tv/subject/{}", self.id),
            total_chapters: self.eps,
            start_date: self.date.clone().unwrap_or_default(),
            ..Default::default()
        }
    }
}
