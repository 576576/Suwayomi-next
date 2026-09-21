//! Shikimori —— 上游 `tracker/shikimori/Shikimori.kt` + `ShikimoriApi.kt`。
//!
//! OAuth2 授权码 + client_secret（不需要 PKCE）。条目写入是 upsert：新增和更新
//! 都打 `POST /v2/user_rates`，`library_id` 是站点返回的 user_rate id。
//! 站点没有「重读」之外的时间线支持，`supports_reading_dates` 为假。

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::error::{DomainError, Result};

use super::service::{TrackerCtx, TrackerService, check, expires_soon, extract_token, now_secs};
use super::{SHIKIMORI, Track, TrackSearch};

const BASE_URL: &str = "https://shikimori.io";
const API_URL: &str = "https://shikimori.io/api";
const OAUTH_URL: &str = "https://shikimori.io/oauth/token";
const LOGIN_URL: &str = "https://shikimori.io/oauth/authorize";
/// 内置默认值 = 上游 Suwayomi 在 Shikimori 注册的应用；`trackers.json` 缺键时用它。
pub(super) const DEFAULT_REDIRECT_URL: &str = "https://suwayomi.org/tracker-oauth";
pub(super) const DEFAULT_CLIENT_ID: &str = "qTrMBF5HtM_33Pv2Vm2fFmEaBUI_c3LvohyJ0beQ9pA";
pub(super) const DEFAULT_CLIENT_SECRET: &str = "MN_XHQK_aeSqduW_rB64cARi2fFoLGl-AgZ0iMD9zq0";

const READING: i32 = 1;
const COMPLETED: i32 = 2;
const ON_HOLD: i32 = 3;
const DROPPED: i32 = 4;
const PLAN_TO_READ: i32 = 5;
const REREADING: i32 = 6;

pub struct Shikimori {
    ctx: TrackerCtx,
}

impl Shikimori {
    pub fn new(ctx: TrackerCtx) -> Self {
        Self { ctx }
    }

    async fn save_token(&self, oauth: Option<&SmOAuth>) -> Result<()> {
        let token = match oauth {
            Some(o) => serde_json::to_string(o).unwrap_or_default(),
            None => String::new(),
        };
        self.ctx.store.set_token(SHIKIMORI, &token).await
    }

    async fn load_token(&self) -> Option<SmOAuth> {
        let raw = self.ctx.store.token(SHIKIMORI).await.ok()?;
        serde_json::from_str(&raw).ok()
    }

    async fn bearer(&self) -> Result<String> {
        let Some(mut oauth) = self.load_token().await else {
            return Err(DomainError::tracker("Shikimori：尚未认证"));
        };
        if expires_soon(oauth.created_at, oauth.expires_in) {
            oauth = self.refresh(&oauth).await?;
        }
        Ok(oauth.access_token)
    }

    async fn refresh(&self, oauth: &SmOAuth) -> Result<SmOAuth> {
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
            ])
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("Shikimori 刷新 token 失败：{e}")))?;
        let value = check(&self.ctx, SHIKIMORI, self.name(), resp).await?;
        let fresh: SmOAuth = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Shikimori 刷新 token 响应无法解析：{e}")))?;
        self.save_token(Some(&fresh)).await?;
        Ok(fresh)
    }

    /// 站点用户 id 以字符串存在 `username` 列。
    async fn user_id(&self) -> Result<String> {
        self.ctx.store.username(SHIKIMORI).await
    }

    async fn get(&self, url: &str) -> Result<serde_json::Value> {
        let token = self.bearer().await?;
        self.ctx.auth_get(SHIKIMORI, self.name(), url, &token).await
    }

    /// 对应上游 `Shikimori.login(code)`：用授权码换 token，再拿用户 id 存下来。
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
            .map_err(|e| DomainError::tracker(format!("Shikimori 换取 token 失败：{e}")))?;
        let value = check(&self.ctx, SHIKIMORI, self.name(), resp).await?;
        let oauth: SmOAuth = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Shikimori 换取 token 响应无法解析：{e}")))?;
        self.save_token(Some(&oauth)).await?;

        let user: SmUser = serde_json::from_value(self.get(&format!("{API_URL}/users/whoami")).await?)
            .map_err(|e| DomainError::tracker(format!("Shikimori 用户信息无法解析：{e}")))?;
        self.ctx.store.set_credentials(SHIKIMORI, &user.id.to_string(), &oauth.access_token).await?;
        Ok(())
    }

    /// 对应上游 `addLibManga` / `updateLibManga`：同一个 upsert 端点。
    async fn put_user_rate(&self, track: &mut Track) -> Result<()> {
        let token = self.bearer().await?;
        let user_id = self.user_id().await?;
        let body = json!({
            "user_rate": {
                "user_id": user_id,
                "target_id": track.remote_id,
                "target_type": "Manga",
                "chapters": track.last_chapter_read as i32,
                "score": track.score as i32,
                "status": to_shikimori_status(track.status)?,
            }
        });
        let resp = self
            .ctx
            .http
            .post(format!("{API_URL}/v2/user_rates"))
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .json(&body)
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("Shikimori 推送失败：{e}")))?;
        let value = check(&self.ctx, SHIKIMORI, self.name(), resp).await?;
        let added: SmAddMangaResponse = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Shikimori 推送响应无法解析：{e}")))?;
        // 站点返回的 user_rate id 就是删除/查询条目要用的 library_id。
        track.library_id = Some(added.id);
        Ok(())
    }

    async fn find_lib_manga(&self, track: &Track) -> Result<Option<Track>> {
        let manga: SmManga = serde_json::from_value(
            self.get(&format!("{API_URL}/mangas/{}", track.remote_id)).await?,
        )
        .map_err(|e| DomainError::tracker(format!("Shikimori 作品详情无法解析：{e}")))?;

        let user_id = self.user_id().await?;
        let url = format!(
            "{API_URL}/v2/user_rates?user_id={user_id}&target_id={}&target_type=Manga",
            track.remote_id
        );
        let value = self.get(&url).await?;
        let entries: Vec<SmUserListEntry> = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Shikimori 列表项响应无法解析：{e}")))?;
        if entries.len() > 1 {
            return Err(DomainError::tracker("Shikimori：该作品的条目多于一条"));
        }
        Ok(entries.first().map(|e| e.to_track(track.remote_id, &manga)))
    }
}

#[async_trait]
impl TrackerService for Shikimori {
    fn id(&self) -> i32 {
        SHIKIMORI
    }
    fn name(&self) -> &'static str {
        "Shikimori"
    }
    fn logo(&self) -> &'static [u8] {
        super::logos::SHIKIMORI
    }
    fn ctx(&self) -> &TrackerCtx {
        &self.ctx
    }
    fn supports_track_deletion(&self) -> bool {
        true
    }

    fn status_list(&self) -> Vec<i32> {
        vec![READING, COMPLETED, ON_HOLD, DROPPED, PLAN_TO_READ, REREADING]
    }
    fn status_name(&self, status: i32) -> Option<&'static str> {
        match status {
            READING => Some("Reading"),
            PLAN_TO_READ => Some("Plan to read"),
            COMPLETED => Some("Completed"),
            ON_HOLD => Some("On hold"),
            DROPPED => Some("Dropped"),
            REREADING => Some("Rereading"),
            _ => None,
        }
    }
    fn reading_status(&self) -> i32 {
        READING
    }
    fn rereading_status(&self) -> i32 {
        REREADING
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
            "{LOGIN_URL}?client_id={}&redirect_uri={}&response_type=code",
            app.client_id(self.name())?,
            app.redirect_uri(self.name())?
        )))
    }

    async fn auth_callback(&self, url: &str) -> Result<()> {
        let code = extract_token(url, "code")
            .ok_or_else(|| DomainError::tracker("Shikimori：回调地址里没有 code"))?;
        self.login(&code).await
    }

    async fn login_impl(&self, _username: &str, password: &str) -> Result<()> {
        self.login(password).await
    }

    async fn logout(&self) -> Result<()> {
        self.ctx.store.clear_credentials(SHIKIMORI).await
    }

    async fn bind(&self, track: &mut Track, has_read_chapters: bool) -> Result<()> {
        match self.find_lib_manga(track).await? {
            Some(remote) => {
                track.copy_personal_from(&remote, true);
                track.library_id = remote.library_id;
                if track.status != COMPLETED {
                    let is_rereading = track.status == REREADING;
                    track.status = if !is_rereading && has_read_chapters { READING } else { track.status };
                }
                self.update(track, false).await
            }
            None => {
                track.status = if has_read_chapters { READING } else { PLAN_TO_READ };
                track.score = 0.0;
                self.put_user_rate(track).await
            }
        }
    }

    async fn update(&self, track: &mut Track, did_read_chapter: bool) -> Result<()> {
        // Shikimori 不支持读书记录日期，所以这里只推状态，不写 start/finish。
        if track.status != COMPLETED && did_read_chapter {
            if track.last_chapter_read as i32 == track.total_chapters && track.total_chapters > 0 {
                track.status = COMPLETED;
            } else if track.status != REREADING {
                track.status = READING;
            }
        }
        self.put_user_rate(track).await
    }

    async fn refresh(&self, track: &mut Track) -> Result<()> {
        let remote = self
            .find_lib_manga(track)
            .await?
            .ok_or_else(|| DomainError::tracker("Shikimori：用户列表里找不到该作品"))?;
        track.library_id = remote.library_id;
        track.copy_personal_from(&remote, true);
        track.total_chapters = remote.total_chapters;
        Ok(())
    }

    async fn search(&self, query: &str) -> Result<Vec<TrackSearch>> {
        let url = format!("{API_URL}/mangas?order=popularity&search={}&limit=20", urlencode(query));
        let value = self.get(&url).await?;
        let mangas: Vec<SmManga> = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Shikimori 搜索响应无法解析：{e}")))?;
        Ok(mangas.iter().map(SmManga::to_track_search).collect())
    }

    async fn delete(&self, track: &Track) -> Result<()> {
        let Some(library_id) = track.library_id else {
            return Ok(());
        };
        let token = self.bearer().await?;
        let resp = self
            .ctx
            .http
            .delete(format!("{API_URL}/v2/user_rates/{library_id}"))
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("Shikimori 删除失败：{e}")))?;
        check(&self.ctx, SHIKIMORI, self.name(), resp).await?;
        Ok(())
    }
}

pub(crate) fn to_shikimori_status(status: i32) -> Result<&'static str> {
    match status {
        READING => Ok("watching"),
        COMPLETED => Ok("completed"),
        ON_HOLD => Ok("on_hold"),
        DROPPED => Ok("dropped"),
        PLAN_TO_READ => Ok("planned"),
        REREADING => Ok("rewatching"),
        other => Err(DomainError::tracker(format!("Shikimori：未知状态 {other}"))),
    }
}

fn from_shikimori_status(status: &str) -> Result<i32> {
    match status {
        "watching" => Ok(READING),
        "completed" => Ok(COMPLETED),
        "on_hold" => Ok(ON_HOLD),
        "dropped" => Ok(DROPPED),
        "planned" => Ok(PLAN_TO_READ),
        "rewatching" => Ok(REREADING),
        other => Err(DomainError::tracker(format!("Shikimori：未知状态 {other}"))),
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---- DTO ----

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
struct SmOAuth {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    /// 站点没返回时按「刚刚签发」处理，否则会被判成已过期而反复刷新。
    #[serde(default = "now_secs")]
    created_at: i64,
    #[serde(default)]
    expires_in: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct SmUser {
    id: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct SmAddMangaResponse {
    id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SmManga {
    id: i64,
    name: String,
    #[serde(default)]
    chapters: i32,
    image: SmCover,
    #[serde(default)]
    score: f64,
    url: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    aired_on: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SmCover {
    #[serde(default)]
    preview: String,
}

impl SmManga {
    fn to_track_search(&self) -> TrackSearch {
        TrackSearch {
            tracker_id: SHIKIMORI,
            remote_id: self.id,
            title: self.name.clone(),
            total_chapters: self.chapters,
            cover_url: format!("{BASE_URL}{}", self.image.preview),
            score: self.score,
            tracking_url: format!("{BASE_URL}{}", self.url),
            publishing_status: self.status.clone(),
            publishing_type: self.kind.clone(),
            start_date: self.aired_on.clone().unwrap_or_default(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct SmUserListEntry {
    id: i64,
    #[serde(default)]
    chapters: f64,
    #[serde(default)]
    score: i32,
    #[serde(default)]
    status: String,
}

impl SmUserListEntry {
    /// `remote_id` 取调用方原本的值：上游这里写的是 entry.id（user_rate 的 id，
    /// 不是作品 id），但 `copyPersonalFrom` 不搬 `remote_id`，所以那个值一直没被
    /// 用到；照抄会把错误的 id 写进 `track_record`。
    fn to_track(&self, remote_id: i64, manga: &SmManga) -> Track {
        let mut track = Track::create(SHIKIMORI);
        track.title = manga.name.clone();
        track.remote_id = remote_id;
        track.total_chapters = manga.chapters;
        track.library_id = Some(self.id);
        track.last_chapter_read = self.chapters;
        track.score = self.score as f64;
        track.status = from_shikimori_status(&self.status).unwrap_or(READING);
        track.tracking_url = format!("{BASE_URL}{}", manga.url);
        track
    }
}
