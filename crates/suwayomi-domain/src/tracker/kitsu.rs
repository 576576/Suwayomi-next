//! Kitsu —— 上游 `tracker/kitsu/Kitsu.kt` + `KitsuApi.kt`。
//!
//! 用户名密码登录（OAuth2 password grant）拿 JWT，过期时用 refresh token 续。
//! `username` 列存站点用户名，`password` 列存 **用户 id**（列表查询要用），真正的
//! token 在 `token` 列。搜索先取 Algolia 的 key 再打 Algolia；带 `id:` 前缀的查询
//! 例外，按站点 id / slug 精确取一条。
//!
//! 评分量表有 simple / regular / advanced 三套（Mihon 口径），`login` 与
//! `refresh_user` 都会从站点读出来存进 `score_type`，之后 `score_list` /
//! `index_to_score` / `display_score` 都按它走。
//!
//! JSON:API 的 `id` 一律是字符串（`"12"`），落进数字列前要解析。

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::error::{DomainError, Result};

use super::service::{TrackerCtx, TrackerService, check, expires_soon};
use super::{KITSU, Track, TrackSearch};

/// 内置默认值 = 上游 Suwayomi 在 Kitsu 注册的应用；`trackers.json` 缺键时用它。
pub(super) const DEFAULT_CLIENT_ID: &str = "dd031b32d2f56c990b1425efe6c42ad847e7fe3ab46bf1299f05ecd856bdb7dd";
pub(super) const DEFAULT_CLIENT_SECRET: &str = "54d7307928f63414defd96399fc31ba847961ceaecef3a5fd93144e960c0e151";
const BASE_URL: &str = "https://kitsu.app/api/edge/";
const LOGIN_URL: &str = "https://kitsu.app/api/oauth/token";
const BASE_MANGA_URL: &str = "https://kitsu.app/manga/";
const ALGOLIA_KEY_URL: &str = "https://kitsu.app/api/edge/algolia-keys/media/";
const ALGOLIA_APP_ID: &str = "AWQO5J657S";
const ALGOLIA_URL: &str = "https://AWQO5J657S-dsn.algolia.net/1/indexes/production_media/query/";
const ALGOLIA_FILTER: &str = "&facetFilters=%5B%22kind%3Amanga%22%5D&attributesToRetrieve=%5B%22synopsis%22%2C%22averageRating%22%2C%22canonicalTitle%22%2C%22chapterCount%22%2C%22posterImage%22%2C%22startDate%22%2C%22subtype%22%2C%22endDate%22%2C%20%22id%22%5D";
const VND_API_JSON: &str = "application/vnd.api+json";

const READING: i32 = 1;
const COMPLETED: i32 = 2;
const ON_HOLD: i32 = 3;
const DROPPED: i32 = 4;
const PLAN_TO_READ: i32 = 5;

const RATING_SIMPLE: &str = "simple";
const RATING_REGULAR: &str = "regular";
const RATING_ADVANCED: &str = "advanced";

/// Mihon `Kitsu.SEARCH_ID_PREFIX`：带此前缀的查询不走 Algolia，直接按站点 id / slug 取一条。
const SEARCH_ID_PREFIX: &str = "id:";

/// 站点侧的三套评分制，对应 Mihon `Kitsu.RatingSystem`。
///
/// `twenty_scale` 是各档位在 Kitsu 原生 2–20 分制下的取值；本仓 `Track::score`
/// 存的是它的一半（0–10，与站点 JSON:API 的 `ratingTwenty` 差一个 ×2）。
struct RatingSystem {
    /// 落库用的名字，也是请求体里 `scoreList` 对照的键。
    name: &'static str,
    /// 下拉框选项，第 0 项固定是「未评分」。
    score_list: Vec<String>,
    /// 与 `score_list[1..]` 一一对应的 20 分制取值。
    twenty_scale: Vec<i32>,
}

impl RatingSystem {
    /// `score_list` 的下标 → `Track::score`。0 号是「未评分」。
    fn index_to_score(&self, index: i32) -> f64 {
        if index <= 0 {
            return 0.0;
        }
        // 越界按未评分处理而不是 panic：前端可能拿着过期的选项表提交。
        self.twenty_scale
            .get(index as usize - 1)
            .map_or(0.0, |v| *v as f64 / 2.0)
    }

    /// `Track::score` → 展示串。取「不超过该分数的最大档位」，与站点网页一致。
    ///
    /// 未评分（0）与低于最低档的分数都落到 `score_list[0]`。
    fn display_score(&self, score: f64) -> String {
        let native = score * 2.0;
        let idx = self.twenty_scale.iter().rposition(|v| *v as f64 <= native);
        self.score_list[idx.map_or(0, |i| i + 1)].clone()
    }
}

/// 不认识的名字按 `advanced` 处理，与 Mihon `getCurrentRatingSystem` 的报错回退一致。
fn rating_system_for(name: &str) -> RatingSystem {
    match name.to_ascii_lowercase().as_str() {
        RATING_SIMPLE => RatingSystem {
            name: RATING_SIMPLE,
            score_list: ["-", "😡", "😐", "😊", "😀"].iter().map(|s| s.to_string()).collect(),
            twenty_scale: (2..=20).step_by(6).collect(),
        },
        RATING_REGULAR => RatingSystem {
            name: RATING_REGULAR,
            score_list: (0..=10).map(|i| format!("{} ★", format_half(f64::from(i) / 2.0))).collect(),
            twenty_scale: (2..=20).step_by(2).collect(),
        },
        _ => RatingSystem {
            name: RATING_ADVANCED,
            score_list: std::iter::once("0".to_string())
                .chain((2..=20).map(|i| format_half(f64::from(i) / 2.0)))
                .collect(),
            twenty_scale: (2..=20).collect(),
        },
    }
}

pub struct Kitsu {
    ctx: TrackerCtx,
}

impl Kitsu {
    pub fn new(ctx: TrackerCtx) -> Self {
        Self { ctx }
    }

    /// 当前评分制。没存过（未登录过 / 旧库）时按 `advanced`。
    async fn current_rating_system(&self) -> RatingSystem {
        let stored = self.ctx.store.score_type(KITSU).await.unwrap_or_default();
        rating_system_for(&stored)
    }

    /// 把站点侧评分制写进 `score_type`。对应 Mihon `Kitsu.login` 里读
    /// `currentUser.ratingSystem` 那步（上游 Suwayomi 只对 AniList 做了这件事）。
    async fn save_rating_system(&self, raw: Option<&str>) -> Result<()> {
        let name = rating_system_for(raw.unwrap_or_default()).name;
        self.ctx.store.set_score_type(KITSU, name).await
    }

    async fn save_token(&self, oauth: Option<&KitsuOAuth>) -> Result<()> {
        let token = match oauth {
            Some(o) => serde_json::to_string(o).unwrap_or_default(),
            None => String::new(),
        };
        self.ctx.store.set_token(KITSU, &token).await
    }

    async fn load_token(&self) -> Option<KitsuOAuth> {
        let raw = self.ctx.store.token(KITSU).await.ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// 对应上游 `KitsuInterceptor`：过期就用 refresh token 换新的。
    async fn bearer(&self) -> Result<String> {
        let Some(mut oauth) = self.load_token().await else {
            return Err(DomainError::tracker("Kitsu：尚未认证"));
        };
        if expires_soon(oauth.created_at, oauth.expires_in) {
            oauth = self.refresh(&oauth).await?;
        }
        Ok(oauth.access_token)
    }

    async fn refresh(&self, oauth: &KitsuOAuth) -> Result<KitsuOAuth> {
        let refresh_token = oauth
            .refresh_token
            .clone()
            .ok_or_else(|| DomainError::token_expired(self.name()))?;
        let app = self.require_oauth_app()?;
        let resp = self
            .ctx
            .http
            .post(LOGIN_URL)
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.as_str()),
                ("client_id", app.client_id(self.name())?),
                ("client_secret", app.client_secret(self.name())?),
            ])
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("Kitsu 刷新 token 失败：{e}")))?;
        let value = check(&self.ctx, KITSU, self.name(), resp).await?;
        let fresh: KitsuOAuth = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Kitsu 刷新 token 响应无法解析：{e}")))?;
        self.save_token(Some(&fresh)).await?;
        Ok(fresh)
    }

    /// 用户 id 以字符串存在 `password` 列（上游 `getUserId() = getPassword()`）。
    async fn user_id(&self) -> Result<String> {
        self.ctx.store.password(KITSU).await
    }

    async fn authed(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let token = self.bearer().await?;
        let mut req = self
            .ctx
            .http
            .request(method, url)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .header(reqwest::header::ACCEPT, VND_API_JSON);
        if let Some(b) = body {
            req = req.header(reqwest::header::CONTENT_TYPE, VND_API_JSON).json(&b);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("Kitsu 请求失败：{e}")))?;
        check(&self.ctx, KITSU, self.name(), resp).await
    }

    async fn add_lib_manga(&self, track: &mut Track) -> Result<()> {
        let user_id = self.user_id().await?;
        let body = json!({
            "data": {
                "type": "libraryEntries",
                "attributes": {
                    "status": to_api_status(track.status)?,
                    "progress": track.last_chapter_read as i32,
                    "private": track.private,
                },
                "relationships": {
                    "user": { "data": { "id": user_id, "type": "users" } },
                    "media": { "data": { "id": track.remote_id, "type": "manga" } },
                }
            }
        });
        let value = self.authed(reqwest::Method::POST, &format!("{BASE_URL}library-entries"), Some(body)).await?;
        let result: KitsuAddMangaResult = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Kitsu 新增条目响应无法解析：{e}")))?;
        track.library_id = Some(json_api_id(&result.data.id)?);
        Ok(())
    }

    async fn update_lib_manga(&self, track: &Track) -> Result<()> {
        let library_id = track
            .library_id
            .ok_or_else(|| DomainError::tracker("Kitsu：条目缺少 library_id，无法更新"))?;
        let body = json!({
            "data": {
                "type": "libraryEntries",
                "id": library_id,
                "attributes": {
                    "status": to_api_status(track.status)?,
                    "progress": track.last_chapter_read as i32,
                    "ratingTwenty": to_api_score(track.score),
                    "startedAt": kitsu_date(track.started_reading_date),
                    "finishedAt": kitsu_date(track.finished_reading_date),
                    "private": track.private,
                }
            }
        });
        self.authed(
            reqwest::Method::PATCH,
            &format!("{BASE_URL}library-entries/{library_id}"),
            Some(body),
        )
        .await?;
        Ok(())
    }

    async fn remove_lib_manga(&self, library_id: i64) -> Result<()> {
        self.authed(reqwest::Method::DELETE, &format!("{BASE_URL}library-entries/{library_id}"), None)
            .await?;
        Ok(())
    }

    /// 对应上游 `findLibManga`。用户列表里没有、或站点没带回作品时返回 `None`。
    async fn find_lib_manga(&self, remote_id: i64, user_id: &str) -> Result<Option<Track>> {
        let url = format!(
            "{BASE_URL}library-entries?filter[manga_id]={remote_id}&filter[user_id]={user_id}&include=manga"
        );
        let value = self.authed(reqwest::Method::GET, &url, None).await?;
        let result: KitsuListSearchResult = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Kitsu 列表项响应无法解析：{e}")))?;
        result.first_to_track()
    }

    async fn get_lib_manga(&self, library_id: i64) -> Result<Track> {
        let url = format!("{BASE_URL}library-entries?filter[id]={library_id}&include=manga");
        let value = self.authed(reqwest::Method::GET, &url, None).await?;
        let result: KitsuListSearchResult = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Kitsu 列表项响应无法解析：{e}")))?;
        result
            .first_to_track()?
            .ok_or_else(|| DomainError::tracker("Kitsu：用户列表里找不到该作品"))
    }

    async fn search_api(&self, query: &str) -> Result<Vec<TrackSearch>> {
        let value = self.authed(reqwest::Method::GET, ALGOLIA_KEY_URL, None).await?;
        let key: KitsuSearchResult = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Kitsu 搜索 key 响应无法解析：{e}")))?;

        let body = json!({ "params": format!("query={}{ALGOLIA_FILTER}", urlencode(query)) });
        let resp = self
            .ctx
            .http
            .post(ALGOLIA_URL)
            .header("X-Algolia-Application-Id", ALGOLIA_APP_ID)
            .header("X-Algolia-API-Key", key.media.key)
            .json(&body)
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("Kitsu 搜索失败：{e}")))?;
        let value = check(&self.ctx, KITSU, self.name(), resp).await?;
        let hits: KitsuAlgoliaSearchResult = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Kitsu 搜索响应无法解析：{e}")))?;
        Ok(hits
            .hits
            .iter()
            .filter(|h| h.subtype.as_deref() != Some("novel"))
            .map(KitsuAlgoliaSearchItem::to_track_search)
            .collect())
    }

    /// 对应 Mihon `KitsuApi.getMangaDetails`：纯数字当站点 id 取，其余当 slug 取。
    /// 站点上查无此作时返回 `None`（Mihon 也是让调用方拿到 null）。
    async fn get_manga_details(&self, key: &str) -> Result<Option<TrackSearch>> {
        let is_id = is_numeric_id(key);
        let url = if is_id {
            format!("{BASE_URL}manga/{key}")
        } else {
            format!("{BASE_URL}manga?filter[slug]={}", urlencode(key))
        };
        let value = self.authed(reqwest::Method::GET, &url, None).await?;
        // 单资源是 `{"data": {...}}`，列表是 `{"data": [...]}`。
        if is_id {
            let one: KitsuMangaResource = serde_json::from_value(value)
                .map_err(|e| DomainError::tracker(format!("Kitsu 作品详情无法解析：{e}")))?;
            one.data.as_ref().map(KitsuManga::to_track_search).transpose()
        } else {
            let many: KitsuMangaListResource = serde_json::from_value(value)
                .map_err(|e| DomainError::tracker(format!("Kitsu 作品详情无法解析：{e}")))?;
            many.data.first().map(KitsuManga::to_track_search).transpose()
        }
    }

    /// 拉站点上的当前用户，把评分制同步进 `score_type`，返回站点用户 id。
    ///
    /// 对应 Mihon `KitsuApi.getCurrentUser()`；`login` 与 `refresh_user` 共用。
    async fn sync_current_user(&self) -> Result<String> {
        let user = self
            .authed(reqwest::Method::GET, &format!("{BASE_URL}users?filter[self]=true"), None)
            .await?;
        let result: KitsuCurrentUserResult = serde_json::from_value(user)
            .map_err(|e| DomainError::tracker(format!("Kitsu 用户信息无法解析：{e}")))?;
        let current = result
            .data
            .first()
            .ok_or_else(|| DomainError::tracker("Kitsu：拿不到当前用户 id"))?;
        self.save_rating_system(current.attributes.rating_system.as_deref()).await?;
        Ok(current.id.clone())
    }
}

#[async_trait]
impl TrackerService for Kitsu {
    fn id(&self) -> i32 {
        KITSU
    }
    fn name(&self) -> &'static str {
        "Kitsu"
    }
    fn logo(&self) -> &'static [u8] {
        super::logos::KITSU
    }
    fn ctx(&self) -> &TrackerCtx {
        &self.ctx
    }
    fn supports_reading_dates(&self) -> bool {
        true
    }
    fn supports_private_tracking(&self) -> bool {
        true
    }
    fn supports_track_deletion(&self) -> bool {
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
        Ok(self.current_rating_system().await.score_list)
    }

    async fn index_to_score(&self, index: i32) -> Result<f64> {
        Ok(self.current_rating_system().await.index_to_score(index))
    }

    async fn display_score(&self, track: &Track) -> Result<String> {
        Ok(self.current_rating_system().await.display_score(track.score))
    }

    async fn login_impl(&self, username: &str, password: &str) -> Result<()> {
        let app = self.require_oauth_app()?;
        let resp = self
            .ctx
            .http
            .post(LOGIN_URL)
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .form(&[
                ("username", username),
                ("password", password),
                ("grant_type", "password"),
                ("client_id", app.client_id(self.name())?),
                ("client_secret", app.client_secret(self.name())?),
            ])
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("Kitsu 登录失败：{e}")))?;
        let value = check(&self.ctx, KITSU, self.name(), resp).await?;
        let oauth: KitsuOAuth = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("Kitsu 登录响应无法解析：{e}")))?;
        self.save_token(Some(&oauth)).await?;

        let user_id = self.sync_current_user().await?;
        self.ctx.store.set_credentials(KITSU, username, &user_id).await?;
        Ok(())
    }

    async fn refresh_user(&self) -> Result<()> {
        self.sync_current_user().await?;
        Ok(())
    }

    async fn logout(&self) -> Result<()> {
        // 上游只清凭据，token 留着（下次登录会覆盖）。
        self.ctx.store.clear_credentials(KITSU).await
    }

    async fn bind(&self, track: &mut Track, has_read_chapters: bool) -> Result<()> {
        let user_id = self.user_id().await?;
        match self.find_lib_manga(track.remote_id, &user_id).await? {
            Some(remote) => {
                track.copy_personal_from(&remote, false);
                track.remote_id = remote.remote_id;
                track.library_id = remote.library_id;
                if track.status != COMPLETED {
                    track.status = if has_read_chapters { READING } else { track.status };
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
                track.finished_reading_date = super::service::now_millis();
            } else {
                track.status = READING;
                if track.last_chapter_read == 1.0 {
                    track.started_reading_date = super::service::now_millis();
                }
            }
        }
        self.update_lib_manga(track).await
    }

    async fn refresh(&self, track: &mut Track) -> Result<()> {
        let library_id = track
            .library_id
            .ok_or_else(|| DomainError::tracker("Kitsu：条目缺少 library_id，无法刷新"))?;
        let remote = self.get_lib_manga(library_id).await?;
        track.copy_personal_from(&remote, true);
        track.total_chapters = remote.total_chapters;
        Ok(())
    }

    async fn search(&self, query: &str) -> Result<Vec<TrackSearch>> {
        if let Some(rest) = query.strip_prefix(SEARCH_ID_PREFIX) {
            return Ok(self.get_manga_details(rest.trim()).await?.into_iter().collect());
        }
        self.search_api(query).await
    }

    async fn delete(&self, track: &Track) -> Result<()> {
        if let Some(id) = track.library_id {
            self.remove_lib_manga(id).await?;
        }
        Ok(())
    }
}

fn to_api_status(status: i32) -> Result<&'static str> {
    match status {
        READING => Ok("current"),
        COMPLETED => Ok("completed"),
        ON_HOLD => Ok("on_hold"),
        DROPPED => Ok("dropped"),
        PLAN_TO_READ => Ok("planned"),
        other => Err(DomainError::tracker(format!("Kitsu：未知状态 {other}"))),
    }
}

fn from_api_status(status: &str) -> Result<i32> {
    match status {
        "current" => Ok(READING),
        "completed" => Ok(COMPLETED),
        "on_hold" => Ok(ON_HOLD),
        "dropped" => Ok(DROPPED),
        "planned" => Ok(PLAN_TO_READ),
        other => Err(DomainError::tracker(format!("Kitsu：未知状态 {other}"))),
    }
}

/// 对应上游 `toApiScore()`：站点用 20 分制，0 分表示未评分（传 null）。
fn to_api_score(score: f64) -> Option<i32> {
    (score > 0.0).then_some((score * 2.0) as i32)
}

/// 对应上游 `DecimalFormat("0.#")`：整数不带小数点。
fn format_half(v: f64) -> String {
    if (v.fract()).abs() < f64::EPSILON {
        format!("{}", v as i64)
    } else {
        format!("{v:.1}")
    }
}

/// 对应上游 `KitsuDateHelper.convert`：本地时间按 `...Z` 的格式发出去。
fn kitsu_date(ms: i64) -> Option<String> {
    use chrono::TimeZone as _;
    if ms == 0 {
        return None;
    }
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
}

/// 对应上游 `KitsuDateHelper.parse`。
fn parse_kitsu_date(s: Option<&str>) -> i64 {
    use chrono::TimeZone as _;
    let Some(s) = s else {
        return 0;
    };
    let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.3fZ") else {
        return 0;
    };
    chrono::Local
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
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
struct KitsuOAuth {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    created_at: i64,
    #[serde(default)]
    expires_in: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct KitsuSearchResult {
    media: KitsuSearchKey,
}

#[derive(Debug, Clone, Deserialize)]
struct KitsuSearchKey {
    key: String,
}

#[derive(Debug, Clone, Deserialize)]
struct KitsuAddMangaResult {
    data: KitsuAddMangaItem,
}

#[derive(Debug, Clone, Deserialize)]
struct KitsuAddMangaItem {
    id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct KitsuCurrentUserResult {
    #[serde(default)]
    data: Vec<KitsuUser>,
}

#[derive(Debug, Clone, Deserialize)]
struct KitsuUser {
    id: String,
    #[serde(default)]
    attributes: KitsuUserAttributes,
}

/// JSON:API 的 `attributes`。`ratingSystem` 决定用哪套评分量表。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KitsuUserAttributes {
    #[serde(default)]
    rating_system: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct KitsuListSearchResult {
    #[serde(default)]
    data: Vec<KitsuListEntry>,
    #[serde(default)]
    included: Vec<KitsuManga>,
}

impl KitsuListSearchResult {
    fn first_to_track(&self) -> Result<Option<Track>> {
        let Some(entry) = self.data.first() else {
            return Ok(None);
        };
        let Some(manga) = self.included.first() else {
            return Ok(None);
        };
        let attrs = &entry.attributes;
        let m = &manga.attributes;

        let mut track = Track::create(KITSU);
        track.remote_id = json_api_id(&manga.id)?;
        track.library_id = Some(json_api_id(&entry.id)?);
        track.title = m.canonical_title.clone();
        track.total_chapters = m.chapter_count.unwrap_or(0);
        track.tracking_url = format!("{BASE_MANGA_URL}{}", track.remote_id);
        track.status = from_api_status(&attrs.status).unwrap_or(READING);
        track.score = attrs.rating_twenty.map(|v| v as f64 / 2.0).unwrap_or(0.0);
        track.last_chapter_read = attrs.progress as f64;
        track.started_reading_date = parse_kitsu_date(attrs.started_at.as_deref());
        track.finished_reading_date = parse_kitsu_date(attrs.finished_at.as_deref());
        track.private = attrs.private;
        Ok(Some(track))
    }
}

#[derive(Debug, Clone, Deserialize)]
struct KitsuListEntry {
    id: String,
    attributes: KitsuListAttributes,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KitsuListAttributes {
    #[serde(default)]
    status: String,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    finished_at: Option<String>,
    #[serde(default)]
    rating_twenty: Option<i32>,
    #[serde(default)]
    progress: i32,
    #[serde(default)]
    private: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct KitsuAlgoliaSearchResult {
    #[serde(default)]
    hits: Vec<KitsuAlgoliaSearchItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KitsuAlgoliaSearchItem {
    id: i64,
    canonical_title: String,
    #[serde(default)]
    chapter_count: Option<i32>,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    poster_image: Option<KitsuCover>,
    #[serde(default)]
    synopsis: Option<String>,
    #[serde(default)]
    average_rating: Option<f64>,
    #[serde(default)]
    start_date: Option<i64>,
    #[serde(default)]
    end_date: Option<i64>,
}

impl KitsuAlgoliaSearchItem {
    fn to_track_search(&self) -> TrackSearch {
        let start_date = self
            .start_date
            .and_then(|secs| super::service::format_date(secs * 1000))
            .unwrap_or_default();
        TrackSearch {
            tracker_id: KITSU,
            remote_id: self.id,
            title: self.canonical_title.clone(),
            total_chapters: self.chapter_count.unwrap_or(0),
            cover_url: self.poster_image.as_ref().and_then(|c| c.original.clone()).unwrap_or_default(),
            summary: self.synopsis.clone().unwrap_or_default(),
            score: self.average_rating.unwrap_or(-1.0),
            tracking_url: format!("{BASE_MANGA_URL}{}", self.id),
            publishing_status: if self.end_date.is_none() { "Publishing" } else { "Finished" }.to_string(),
            publishing_type: self.subtype.clone().unwrap_or_default(),
            start_date,
            ..Default::default()
        }
    }
}

/// JSON:API 的 manga 资源：`/api/edge/manga/{id}` 与 `?filter[slug]=` 都用它。
#[derive(Debug, Clone, Deserialize)]
struct KitsuManga {
    id: String,
    #[serde(default)]
    attributes: KitsuMangaAttributes,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KitsuMangaAttributes {
    #[serde(default)]
    canonical_title: String,
    #[serde(default)]
    chapter_count: Option<i32>,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    poster_image: Option<KitsuCover>,
    #[serde(default)]
    synopsis: Option<String>,
    /// 站点给的是字符串（`"82.72"`）。
    #[serde(default)]
    average_rating: Option<String>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    end_date: Option<String>,
}

impl KitsuManga {
    /// 字段映射与 Algolia 的结果保持一致，同一次搜索里两种来源的条目形状才不跳。
    fn to_track_search(&self) -> Result<TrackSearch> {
        let a = &self.attributes;
        Ok(TrackSearch {
            tracker_id: KITSU,
            remote_id: json_api_id(&self.id)?,
            title: a.canonical_title.clone(),
            total_chapters: a.chapter_count.unwrap_or(0),
            cover_url: a.poster_image.as_ref().and_then(|c| c.original.clone()).unwrap_or_default(),
            summary: a.synopsis.clone().unwrap_or_default(),
            score: a.average_rating.as_deref().and_then(|s| s.parse().ok()).unwrap_or(-1.0),
            tracking_url: format!("{BASE_MANGA_URL}{}", self.id),
            publishing_status: if a.end_date.is_none() { "Publishing" } else { "Finished" }.to_string(),
            publishing_type: a.subtype.clone().unwrap_or_default(),
            start_date: a.start_date.clone().unwrap_or_default(),
            ..Default::default()
        })
    }
}

/// 单资源响应：`{"data": {...}}`。
#[derive(Debug, Clone, Default, Deserialize)]
struct KitsuMangaResource {
    #[serde(default)]
    data: Option<KitsuManga>,
}

/// 列表响应：`{"data": [...]}`。
#[derive(Debug, Clone, Default, Deserialize)]
struct KitsuMangaListResource {
    #[serde(default)]
    data: Vec<KitsuManga>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct KitsuCover {
    #[serde(default)]
    original: Option<String>,
}

/// JSON:API 的 `id` 是字符串，落进 `Track` 的 `i64` 字段前要解析。
fn json_api_id(raw: &str) -> Result<i64> {
    raw.parse()
        .map_err(|_| DomainError::tracker(format!("Kitsu：id「{raw}」不是数字")))
}

/// 对应 Mihon `KitsuApi.getMangaDetails` 里的 `matches(Regex("\\d+"))`：
/// 纯数字当站点 id（走单资源端点），其余当 slug（走 `filter[slug]`）。
fn is_numeric_id(key: &str) -> bool {
    !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三套量表的档位与 Mihon `Kitsu.ratingSystems` 逐项一致。
    #[test]
    fn score_lists_match_mihon() {
        let simple = rating_system_for("simple");
        assert_eq!(simple.score_list, ["-", "😡", "😐", "😊", "😀"]);
        assert_eq!(simple.twenty_scale, [2, 8, 14, 20]);

        let regular = rating_system_for("regular");
        assert_eq!(regular.score_list.len(), 11);
        assert_eq!(regular.score_list[0], "0 ★");
        assert_eq!(regular.score_list[1], "0.5 ★");
        assert_eq!(regular.score_list[10], "5 ★");
        assert_eq!(regular.twenty_scale, [2, 4, 6, 8, 10, 12, 14, 16, 18, 20]);

        let advanced = rating_system_for("advanced");
        assert_eq!(advanced.score_list.len(), 20);
        assert_eq!(advanced.score_list[0], "0");
        assert_eq!(advanced.score_list[1], "1");
        assert_eq!(advanced.score_list[2], "1.5");
        assert_eq!(advanced.score_list[19], "10");
        assert_eq!(advanced.twenty_scale, (2..=20).collect::<Vec<_>>());
    }

    /// 每套量表：下标 → 分数 → 展示串必须回到原档位（0 号「未评分」也算）。
    /// 这条覆盖了 `index_to_score` 与 `display_score` 的档位对齐关系。
    #[test]
    fn index_and_display_round_trip() {
        for name in ["simple", "regular", "advanced"] {
            let rs = rating_system_for(name);
            for (i, expected) in rs.score_list.iter().enumerate() {
                let score = rs.index_to_score(i as i32);
                assert_eq!(&rs.display_score(score), expected, "{name} index={i}");
            }
        }
    }

    /// 落在两档之间的分数向下取档（站点网页也是这个口径）；低于最低档落到「未评分」。
    #[test]
    fn display_score_rounds_down() {
        let advanced = rating_system_for("advanced");
        assert_eq!(advanced.display_score(8.7), "8.5");
        assert_eq!(advanced.display_score(10.0), "10");
        assert_eq!(advanced.display_score(0.4), "0");

        let regular = rating_system_for("regular");
        assert_eq!(regular.display_score(4.7), "2 ★");

        let simple = rating_system_for("simple");
        assert_eq!(simple.display_score(0.5), "-"); // 原生 1，低于最低档 2
        assert_eq!(simple.display_score(4.0), "😐"); // 原生 8
        assert_eq!(simple.display_score(10.0), "😀"); // 原生 20
    }

    /// 档位下标越界（前端拿着过期选项表提交）按未评分处理，不能 panic。
    #[test]
    fn out_of_range_index_is_unrated() {
        let advanced = rating_system_for("advanced");
        assert_eq!(advanced.index_to_score(0), 0.0);
        assert_eq!(advanced.index_to_score(-3), 0.0);
        assert_eq!(advanced.index_to_score(19), 10.0);
        assert_eq!(advanced.index_to_score(20), 0.0);
        assert_eq!(advanced.index_to_score(999), 0.0);
    }

    /// 站点给的名字不认识时整套退回 advanced；大小写不敏感。
    #[test]
    fn unknown_rating_system_falls_back_to_advanced() {
        for name in ["", "weird", "10", "simple2"] {
            assert_eq!(rating_system_for(name).name, RATING_ADVANCED, "{name}");
        }
        assert_eq!(rating_system_for("Simple").name, RATING_SIMPLE);
        assert_eq!(rating_system_for("REGULAR").name, RATING_REGULAR);
        assert_eq!(rating_system_for("Advanced").name, RATING_ADVANCED);
    }

    /// JSON:API 的 `id` 是字符串；解析失败要报错，不能悄悄当成 0。
    #[test]
    fn json_api_id_parses_string_ids() {
        assert_eq!(json_api_id("12").unwrap(), 12);
        for bad in ["", "abc", "1.5", " "] {
            assert!(json_api_id(bad).is_err(), "{bad}");
        }
    }

    /// `id:` 后是全数字才按 id 取，其余（含空串）按 slug 取。
    #[test]
    fn numeric_key_selects_id_lookup() {
        assert!(is_numeric_id("12"));
        assert!(is_numeric_id("0"));
        for key in ["", "one-piece", "12a", "a12", "1.5", "-1", " 12"] {
            assert!(!is_numeric_id(key), "{key}");
        }
    }

    /// 站点单资源响应（`/api/edge/manga/12`）能解析成搜索结果。
    #[test]
    fn manga_resource_to_track_search() {
        let raw = json!({
            "data": {
                "id": "12",
                "type": "manga",
                "attributes": {
                    "canonicalTitle": "20th Century Boys",
                    "chapterCount": 249,
                    "subtype": "manga",
                    "synopsis": "Humanity...",
                    "averageRating": "82.72",
                    "startDate": "1999-09-27",
                    "endDate": "2006-04-24",
                    "posterImage": { "original": "https://media.kitsu.app/manga/12/poster_image/x.jpeg" }
                }
            }
        });
        let one: KitsuMangaResource = serde_json::from_value(raw).unwrap();
        let found = one.data.unwrap().to_track_search().unwrap();
        assert_eq!(found.remote_id, 12);
        assert_eq!(found.title, "20th Century Boys");
        assert_eq!(found.total_chapters, 249);
        assert_eq!(found.publishing_status, "Finished");
        assert_eq!(found.start_date, "1999-09-27");
        assert_eq!(found.score, 82.72);
        assert_eq!(found.tracking_url, "https://kitsu.app/manga/12");
    }

    /// 连载中（`endDate` 为 null）与没有封面的作品不能解析失败。
    #[test]
    fn manga_resource_without_end_date_or_cover() {
        let raw = json!({ "data": [{ "id": "38", "attributes": { "canonicalTitle": "One Piece", "endDate": null } }] });
        let many: KitsuMangaListResource = serde_json::from_value(raw).unwrap();
        let found = many.data.first().unwrap().to_track_search().unwrap();
        assert_eq!(found.remote_id, 38);
        assert_eq!(found.publishing_status, "Publishing");
        assert_eq!(found.cover_url, "");
        assert_eq!(found.score, -1.0);
        assert_eq!(found.total_chapters, 0);
    }

    /// 库条目与 `include=manga` 都是字符串 id，要能落进 `Track` 的 i64 字段。
    #[test]
    fn library_entries_parse_string_ids() {
        let raw = json!({
            "data": [{
                "id": "77",
                "attributes": { "status": "current", "progress": 3, "ratingTwenty": 16, "private": true }
            }],
            "included": [{ "id": "38", "attributes": { "canonicalTitle": "One Piece", "chapterCount": 1100 } }]
        });
        let list: KitsuListSearchResult = serde_json::from_value(raw).unwrap();
        let track = list.first_to_track().unwrap().unwrap();
        assert_eq!(track.remote_id, 38);
        assert_eq!(track.library_id, Some(77));
        assert_eq!(track.score, 8.0);
        assert_eq!(track.total_chapters, 1100);
        assert!(track.private);
    }

    /// 库里没有这条时返回 None，而不是报错。
    #[test]
    fn empty_library_result_is_none() {
        let list: KitsuListSearchResult = serde_json::from_value(json!({ "data": [], "included": [] })).unwrap();
        assert!(list.first_to_track().unwrap().is_none());
    }
}
