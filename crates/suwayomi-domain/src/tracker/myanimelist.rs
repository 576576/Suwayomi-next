//! MyAnimeList —— 上游 `tracker/myanimelist/MyAnimeList.kt` + `MyAnimeListApi.kt`。
//!
//! 登录是 OAuth2 授权码 + PKCE，且用的是 **plain 方式**：`authUrl` 里
//! `code_challenge` 就是 code_verifier 本身（上游把 `PkceUtil.generateCodeVerifier()`
//! 的返回值直接当 challenge 传）。所以 verifier 要存起来，换 token 时原样回传。
//!
//! access token 一小时过期，但上游的拦截器会拿 refresh token 自动续期，用户只在
//! 拿不到新 token 时才会看到「登录已过期」。

use async_trait::async_trait;
use serde::Deserialize;

use crate::error::{DomainError, Result};

use super::service::{
    TrackerCtx, TrackerService, check, extract_token, format_date, generate_code_verifier, now_secs, parse_date,
};
use super::{MYANIMELIST, Track, TrackSearch};

const CLIENT_ID: &str = "3fda277931a4f9bc01fa4a715ce8b91d";
const BASE_OAUTH_URL: &str = "https://myanimelist.net/v1/oauth2";
const BASE_API_URL: &str = "https://api.myanimelist.net/v2";
/// 上游的列表分页步长（`LIST_PAGINATION_AMOUNT`）。
const LIST_PAGE: usize = 250;
/// MAL 的搜索接口超过 64 字符直接 400，所以查询串要截断。
const MAX_QUERY: usize = 64;

const READING: i32 = 1;
const COMPLETED: i32 = 2;
const ON_HOLD: i32 = 3;
const DROPPED: i32 = 4;
const PLAN_TO_READ: i32 = 6;
const REREADING: i32 = 7;

pub struct MyAnimeList {
    ctx: TrackerCtx,
}

impl MyAnimeList {
    pub fn new(ctx: TrackerCtx) -> Self {
        Self { ctx }
    }

    /// 保存 OAuth 串（上游 `saveOAuth`）。空串等同 `null`。
    async fn save_oauth(&self, oauth: Option<&MalOAuth>) -> Result<()> {
        let token = match oauth {
            Some(o) => serde_json::to_string(o).unwrap_or_default(),
            None => String::new(),
        };
        self.ctx.store.set_token(MYANIMELIST, &token).await
    }

    async fn load_oauth(&self) -> Option<MalOAuth> {
        let raw = self.ctx.store.token(MYANIMELIST).await.ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// 对应上游 `MyAnimeListInterceptor.intercept`：过期就刷新，拿不到就报过期。
    async fn bearer(&self) -> Result<String> {
        if self.ctx.store.token_expired(MYANIMELIST).await? {
            return Err(DomainError::token_expired(self.name()));
        }
        let Some(mut oauth) = self.load_oauth().await else {
            return Err(DomainError::tracker("MyAnimeList：尚未认证"));
        };
        if oauth.is_expired() {
            oauth = self.refresh_token(&oauth).await?;
        }
        Ok(oauth.access_token)
    }

    async fn refresh_token(&self, oauth: &MalOAuth) -> Result<MalOAuth> {
        let resp = self
            .ctx
            .http
            .post(format!("{BASE_OAUTH_URL}/token"))
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", oauth.access_token))
            .form(&[
                ("client_id", CLIENT_ID),
                ("refresh_token", oauth.refresh_token.as_str()),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("MyAnimeList 刷新 token 失败：{e}")))?;
        let value = check(&self.ctx, MYANIMELIST, self.name(), resp).await?;
        let fresh: MalOAuth = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("MyAnimeList 刷新 token 响应无法解析：{e}")))?;
        self.save_oauth(Some(&fresh)).await?;
        Ok(fresh)
    }

    async fn get_access_token(&self, code: &str) -> Result<MalOAuth> {
        let verifier = self.ctx.store.pkce_verifier(MYANIMELIST).await?;
        let resp = self
            .ctx
            .http
            .post(format!("{BASE_OAUTH_URL}/token"))
            .form(&[
                ("client_id", CLIENT_ID),
                ("code", code),
                ("code_verifier", verifier.as_str()),
                ("grant_type", "authorization_code"),
            ])
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("MyAnimeList 换取 token 失败：{e}")))?;
        let value = check(&self.ctx, MYANIMELIST, self.name(), resp).await?;
        serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("MyAnimeList 换取 token 响应无法解析：{e}")))
    }

    async fn get_current_user(&self) -> Result<String> {
        let token = self.bearer().await?;
        let value = self.ctx.auth_get(MYANIMELIST, self.name(), &format!("{BASE_API_URL}/users/@me"), &token).await?;
        let user: MalUser = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("MyAnimeList 用户信息无法解析：{e}")))?;
        Ok(user.name)
    }

    /// 对应上游 `MyAnimeList.login(authCode)`。登录成功后立刻清掉 PKCE verifier。
    async fn login(&self, auth_code: &str) -> Result<()> {
        let oauth = self.get_access_token(auth_code).await?;
        self.save_oauth(Some(&oauth)).await?;
        let username = self.get_current_user().await?;
        self.ctx.store.set_credentials(MYANIMELIST, &username, &oauth.access_token).await?;
        self.ctx.store.clear_pkce_verifier(MYANIMELIST).await?;
        Ok(())
    }

    async fn manga_url(&self, remote_id: i64) -> String {
        format!("{BASE_API_URL}/manga/{remote_id}/my_list_status")
    }

    /// 对应上游 `getMangaDetails`。
    async fn get_manga_details(&self, id: i64) -> Result<TrackSearch> {
        let token = self.bearer().await?;
        let url = format!(
            "{BASE_API_URL}/manga/{id}?fields=id,title,synopsis,num_chapters,mean,main_picture,status,media_type,start_date"
        );
        let value = self.ctx.auth_get(MYANIMELIST, self.name(), &url, &token).await?;
        let manga: MalManga = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("MyAnimeList 作品详情无法解析：{e}")))?;
        let remote_id = manga.id;
        Ok(TrackSearch {
            tracker_id: MYANIMELIST,
            remote_id,
            title: manga.title,
            summary: manga.synopsis,
            total_chapters: manga.num_chapters,
            score: manga.mean,
            cover_url: manga.main_picture.map(|c| c.large).unwrap_or_default(),
            tracking_url: format!("https://myanimelist.net/manga/{remote_id}"),
            publishing_status: manga.status.replace('_', " "),
            publishing_type: manga.media_type.replace('_', " "),
            start_date: manga.start_date.unwrap_or_default(),
            ..Default::default()
        })
    }

    /// 对应上游 `updateItem`：把本地 track 推上去，并按响应回写站点侧的状态。
    async fn update_item(&self, track: &mut Track) -> Result<()> {
        let token = self.bearer().await?;
        let mut form: Vec<(&str, String)> = vec![
            ("status", to_my_anime_list_status(track.status).unwrap_or("reading").to_string()),
            ("is_rereading", (track.status == REREADING).to_string()),
            ("score", track.score.to_string()),
            ("num_chapters_read", (track.last_chapter_read as i32).to_string()),
        ];
        if let Some(d) = format_date(track.started_reading_date) {
            form.push(("start_date", d));
        }
        if let Some(d) = format_date(track.finished_reading_date) {
            form.push(("finish_date", d));
        }
        let resp = self
            .ctx
            .http
            .put(self.manga_url(track.remote_id).await)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .form(&form)
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("MyAnimeList 推送失败：{e}")))?;
        let value = check(&self.ctx, MYANIMELIST, self.name(), resp).await?;
        let status: MalListStatus = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("MyAnimeList 推送响应无法解析：{e}")))?;
        apply_list_status(track, &status);
        Ok(())
    }

    async fn delete_item(&self, track: &Track) -> Result<()> {
        let token = self.bearer().await?;
        let resp = self
            .ctx
            .http
            .delete(self.manga_url(track.remote_id).await)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .send()
            .await
            .map_err(|e| DomainError::tracker(format!("MyAnimeList 删除失败：{e}")))?;
        check(&self.ctx, MYANIMELIST, self.name(), resp).await?;
        Ok(())
    }

    /// 对应上游 `findListItem`：不在用户列表上时返回 `None`。
    async fn find_list_item(&self, track: &mut Track) -> Result<Option<Track>> {
        let token = self.bearer().await?;
        let url = format!("{BASE_API_URL}/manga/{}?fields=num_chapters,my_list_status{{start_date,finish_date}}", track.remote_id);
        let value = self.ctx.auth_get(MYANIMELIST, self.name(), &url, &token).await?;
        let item: MalList = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("MyAnimeList 列表项无法解析：{e}")))?;
        if let Some(n) = item.num_chapters {
            track.total_chapters = n;
        }
        let Some(status) = item.my_list_status else {
            return Ok(None);
        };
        apply_list_status(track, &status);
        Ok(Some(track.clone()))
    }

    async fn search_api(&self, query: &str) -> Result<Vec<TrackSearch>> {
        let token = self.bearer().await?;
        let truncated: String = query.chars().take(MAX_QUERY).collect();
        let url = format!("{BASE_API_URL}/manga?q={}&nsfw=true", urlencode(&truncated));
        let value = self.ctx.auth_get(MYANIMELIST, self.name(), &url, &token).await?;
        let result: MalSearchResult = serde_json::from_value(value)
            .map_err(|e| DomainError::tracker(format!("MyAnimeList 搜索响应无法解析：{e}")))?;
        let mut out = Vec::new();
        for node in result.data {
            let details = self.get_manga_details(node.node.id).await?;
            // MAL 的搜索结果里混着小说；上游按 publishing_type 过滤。
            if !details.publishing_type.contains("novel") {
                out.push(details);
            }
        }
        Ok(out)
    }

    /// 对应上游 `findListItems("my:<title>")`：翻遍用户列表按标题子串匹配。
    async fn find_list_items(&self, query: &str) -> Result<Vec<TrackSearch>> {
        let mut out = Vec::new();
        let mut offset = 0usize;
        loop {
            let token = self.bearer().await?;
            let mut url = format!(
                "{BASE_API_URL}/users/@me/mangalist?fields=list_status{{start_date,finish_date}}&limit={LIST_PAGE}"
            );
            if offset > 0 {
                url.push_str(&format!("&offset={offset}"));
            }
            let value = self.ctx.auth_get(MYANIMELIST, self.name(), &url, &token).await?;
            let page: MalUserSearchResult = serde_json::from_value(value)
                .map_err(|e| DomainError::tracker(format!("MyAnimeList 列表无法解析：{e}")))?;

            let needle = query.to_lowercase();
            for item in &page.data {
                if item.node.title.to_lowercase().contains(&needle) {
                    out.push(self.get_manga_details(item.node.id).await?);
                }
            }
            if page.paging.next.as_deref().map(str::is_empty).unwrap_or(true) {
                break;
            }
            offset += LIST_PAGE;
        }
        Ok(out)
    }
}

#[async_trait]
impl TrackerService for MyAnimeList {
    fn id(&self) -> i32 {
        MYANIMELIST
    }
    fn name(&self) -> &'static str {
        "MyAnimeList"
    }
    fn logo(&self) -> &'static [u8] {
        super::logos::MAL
    }
    fn ctx(&self) -> &TrackerCtx {
        &self.ctx
    }
    fn supports_reading_dates(&self) -> bool {
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
        Ok((0..=10).map(|i| i.to_string()).collect())
    }
    async fn index_to_score(&self, index: i32) -> Result<f64> {
        Ok(index as f64)
    }
    async fn display_score(&self, track: &Track) -> Result<String> {
        Ok((track.score as i32).to_string())
    }

    async fn auth_url(&self) -> Result<Option<String>> {
        let verifier = generate_code_verifier();
        self.ctx.store.set_pkce_verifier(MYANIMELIST, &verifier).await?;
        Ok(Some(format!(
            "{BASE_OAUTH_URL}/authorize?client_id={CLIENT_ID}&code_challenge={verifier}&response_type=code"
        )))
    }

    async fn auth_callback(&self, url: &str) -> Result<()> {
        let code = extract_token(url, "code")
            .ok_or_else(|| DomainError::tracker("MyAnimeList：回调地址里没有 code"))?;
        self.login(&code).await
    }

    /// 上游这里就是把 password 当授权码用（界面上的「密码」填的是 auth code）。
    async fn login_impl(&self, _username: &str, password: &str) -> Result<()> {
        self.login(password).await
    }

    async fn logout(&self) -> Result<()> {
        self.ctx.store.clear_credentials(MYANIMELIST).await?;
        self.ctx.store.clear_pkce_verifier(MYANIMELIST).await
    }

    async fn bind(&self, track: &mut Track, has_read_chapters: bool) -> Result<()> {
        if let Some(remote) = self.find_list_item(track).await? {
            track.copy_personal_from(&remote, true);
            track.remote_id = remote.remote_id;
            if track.status != COMPLETED {
                let is_rereading = track.status == REREADING;
                track.status = if !is_rereading && has_read_chapters { READING } else { track.status };
            }
            self.update(track, false).await
        } else {
            track.status = if has_read_chapters { READING } else { PLAN_TO_READ };
            track.score = 0.0;
            self.update_item(track).await
        }
    }

    async fn update(&self, track: &mut Track, did_read_chapter: bool) -> Result<()> {
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
        self.update_item(track).await
    }

    async fn refresh(&self, track: &mut Track) -> Result<()> {
        match self.find_list_item(track).await? {
            Some(_) => Ok(()),
            None => self.update_item(track).await,
        }
    }

    async fn search(&self, query: &str) -> Result<Vec<TrackSearch>> {
        if let Some(rest) = query.strip_prefix("id:")
            && let Ok(id) = rest.parse::<i64>()
        {
            return Ok(vec![self.get_manga_details(id).await?]);
        }
        if let Some(rest) = query.strip_prefix("my:") {
            return self.find_list_items(rest).await;
        }
        self.search_api(query).await
    }

    async fn delete(&self, track: &Track) -> Result<()> {
        self.delete_item(track).await
    }
}

fn to_my_anime_list_status(status: i32) -> Option<&'static str> {
    match status {
        READING | REREADING => Some("reading"),
        COMPLETED => Some("completed"),
        ON_HOLD => Some("on_hold"),
        DROPPED => Some("dropped"),
        PLAN_TO_READ => Some("plan_to_read"),
        _ => None,
    }
}

/// 站点返回的状态串 → 本地状态码。未知值按 READING 处理（上游 `getStatus` 如此）。
fn from_my_anime_list_status(status: Option<&str>) -> i32 {
    match status {
        Some("completed") => COMPLETED,
        Some("on_hold") => ON_HOLD,
        Some("dropped") => DROPPED,
        Some("plan_to_read") => PLAN_TO_READ,
        _ => READING,
    }
}

fn apply_list_status(track: &mut Track, status: &MalListStatus) {
    track.status = if status.is_rereading.unwrap_or(false) {
        REREADING
    } else {
        from_my_anime_list_status(status.status.as_deref())
    };
    if let Some(v) = status.num_chapters_read {
        track.last_chapter_read = v;
    }
    if let Some(v) = status.score {
        track.score = v as f64;
    }
    if let Some(d) = status.start_date.as_deref() {
        track.started_reading_date = parse_date(d);
    }
    if let Some(d) = status.finish_date.as_deref() {
        track.finished_reading_date = parse_date(d);
    }
}

/// 极简 query 转义：只处理会破坏查询串的字符，值都是搜索关键词。
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

#[derive(Debug, Clone, Deserialize)]
struct MalUser {
    name: String,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
struct MalOAuth {
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    expires_in: i64,
    #[serde(default = "default_created_at")]
    created_at: i64,
}

fn default_created_at() -> i64 {
    now_secs()
}

impl MalOAuth {
    /// 上游假设 token 提前一分钟过期（`adjustedExpiresIn = expiresIn - 60`）。
    fn is_expired(&self) -> bool {
        self.created_at + self.expires_in - 60 < now_secs()
    }
}

#[derive(Debug, Clone, Deserialize)]
struct MalManga {
    id: i64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    synopsis: String,
    #[serde(default)]
    num_chapters: i32,
    #[serde(default = "minus_one")]
    mean: f64,
    #[serde(default)]
    main_picture: Option<MalCover>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    media_type: String,
    #[serde(default)]
    start_date: Option<String>,
}

fn minus_one() -> f64 {
    -1.0
}

#[derive(Debug, Clone, Deserialize)]
struct MalCover {
    #[serde(default)]
    large: String,
}

#[derive(Debug, Clone, Deserialize)]
struct MalListStatus {
    #[serde(default)]
    is_rereading: Option<bool>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    num_chapters_read: Option<f64>,
    #[serde(default)]
    score: Option<i32>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    finish_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MalList {
    #[serde(default)]
    num_chapters: Option<i32>,
    #[serde(default)]
    my_list_status: Option<MalListStatus>,
}

#[derive(Debug, Clone, Deserialize)]
struct MalSearchResult {
    #[serde(default)]
    data: Vec<MalSearchNode>,
}

#[derive(Debug, Clone, Deserialize)]
struct MalSearchNode {
    node: MalSearchItem,
}

#[derive(Debug, Clone, Deserialize)]
struct MalSearchItem {
    id: i64,
    #[serde(default)]
    title: String,
}

#[derive(Debug, Clone, Deserialize)]
struct MalUserSearchResult {
    #[serde(default)]
    data: Vec<MalSearchNode>,
    #[serde(default)]
    paging: MalPaging,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MalPaging {
    #[serde(default)]
    next: Option<String>,
}
