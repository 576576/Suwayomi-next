//! AniList —— 上游 `tracker/anilist/Anilist.kt` + `AnilistApi.kt`。
//!
//! 登录走 **隐式流**（`response_type=token`），没有 refresh token，token 有效期一年，
//! 过期只能重登。站点内部评分一律按 `POINT_100` 存取，展示时再按用户的
//! `scoreFormat` 折算 —— 所以 `score_type` 必须跟着登录一起存下来。

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::error::{DomainError, Result};

use super::service::{TrackerCtx, TrackerService, check, extract_token, now_secs};
use super::{ANILIST, Track, TrackSearch};

/// 内置默认值 = 上游 Suwayomi 在 AniList 注册的应用；`trackers.json` 缺键时用它。
pub(super) const DEFAULT_CLIENT_ID: &str = "16186";
const API_URL: &str = "https://graphql.anilist.co/";
const BASE_URL: &str = "https://anilist.co/api/v2/";
const BASE_MANGA_URL: &str = "https://anilist.co/manga/";
/// 隐式流拿到的 token 上游按一年算。
const TOKEN_TTL_SECS: i64 = 31_536_000;

/// 用户没设过偏好时的默认展示口径（上游 `TrackerPreferences.getScoreType`）。
const DEFAULT_SCORE_TYPE: &str = "POINT_10";

const READING: i32 = 1;
const COMPLETED: i32 = 2;
const ON_HOLD: i32 = 3;
const DROPPED: i32 = 4;
const PLAN_TO_READ: i32 = 5;
const REREADING: i32 = 6;

/// mediaList 里要取的字段（与上游 `findLibManga` 的 query 一致）。
const MEDIA_FIELDS: &str = r#"
    id
    title { userPreferred }
    coverImage { large }
    format
    status
    chapters
    description
    startDate { year month day }
    staff { edges { role node { name { full userPreferred native } } } }
"#;

pub struct AniList {
    ctx: TrackerCtx,
}

impl AniList {
    pub fn new(ctx: TrackerCtx) -> Self {
        Self { ctx }
    }

    async fn score_type(&self) -> String {
        let stored = self.ctx.store.score_type(ANILIST).await.unwrap_or_default();
        if stored.is_empty() { DEFAULT_SCORE_TYPE.to_string() } else { stored }
    }

    async fn bearer(&self) -> Result<String> {
        if self.ctx.store.token_expired(ANILIST).await? {
            return Err(DomainError::token_expired(self.name()));
        }
        let raw = self.ctx.store.token(ANILIST).await?;
        if raw.is_empty() {
            return Err(DomainError::tracker("AniList：尚未认证"));
        }
        let oauth: AlOAuth =
            serde_json::from_str(&raw).map_err(|e| DomainError::tracker(format!("AniList token 无法解析：{e}")))?;
        if now_secs() > oauth.expires {
            self.ctx.store.set_token_expired(ANILIST, true).await?;
            return Err(DomainError::token_expired(self.name()));
        }
        Ok(oauth.access_token)
    }

    /// 用户 id 以字符串存在 `username` 列（评分与列表查询都用它）。
    async fn user_id(&self) -> Result<i64> {
        let raw = self.ctx.store.username(ANILIST).await?;
        raw.parse::<i64>()
            .map_err(|_| DomainError::tracker(format!("AniList：用户 id 「{raw}」无法解析")))
    }

    async fn gql(&self, query: &str, variables: serde_json::Value) -> Result<serde_json::Value> {
        let token = self.bearer().await?;
        let resp = self
            .ctx
            .http
            .post(API_URL)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("AniList 请求失败：{e}")))?;
        let value = check(&self.ctx, ANILIST, self.name(), resp).await?;
        // GraphQL 的失败藏在 200 的 `errors` 里，不检查会当成「查不到」。
        if let Some(errors) = value.get("errors").and_then(|e| e.as_array())
            && !errors.is_empty()
        {
            return Err(DomainError::tracker(format!("AniList：{errors:?}")));
        }
        Ok(value)
    }

    async fn add_lib_manga(&self, track: &mut Track) -> Result<()> {
        let query = r#"
            mutation AddManga($mangaId: Int, $progress: Int, $status: MediaListStatus, $private: Boolean) {
                SaveMediaListEntry(mediaId: $mangaId, progress: $progress, status: $status, private: $private) {
                    id
                }
            }
        "#;
        let value = self
            .gql(
                query,
                json!({
                    "mangaId": track.remote_id,
                    "progress": track.last_chapter_read as i32,
                    "status": to_api_status(track.status)?,
                    "private": track.private,
                }),
            )
            .await?;
        let result: AlAddMangaResult = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("AniList 新增条目响应无法解析：{e}")))?;
        track.library_id = Some(result.data.save_media_list_entry.id);
        Ok(())
    }

    async fn update_lib_manga(&self, track: &mut Track) -> Result<()> {
        let query = r#"
            mutation UpdateManga(
                $listId: Int, $progress: Int, $status: MediaListStatus, $private: Boolean,
                $score: Int, $startedAt: FuzzyDateInput, $completedAt: FuzzyDateInput
            ) {
                SaveMediaListEntry(
                    id: $listId, progress: $progress, status: $status, private: $private,
                    scoreRaw: $score, startedAt: $startedAt, completedAt: $completedAt
                ) {
                    id
                }
            }
        "#;
        self.gql(
            query,
            json!({
                "listId": track.library_id,
                "progress": track.last_chapter_read as i32,
                "status": to_api_status(track.status)?,
                "score": track.score as i32,
                "startedAt": fuzzy_date_input(track.started_reading_date),
                "completedAt": fuzzy_date_input(track.finished_reading_date),
                "private": track.private,
            }),
        )
        .await?;
        Ok(())
    }

    async fn delete_lib_manga(&self, track: &Track) -> Result<()> {
        let query = r#"
            mutation DeleteManga($listId: Int) {
                DeleteMediaListEntry(id: $listId) { deleted }
            }
        "#;
        self.gql(query, json!({ "listId": track.library_id })).await?;
        Ok(())
    }

    async fn find_lib_manga(&self, remote_id: i64, user_id: i64) -> Result<Option<Track>> {
        let query = format!(
            r#"
            query ($id: Int!, $manga_id: Int!) {{
                Page {{
                    mediaList(userId: $id, type: MANGA, mediaId: $manga_id) {{
                        id
                        status
                        scoreRaw: score(format: POINT_100)
                        progress
                        private
                        startedAt {{ year month day }}
                        completedAt {{ year month day }}
                        media {{ {MEDIA_FIELDS} }}
                    }}
                }}
            }}
        "#
        );
        let value = self.gql(&query, json!({ "id": user_id, "manga_id": remote_id })).await?;
        let result: AlUserListResult = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("AniList 列表项响应无法解析：{e}")))?;
        Ok(result
            .data
            .page
            .media_list
            .into_iter()
            .next()
            .map(|item| item.into_track()))
    }

    async fn get_lib_manga(&self, remote_id: i64, user_id: i64) -> Result<Track> {
        self.find_lib_manga(remote_id, user_id)
            .await?
            .ok_or_else(|| DomainError::tracker("AniList：用户列表里找不到该作品"))
    }

    async fn search_api(&self, query: &str) -> Result<Vec<TrackSearch>> {
        let gql = format!(
            r#"
            query Search($query: String) {{
                Page(perPage: 50) {{
                    media(search: $query, type: MANGA, format_not_in: [NOVEL]) {{ {MEDIA_FIELDS} }}
                }}
            }}
        "#
        );
        let value = self.gql(&gql, json!({ "query": query })).await?;
        let result: AlSearchResult = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("AniList 搜索响应无法解析：{e}")))?;
        Ok(result.data.page.media.iter().map(|m| m.to_track_search()).collect())
    }

    async fn get_current_user(&self) -> Result<(i64, String)> {
        let query = r#"
            query User { Viewer { id mediaListOptions { scoreFormat } } }
        "#;
        let value = self.gql(query, json!({})).await?;
        let result: AlCurrentUserResult = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("AniList 用户信息无法解析：{e}")))?;
        Ok((result.data.viewer.id, result.data.viewer.media_list_options.score_format))
    }

    /// 对应上游 `Anilist.login(token)`。隐式流的 token 只在这时拿到一次。
    async fn login(&self, token: &str) -> Result<()> {
        let oauth = AlOAuth {
            access_token: token.to_string(),
            token_type: "Bearer".to_string(),
            expires: now_secs() + TOKEN_TTL_SECS,
        };
        self.ctx.store.set_token(ANILIST, &serde_json::to_string(&oauth).unwrap_or_default()).await?;
        let (user_id, score_format) = self.get_current_user().await?;
        self.ctx.store.set_score_type(ANILIST, &score_format).await?;
        self.ctx.store.set_credentials(ANILIST, &user_id.to_string(), token).await?;
        Ok(())
    }
}

#[async_trait]
impl TrackerService for AniList {
    fn id(&self) -> i32 {
        ANILIST
    }
    fn name(&self) -> &'static str {
        "AniList"
    }
    fn logo(&self) -> &'static [u8] {
        super::logos::ANILIST
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
        Ok(match self.score_type().await.as_str() {
            "POINT_100" => (0..=100).map(|i| i.to_string()).collect(),
            "POINT_5" => (0..=5).map(|i| format!("{i} ★")).collect(),
            "POINT_3" => vec!["-".into(), "😦".into(), "😐".into(), "😊".into()],
            "POINT_10_DECIMAL" => (0..=100).map(|i| format!("{:.1}", i as f64 / 10.0)).collect(),
            _ => (0..=10).map(|i| i.to_string()).collect(),
        })
    }

    async fn index_to_score(&self, index: i32) -> Result<f64> {
        Ok(match self.score_type().await.as_str() {
            "POINT_100" | "POINT_10_DECIMAL" => index as f64,
            "POINT_5" => if index == 0 { 0.0 } else { index as f64 * 20.0 - 10.0 },
            "POINT_3" => if index == 0 { 0.0 } else { index as f64 * 25.0 + 10.0 },
            _ => index as f64 * 10.0,
        })
    }

    async fn display_score(&self, track: &Track) -> Result<String> {
        let score = track.score;
        Ok(match self.score_type().await.as_str() {
            "POINT_5" => {
                if score == 0.0 {
                    "0 ★".to_string()
                } else {
                    format!("{} ★", ((score + 10.0) / 20.0) as i64)
                }
            }
            "POINT_3" => {
                if score == 0.0 {
                    "0".to_string()
                } else if score <= 35.0 {
                    "😦".to_string()
                } else if score <= 60.0 {
                    "😐".to_string()
                } else {
                    "😊".to_string()
                }
            }
            other => to_api_score(score, other)?.to_string(),
        })
    }

    async fn auth_url(&self) -> Result<Option<String>> {
        let client_id = self.require_oauth_app()?.client_id(self.name())?.to_string();
        Ok(Some(format!("{BASE_URL}oauth/authorize?client_id={client_id}&response_type=token")))
    }

    async fn auth_callback(&self, url: &str) -> Result<()> {
        let token = extract_token(url, "access_token")
            .ok_or_else(|| DomainError::tracker("AniList：回调地址里没有 access_token"))?;
        self.login(&token).await
    }

    async fn login_impl(&self, _username: &str, password: &str) -> Result<()> {
        self.login(password).await
    }

    async fn logout(&self) -> Result<()> {
        self.ctx.store.clear_credentials(ANILIST).await
    }

    /// Mirrors Mihon `Anilist.updateUserConfig()` —— 站点上改了评分制后，不重新登录也能跟上。
    async fn refresh_user(&self) -> Result<()> {
        let (_, score_format) = self.get_current_user().await?;
        self.ctx.store.set_score_type(ANILIST, &score_format).await
    }

    async fn bind(&self, track: &mut Track, has_read_chapters: bool) -> Result<()> {
        let user_id = self.user_id().await?;
        match self.find_lib_manga(track.remote_id, user_id).await? {
            Some(remote) => {
                // AniList 的条目隐私属于远端设置，绑定时不覆盖本地（上游 copyRemotePrivate=false）。
                track.copy_personal_from(&remote, false);
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
                self.add_lib_manga(track).await
            }
        }
    }

    async fn update(&self, track: &mut Track, did_read_chapter: bool) -> Result<()> {
        // 旧版 API v1 迁过来的记录没有 library_id，先补上，否则 SaveMediaListEntry 会新建一条。
        if track.library_id.is_none() || track.library_id == Some(0) {
            let user_id = self.user_id().await?;
            let remote = self
                .find_lib_manga(track.remote_id, user_id)
                .await?
                .ok_or_else(|| DomainError::tracker(format!("AniList：{} 不在用户列表里", track.title)))?;
            track.library_id = remote.library_id;
        }

        if track.status != COMPLETED && did_read_chapter {
            if track.last_chapter_read as i32 == track.total_chapters && track.total_chapters > 0 {
                track.status = COMPLETED;
                track.finished_reading_date = super::service::now_millis();
            } else if track.status != REREADING {
                track.status = READING;
                if track.last_chapter_read == 1.0 {
                    track.started_reading_date = super::service::now_millis();
                }
            }
        }
        self.update_lib_manga(track).await
    }

    async fn refresh(&self, track: &mut Track) -> Result<()> {
        let user_id = self.user_id().await?;
        let remote = self.get_lib_manga(track.remote_id, user_id).await?;
        track.copy_personal_from(&remote, true);
        track.title = remote.title;
        track.total_chapters = remote.total_chapters;
        Ok(())
    }

    async fn search(&self, query: &str) -> Result<Vec<TrackSearch>> {
        self.search_api(query).await
    }

    async fn delete(&self, track: &Track) -> Result<()> {
        let mut owned = track.clone();
        if owned.library_id.is_none() || owned.library_id == Some(0) {
            let user_id = self.user_id().await?;
            let Some(remote) = self.find_lib_manga(owned.remote_id, user_id).await? else {
                return Ok(());
            };
            owned.library_id = remote.library_id;
        }
        self.delete_lib_manga(&owned).await
    }
}

fn to_api_status(status: i32) -> Result<&'static str> {
    match status {
        READING => Ok("CURRENT"),
        COMPLETED => Ok("COMPLETED"),
        ON_HOLD => Ok("PAUSED"),
        DROPPED => Ok("DROPPED"),
        PLAN_TO_READ => Ok("PLANNING"),
        REREADING => Ok("REPEATING"),
        other => Err(DomainError::tracker(format!("AniList：未知状态 {other}"))),
    }
}

fn from_api_status(status: &str) -> Result<i32> {
    match status {
        "CURRENT" => Ok(READING),
        "COMPLETED" => Ok(COMPLETED),
        "PAUSED" => Ok(ON_HOLD),
        "DROPPED" => Ok(DROPPED),
        "PLANNING" => Ok(PLAN_TO_READ),
        "REPEATING" => Ok(REREADING),
        other => Err(DomainError::tracker(format!("AniList：未知状态 {other}"))),
    }
}

/// 对应上游 `Track.toApiScore(scoreType)`：内部 0–100 -> 站点展示值。
fn to_api_score(score: f64, score_type: &str) -> Result<String> {
    Ok(match score_type {
        "POINT_10" => ((score as i32) / 10).to_string(),
        "POINT_100" => (score as i32).to_string(),
        "POINT_10_DECIMAL" => format!("{}", score / 10.0),
        "POINT_5" => {
            if score == 0.0 {
                "0".to_string()
            } else if score < 30.0 {
                "1".to_string()
            } else if score < 50.0 {
                "2".to_string()
            } else if score < 70.0 {
                "3".to_string()
            } else if score < 90.0 {
                "4".to_string()
            } else {
                "5".to_string()
            }
        }
        "POINT_3" => {
            if score == 0.0 {
                "0".to_string()
            } else if score <= 35.0 {
                ":(".to_string()
            } else if score <= 60.0 {
                ":|".to_string()
            } else {
                ":)".to_string()
            }
        }
        other => return Err(DomainError::tracker(format!("AniList：未知评分口径 {other}"))),
    })
}

/// 对应上游 `createDate`：0 表示没有日期，三个字段都要传 null。
fn fuzzy_date_input(ms: i64) -> serde_json::Value {
    use chrono::{Datelike as _, TimeZone as _};
    if ms == 0 {
        return json!({ "year": null, "month": null, "day": null });
    }
    match chrono::Local.timestamp_millis_opt(ms).single() {
        Some(dt) => json!({ "year": dt.year(), "month": dt.month(), "day": dt.day() }),
        None => json!({ "year": null, "month": null, "day": null }),
    }
}

fn html_decode(s: &str) -> String {
    s.replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&nbsp;", " ")
}

// ---- DTO ----

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
struct AlOAuth {
    access_token: String,
    #[serde(default)]
    token_type: String,
    expires: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct AlFuzzyDate {
    #[serde(default)]
    year: Option<i32>,
    #[serde(default)]
    month: Option<i32>,
    #[serde(default)]
    day: Option<i32>,
}

impl AlFuzzyDate {
    fn to_epoch_millis(&self) -> i64 {
        use chrono::TimeZone as _;
        let (Some(y), Some(m), Some(d)) = (self.year, self.month, self.day) else {
            return 0;
        };
        chrono::NaiveDate::from_ymd_opt(y, m as u32, d as u32)
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .and_then(|naive| chrono::Local.from_local_datetime(&naive).single())
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct AlStaff {
    #[serde(default)]
    edges: Vec<AlEdge>,
}

#[derive(Debug, Clone, Deserialize)]
struct AlEdge {
    #[serde(default)]
    role: String,
    node: AlStaffNode,
}

#[derive(Debug, Clone, Deserialize)]
struct AlStaffNode {
    name: AlStaffName,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlStaffName {
    #[serde(default)]
    user_preferred: Option<String>,
    #[serde(default)]
    native: Option<String>,
    #[serde(default)]
    full: Option<String>,
}

impl AlStaffName {
    fn display(&self) -> Option<&str> {
        self.user_preferred.as_deref().or(self.full.as_deref()).or(self.native.as_deref())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct AlTitle {
    #[serde(rename = "userPreferred", default)]
    user_preferred: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct AlCover {
    #[serde(default)]
    large: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlSearchItem {
    id: i64,
    title: AlTitle,
    #[serde(default)]
    cover_image: Option<AlCover>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    chapters: Option<i32>,
    #[serde(default)]
    average_score: Option<i32>,
    #[serde(default)]
    staff: AlStaff,
    #[serde(default)]
    start_date: AlFuzzyDate,
}

impl AlSearchItem {
    fn to_track_search(&self) -> TrackSearch {
        let publishing_type = self.format.clone().unwrap_or_default().replace('_', "-");
        let start_date = super::service::format_date(self.start_date.to_epoch_millis()).unwrap_or_default();

        let mut authors = Vec::new();
        let mut artists = Vec::new();
        for edge in &self.staff.edges {
            let Some(name) = edge.node.name.display() else {
                continue;
            };
            if edge.role.contains("Story") {
                authors.push(name.to_string());
            }
            if edge.role.contains("Art") {
                artists.push(name.to_string());
            }
        }

        TrackSearch {
            tracker_id: ANILIST,
            remote_id: self.id,
            title: self.title.user_preferred.clone(),
            total_chapters: self.chapters.unwrap_or(0),
            cover_url: self.cover_image.as_ref().map(|c| c.large.clone()).unwrap_or_default(),
            summary: html_decode(self.description.as_deref().unwrap_or_default()),
            score: self.average_score.unwrap_or(-1) as f64,
            tracking_url: format!("{BASE_MANGA_URL}{}", self.id),
            publishing_status: self.status.clone().unwrap_or_default(),
            publishing_type,
            start_date,
            authors,
            artists,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct AlSearchResult {
    data: AlSearchPage,
}

#[derive(Debug, Clone, Deserialize)]
struct AlSearchPage {
    #[serde(rename = "Page")]
    page: AlSearchMedia,
}

#[derive(Debug, Clone, Deserialize)]
struct AlSearchMedia {
    #[serde(default)]
    media: Vec<AlSearchItem>,
}

#[derive(Debug, Clone, Deserialize)]
struct AlAddMangaResult {
    data: AlAddMangaData,
}

#[derive(Debug, Clone, Deserialize)]
struct AlAddMangaData {
    #[serde(rename = "SaveMediaListEntry")]
    save_media_list_entry: AlAddMangaEntry,
}

#[derive(Debug, Clone, Deserialize)]
struct AlAddMangaEntry {
    id: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct AlCurrentUserResult {
    data: AlViewerWrap,
}

#[derive(Debug, Clone, Deserialize)]
struct AlViewerWrap {
    #[serde(rename = "Viewer")]
    viewer: AlViewer,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlViewer {
    id: i64,
    media_list_options: AlMediaListOptions,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlMediaListOptions {
    score_format: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AlUserListResult {
    data: AlUserListPageWrap,
}

#[derive(Debug, Clone, Deserialize)]
struct AlUserListPageWrap {
    #[serde(rename = "Page")]
    page: AlUserListPage,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlUserListPage {
    #[serde(default)]
    media_list: Vec<AlUserListItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlUserListItem {
    id: i64,
    status: String,
    #[serde(default)]
    score_raw: i32,
    #[serde(default)]
    progress: i32,
    #[serde(default)]
    started_at: AlFuzzyDate,
    #[serde(default)]
    completed_at: AlFuzzyDate,
    media: AlSearchItem,
    #[serde(default)]
    private: bool,
}

impl AlUserListItem {
    fn into_track(self) -> Track {
        let mut track = Track::create(ANILIST);
        track.remote_id = self.media.id;
        track.title = self.media.title.user_preferred.clone();
        track.status = from_api_status(&self.status).unwrap_or(READING);
        track.score = self.score_raw as f64;
        track.started_reading_date = self.started_at.to_epoch_millis();
        track.finished_reading_date = self.completed_at.to_epoch_millis();
        track.last_chapter_read = self.progress as f64;
        track.library_id = Some(self.id);
        track.total_chapters = self.media.chapters.unwrap_or(0);
        track.private = self.private;
        track
    }
}
