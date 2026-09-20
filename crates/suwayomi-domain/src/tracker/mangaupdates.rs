//! MangaUpdates —— 上游 `tracker/mangaupdates/MangaUpdates.kt` + `MangaUpdatesApi.kt`。
//!
//! 用户名密码登录换 session token（不用 OAuth，没有刷新机制）。`username` 列存
//! 站点返回的 uid，`password` 列存 session token —— 上游 `restoreSession()` 读的
//! 就是 password。
//!
//! 评分是 0–10 的十分位，未评分用 `-` 表示；条目状态用 `list_id`（0..4）。

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::error::{DomainError, Result};

use super::service::{TrackerCtx, TrackerService, check};
use super::{MANGA_UPDATES, Track, TrackSearch};

const BASE_URL: &str = "https://api.mangaupdates.com";
const CONTENT_TYPE: &str = "application/vnd.api+json";

const READING_LIST: i32 = 0;
const WISH_LIST: i32 = 1;
const COMPLETE_LIST: i32 = 2;
const UNFINISHED_LIST: i32 = 3;
const ON_HOLD_LIST: i32 = 4;

pub struct MangaUpdates {
    ctx: TrackerCtx,
}

impl MangaUpdates {
    pub fn new(ctx: TrackerCtx) -> Self {
        Self { ctx }
    }

    async fn token(&self) -> Result<String> {
        let token = self.ctx.store.password(MANGA_UPDATES).await?;
        if token.is_empty() {
            return Err(DomainError::tracker("MangaUpdates：尚未认证"));
        }
        Ok(token)
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let token = self.token().await?;
        let mut req = self
            .ctx
            .http
            .request(method, format!("{BASE_URL}{path}"))
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .header(reqwest::header::USER_AGENT, super::USER_AGENT);
        if let Some(b) = body {
            req = req.header(reqwest::header::CONTENT_TYPE, CONTENT_TYPE).json(&b);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("MangaUpdates 请求失败：{e}")))?;
        check(&self.ctx, MANGA_UPDATES, self.name(), resp).await
    }

    /// 对应上游 `getSeriesListItem`：列表项 + 评分，评分拿不到按未评分处理。
    async fn get_series_list_item(&self, remote_id: i64) -> Result<(MuListItem, Option<f64>)> {
        let item: MuListItem = serde_json::from_value(
            self.request(reqwest::Method::GET, &format!("/v1/lists/series/{remote_id}"), None).await?,
        )
        .map_err(|e| DomainError::tracker(format!("MangaUpdates 列表项无法解析：{e}")))?;

        let rating = match self
            .request(reqwest::Method::GET, &format!("/v1/series/{remote_id}/rating"), None)
            .await
        {
            Ok(value) => serde_json::from_value::<MuRating>(value).ok().and_then(|r| r.rating),
            Err(_) => None,
        };
        Ok((item, rating))
    }

    /// 对应上游 `addSeriesToList`。
    async fn add_series_to_list(&self, track: &mut Track, has_read_chapters: bool) -> Result<()> {
        let status = if has_read_chapters { READING_LIST } else { WISH_LIST };
        let body = json!([{ "series": { "id": track.remote_id }, "list_id": status }]);
        self.request(reqwest::Method::POST, "/v1/lists/series", Some(body)).await?;
        track.status = status;
        track.last_chapter_read = 1.0;
        Ok(())
    }

    async fn update_series_list_item(&self, track: &Track) -> Result<()> {
        let body = json!([{
            "series": { "id": track.remote_id },
            "list_id": track.status,
            "status": { "chapter": track.last_chapter_read as i32 },
        }]);
        self.request(reqwest::Method::POST, "/v1/lists/series/update", Some(body)).await?;
        self.update_series_rating(track).await
    }

    /// 评分 0 表示「清掉评分」，负数表示本地就没评分，直接跳过。
    async fn update_series_rating(&self, track: &Track) -> Result<()> {
        if track.score < 0.0 {
            return Ok(());
        }
        let path = format!("/v1/series/{}/rating", track.remote_id);
        if track.score != 0.0 {
            let body = json!({ "rating": track.score });
            self.request(reqwest::Method::PUT, &path, Some(body)).await?;
        } else {
            self.request(reqwest::Method::DELETE, &path, None).await?;
        }
        Ok(())
    }

    /// 对应上游 `MangaUpdates.loginImpl`。
    async fn authenticate(&self, username: &str, password: &str) -> Result<()> {
        let resp = self
            .ctx
            .http
            .put(format!("{BASE_URL}/v1/account/login"))
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .header(reqwest::header::CONTENT_TYPE, CONTENT_TYPE)
            .json(&json!({ "username": username, "password": password }))
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("MangaUpdates 登录失败：{e}")))?;
        let value = check(&self.ctx, MANGA_UPDATES, self.name(), resp).await?;
        let login: MuLoginResponse = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("MangaUpdates 登录响应无法解析：{e}")))?;
        let context = login
            .context
            .ok_or_else(|| DomainError::tracker("MangaUpdates：登录失败"))?;
        self.ctx
            .store
            .set_credentials(MANGA_UPDATES, &context.uid.to_string(), &context.session_token)
            .await
    }
}

#[async_trait]
impl TrackerService for MangaUpdates {
    fn id(&self) -> i32 {
        MANGA_UPDATES
    }
    fn name(&self) -> &'static str {
        "MangaUpdates"
    }
    fn logo(&self) -> &'static [u8] {
        super::logos::MANGA_UPDATES
    }
    fn ctx(&self) -> &TrackerCtx {
        &self.ctx
    }
    fn supports_track_deletion(&self) -> bool {
        true
    }

    fn status_list(&self) -> Vec<i32> {
        vec![READING_LIST, COMPLETE_LIST, ON_HOLD_LIST, UNFINISHED_LIST, WISH_LIST]
    }
    fn status_name(&self, status: i32) -> Option<&'static str> {
        match status {
            READING_LIST => Some("Reading List"),
            WISH_LIST => Some("Wish List"),
            COMPLETE_LIST => Some("Complete List"),
            ON_HOLD_LIST => Some("On Hold List"),
            UNFINISHED_LIST => Some("Unfinished List"),
            _ => None,
        }
    }
    fn reading_status(&self) -> i32 {
        READING_LIST
    }
    fn completion_status(&self) -> i32 {
        COMPLETE_LIST
    }

    async fn score_list(&self) -> Result<Vec<String>> {
        Ok(score_list())
    }

    async fn index_to_score(&self, index: i32) -> Result<f64> {
        if index <= 0 {
            return Ok(0.0);
        }
        let list = score_list();
        Ok(list.get(index as usize).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0))
    }

    async fn display_score(&self, track: &Track) -> Result<String> {
        Ok(track.score.to_string())
    }

    async fn login_impl(&self, username: &str, password: &str) -> Result<()> {
        self.authenticate(username, password).await
    }

    async fn logout(&self) -> Result<()> {
        self.ctx.store.clear_credentials(MANGA_UPDATES).await
    }

    async fn bind(&self, track: &mut Track, has_read_chapters: bool) -> Result<()> {
        match self.get_series_list_item(track.remote_id).await {
            Ok((item, rating)) => {
                track.status = item.list_id.unwrap_or(READING_LIST);
                track.last_chapter_read = item.status.and_then(|s| s.chapter).map(|c| c as f64).unwrap_or(0.0);
                track.score = rating.unwrap_or(0.0);
                Ok(())
            }
            Err(_) => {
                // 站点上没有这条 series，或用户列表里还没有：新建。
                track.score = 0.0;
                self.add_series_to_list(track, has_read_chapters).await
            }
        }
    }

    async fn update(&self, track: &mut Track, did_read_chapter: bool) -> Result<()> {
        if track.status != COMPLETE_LIST && did_read_chapter {
            track.status = READING_LIST;
        }
        self.update_series_list_item(track).await
    }

    async fn refresh(&self, track: &mut Track) -> Result<()> {
        let (item, rating) = self.get_series_list_item(track.remote_id).await?;
        track.status = item.list_id.unwrap_or(READING_LIST);
        track.last_chapter_read = item.status.and_then(|s| s.chapter).map(|c| c as f64).unwrap_or(0.0);
        track.score = rating.unwrap_or(0.0);
        Ok(())
    }

    async fn search(&self, query: &str) -> Result<Vec<TrackSearch>> {
        let body = json!({ "search": query, "filter_types": ["drama cd", "novel"] });
        let value = self.request(reqwest::Method::POST, "/v1/series/search", Some(body)).await?;
        let result: MuSearchResult = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("MangaUpdates 搜索响应无法解析：{e}")))?;
        Ok(result.results.into_iter().map(|r| r.record.to_track_search()).collect())
    }

    async fn delete(&self, track: &Track) -> Result<()> {
        let body = json!([track.remote_id]);
        self.request(reqwest::Method::POST, "/v1/lists/series/delete", Some(body)).await?;
        Ok(())
    }
}

/// 对应上游 `MangaUpdates.SCORE_LIST`：`-` 是「未评分」，其余是 0.0 起的十分位。
fn score_list() -> Vec<String> {
    let mut out = vec!["-".to_string()];
    for decimal in 0..=10 {
        if decimal == 0 {
            continue;
        }
        if decimal == 10 {
            out.push("10.0".to_string());
            continue;
        }
        for fraction in 0..=9 {
            out.push(format!("{decimal}.{fraction}"));
        }
    }
    out
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

#[derive(Debug, Clone, Default, Deserialize)]
struct MuLoginResponse {
    #[serde(default)]
    context: Option<MuContext>,
}

#[derive(Debug, Clone, Deserialize)]
struct MuContext {
    #[serde(default)]
    session_token: String,
    #[serde(default)]
    uid: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MuListItem {
    #[serde(default)]
    list_id: Option<i32>,
    #[serde(default)]
    status: Option<MuStatus>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MuStatus {
    #[serde(default)]
    chapter: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MuRating {
    #[serde(default)]
    rating: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MuSearchResult {
    #[serde(default)]
    results: Vec<MuSearchItem>,
}

#[derive(Debug, Clone, Deserialize)]
struct MuSearchItem {
    record: MuRecord,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MuRecord {
    #[serde(default)]
    series_id: Option<i64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    image: Option<MuImage>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    year: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MuImage {
    #[serde(default)]
    url: Option<MuImageUrl>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MuImageUrl {
    #[serde(default)]
    original: Option<String>,
}

impl MuRecord {
    /// 上游的 `total_chapters` 固定写 0、`publishing_status` 固定写空串（站点搜索
    /// 结果里没有这两项），照抄。
    fn to_track_search(&self) -> TrackSearch {
        TrackSearch {
            tracker_id: MANGA_UPDATES,
            remote_id: self.series_id.unwrap_or(0),
            title: html_decode(self.title.as_deref().unwrap_or_default()),
            cover_url: self
                .image
                .as_ref()
                .and_then(|i| i.url.as_ref())
                .and_then(|u| u.original.clone())
                .unwrap_or_default(),
            summary: html_decode(self.description.as_deref().unwrap_or_default()),
            tracking_url: self.url.clone().unwrap_or_default(),
            publishing_type: self.kind.clone().unwrap_or_else(|| "null".to_string()),
            start_date: self.year.clone().unwrap_or_else(|| "null".to_string()),
            ..Default::default()
        }
    }
}
