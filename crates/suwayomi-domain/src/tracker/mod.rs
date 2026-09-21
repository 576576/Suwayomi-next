//! 追踪器（MyAnimeList / AniList / Kitsu / Shikimori / Bangumi / MangaUpdates）
//! —— 上游 `manga/impl/track/Track.kt` + `track/tracker/*` 的对应物。
//!
//! 分工：本模块放模型（[`Track`] / [`TrackSearch`]）与编排（[`TrackerManager`]，
//! 含 `track_record` / `track_search` 的落库与 `bind`/`update`/`unbind`/
//! `trackChapter` 等对外语义）；站点各自的 HTTP 细节在 [`service`] 的实现里；
//! 凭据在 [`store`]。
//!
//! 追踪器列表的顺序**保持本仓现状**（1,2,3,4,5,7），与上游 `TrackerManager.
//! services` 的 1,2,3,7,4,5 不同：列表顺序对外可见，本仓 WebUI 与既有测试都按
//! 前者，改顺序会改动界面上的排列。

pub mod anilist;
pub mod bangumi;
pub mod kitsu;
mod logos;
pub mod mangaupdates;
pub mod myanimelist;
pub mod oauth;
mod service;
pub mod shikimori;
mod store;

use std::sync::{Arc, RwLock};

use suwayomi_core::db::Db;
use suwayomi_core::schema::{ChapterRow, TrackRecordRow, TrackSearchRow};

use crate::error::{DomainError, Result};

use oauth::TrackerOAuthApps;

pub use service::{TrackerCtx, TrackerService, extract_token};
pub use store::{TrackerCredential, TrackerStore};

/// 补丁里给了就用它（去掉首尾空白）；留空或全空白 = 回到内置默认值。
fn filled(value: String, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

/// 桌面端默认 UA。部分站点（MAL）缺 UA 会直接拒请求。
pub const USER_AGENT: &str = concat!("Suwayomi-next/", env!("CARGO_PKG_VERSION"));

// 上游 `TrackerManager` 的常量。
pub const MYANIMELIST: i32 = 1;
pub const ANILIST: i32 = 2;
pub const KITSU: i32 = 3;
pub const SHIKIMORI: i32 = 4;
pub const BANGUMI: i32 = 5;
pub const MANGA_UPDATES: i32 = 7;

/// 一个追踪器。与上游 `Track` 模型一一对应；`id` 为 `None` 表示还没落库。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    pub id: Option<i32>,
    pub manga_id: i32,
    pub tracker_id: i32,
    pub remote_id: i64,
    pub library_id: Option<i64>,
    pub title: String,
    pub last_chapter_read: f64,
    pub total_chapters: i32,
    pub score: f64,
    pub status: i32,
    pub started_reading_date: i64,
    pub finished_reading_date: i64,
    pub tracking_url: String,
    pub private: bool,
}

impl Track {
    /// 对应上游 `Track.create(serviceId)`。
    pub fn create(tracker_id: i32) -> Self {
        Self { tracker_id, ..Default::default() }
    }

    /// 对应上游 `copyPersonalFrom`：只搬「用户数据」，不搬 `remote_id` /
    /// `library_id`（那两项由调用方按站点返回单独设）。
    pub fn copy_personal_from(&mut self, other: &Track, copy_remote_private: bool) {
        self.last_chapter_read = other.last_chapter_read;
        self.score = other.score;
        self.status = other.status;
        self.started_reading_date = other.started_reading_date;
        self.finished_reading_date = other.finished_reading_date;
        if copy_remote_private {
            self.private = other.private;
        }
    }

    fn from_row(row: &TrackRecordRow) -> Self {
        Self {
            id: Some(row.id),
            manga_id: row.manga_id,
            tracker_id: row.sync_id,
            remote_id: row.remote_id,
            library_id: row.library_id,
            title: row.title.clone(),
            last_chapter_read: row.last_chapter_read,
            total_chapters: row.total_chapters,
            score: row.score,
            status: row.status,
            started_reading_date: row.start_date,
            finished_reading_date: row.finish_date,
            tracking_url: row.remote_url.clone(),
            private: row.private,
        }
    }
}

/// 一次搜索命中的条目。落库后 `id` 才有值。
///
/// `authors` / `artists` 只落库，不出现在 REST 或 GraphQL 的响应里（上游
/// `TrackSearchDataClass` 与 `TrackSearchType` 都没有这两个字段），所以标了
/// `serde(skip)`。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackSearch {
    pub id: i32,
    pub tracker_id: i32,
    pub remote_id: i64,
    pub library_id: Option<i64>,
    pub title: String,
    pub last_chapter_read: f64,
    pub total_chapters: i32,
    pub tracking_url: String,
    pub cover_url: String,
    pub summary: String,
    pub publishing_status: String,
    pub publishing_type: String,
    pub start_date: String,
    pub status: i32,
    pub score: f64,
    pub score_string: Option<String>,
    pub started_reading_date: i64,
    pub finished_reading_date: i64,
    pub private: bool,
    #[serde(skip)]
    pub authors: Vec<String>,
    #[serde(skip)]
    pub artists: Vec<String>,
}

impl TrackSearch {
    pub fn create(tracker_id: i32) -> Self {
        Self { tracker_id, ..Default::default() }
    }

    fn from_row(row: &TrackSearchRow) -> Self {
        Self {
            id: row.id,
            tracker_id: row.tracker_id,
            remote_id: row.remote_id,
            library_id: row.library_id,
            title: row.title.clone(),
            last_chapter_read: row.last_chapter_read,
            total_chapters: row.total_chapters,
            tracking_url: row.tracking_url.clone(),
            cover_url: row.cover_url.clone(),
            summary: row.summary.clone(),
            publishing_status: row.publishing_status.clone(),
            publishing_type: row.publishing_type.clone(),
            start_date: row.start_date.clone(),
            status: row.status,
            score: row.score,
            score_string: None,
            started_reading_date: row.started_reading_date,
            finished_reading_date: row.finished_reading_date,
            private: row.private,
            authors: split_list(row.authors.as_deref()),
            artists: split_list(row.artists.as_deref()),
        }
    }
}

fn split_list(v: Option<&str>) -> Vec<String> {
    match v {
        Some(s) if !s.is_empty() => s.split(',').map(|x| x.to_string()).collect(),
        _ => Vec::new(),
    }
}

fn join_list(v: &[String]) -> Option<String> {
    (!v.is_empty()).then(|| v.join(","))
}

/// 列表接口返回的一个追踪器（对应上游 `TrackerDataClass` 去掉 icon）。
#[derive(Debug, Clone)]
pub struct TrackerDescriptor {
    pub id: i32,
    pub name: String,
    pub is_login: bool,
    pub auth_url: Option<String>,
}

/// `update` 的入参，对应上游 `Track.UpdateInput`。
#[derive(Debug, Clone, Default)]
pub struct TrackUpdate {
    pub record_id: i32,
    pub status: Option<i32>,
    pub last_chapter_read: Option<f64>,
    pub score_string: Option<String>,
    pub start_date: Option<i64>,
    pub finish_date: Option<i64>,
    /// 已废弃，等价于 `unbind(recordId, false)`；上游保留是为了兼容老前端。
    pub unbind: Option<bool>,
    pub private: Option<bool>,
}

/// 追踪器注册表 + 编排。REST 与 GraphQL 共用同一个句柄。
#[derive(Clone)]
pub struct TrackerManager {
    services: Arc<Vec<Arc<dyn TrackerService>>>,
    db: Db,
    store: TrackerStore,
    oauth: Arc<RwLock<TrackerOAuthApps>>,
    /// `trackers.json` 的位置（设置页改凭据要落盘）；`None` = 没有配置文件可用。
    oauth_config: Option<std::path::PathBuf>,
}

/// 设置页提交上来的站点应用凭据：`None` 或空串 = 回到内置默认值。
#[derive(Debug, Clone, Default)]
pub struct OAuthAppPatch {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub redirect_uri: Option<String>,
}

impl TrackerManager {
    pub fn new(db: Db) -> Self {
        Self::build(db, Arc::new(RwLock::new(TrackerOAuthApps::default())), None)
    }

    /// 同 [`Self::new`]，但用 `trackers.json` 里读到的站点应用凭据，并记住它的位置
    /// 以便设置页改动落盘。
    pub fn with_oauth(
        db: Db,
        oauth: Arc<RwLock<TrackerOAuthApps>>,
        oauth_config: std::path::PathBuf,
    ) -> Self {
        Self::build(db, oauth, Some(oauth_config))
    }

    fn build(db: Db, oauth: Arc<RwLock<TrackerOAuthApps>>, oauth_config: Option<std::path::PathBuf>) -> Self {
        let store = TrackerStore::new(db.clone());
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_default();
        let ctx = TrackerCtx::with_oauth(store.clone(), http, oauth.clone());
        let services: Vec<Arc<dyn TrackerService>> = vec![
            Arc::new(myanimelist::MyAnimeList::new(ctx.clone())),
            Arc::new(anilist::AniList::new(ctx.clone())),
            Arc::new(kitsu::Kitsu::new(ctx.clone())),
            Arc::new(shikimori::Shikimori::new(ctx.clone())),
            Arc::new(bangumi::Bangumi::new(ctx.clone())),
            Arc::new(mangaupdates::MangaUpdates::new(ctx)),
        ];
        Self { services: Arc::new(services), db, store, oauth, oauth_config }
    }

    /// 某个站点的应用凭据（非 OAuth 站点为 `None`）。
    pub fn oauth_app(&self, tracker_id: i32) -> Option<oauth::AppCredentials> {
        self.oauth.read().unwrap_or_else(|e| e.into_inner()).app(tracker_id).cloned()
    }

    /// 改站点应用凭据：立刻生效（下一次登录/刷新就用新值）并落盘（重启后仍在）。
    /// 补丁里留空或没给的字段，回落到内置默认值。
    pub fn update_oauth_app(&self, tracker_id: i32, patch: OAuthAppPatch) -> Result<()> {
        let builtin = TrackerOAuthApps::default()
            .app(tracker_id)
            .cloned()
            .ok_or_else(|| DomainError::tracker("该追踪器不使用应用凭据"))?;
        let app = oauth::AppCredentials {
            client_id: patch.client_id.map_or(builtin.client_id.clone(), |v| filled(v, &builtin.client_id)),
            client_secret: patch
                .client_secret
                .map_or(builtin.client_secret.clone(), |v| filled(v, &builtin.client_secret)),
            redirect_uri: patch
                .redirect_uri
                .map_or(builtin.redirect_uri.clone(), |v| filled(v, &builtin.redirect_uri)),
        };

        let path = self
            .oauth_config
            .as_ref()
            .ok_or_else(|| DomainError::tracker("没有可写的 trackers.json 路径"))?;
        let mut guard = self.oauth.write().unwrap_or_else(|e| e.into_inner());
        guard.set_app(tracker_id, app);
        oauth::save(path, &guard)
            .map_err(|e| DomainError::tracker(format!("写入 trackers.json 失败：{e}")))?;
        drop(guard);
        tracing::info!("tracker oauth app updated: {} ({})", tracker_id, path.display());
        Ok(())
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub fn store(&self) -> &TrackerStore {
        &self.store
    }

    /// 全部追踪器，顺序即对外列表顺序。
    pub fn services(&self) -> &[Arc<dyn TrackerService>] {
        &self.services
    }

    pub fn find(&self, id: i32) -> Option<Arc<dyn TrackerService>> {
        self.services.iter().find(|s| s.id() == id).cloned()
    }

    /// 找不到就报 `NotFound`（对应上游 `getTracker(id)!!`）。
    pub fn get(&self, id: i32) -> Result<Arc<dyn TrackerService>> {
        self.find(id).ok_or(DomainError::TrackerNotFound(id))
    }

    pub async fn has_logged_tracker(&self) -> bool {
        for s in self.services.iter() {
            if s.is_logged_in().await.unwrap_or(false) {
                return true;
            }
        }
        false
    }

    /// 对应上游 `Track.getTrackerList()`。
    pub async fn list(&self) -> Vec<TrackerDescriptor> {
        let mut out = Vec::with_capacity(self.services.len());
        for s in self.services.iter() {
            let is_login = s.is_logged_in().await.unwrap_or(false);
            let auth_url = if is_login { None } else { s.auth_url().await.unwrap_or(None) };
            out.push(TrackerDescriptor { id: s.id(), name: s.name().to_string(), is_login, auth_url });
        }
        out
    }

    /// 对应上游 `Track.login()`：给了 callbackUrl 就走 OAuth 回调，否则当密码登录。
    ///
    /// 失败时**不清**已有凭据 —— 上游 REST `Track.login` / GraphQL
    /// `loginTrackerCredentials` 都是直接调 `authCallback` / `loginImpl`，没有失败
    /// 兜底；站点报错后旧的登录态仍然保留。
    pub async fn login(&self, tracker_id: i32, callback_url: Option<&str>, username: &str, password: &str) -> Result<()> {
        let tracker = self.get(tracker_id)?;
        match callback_url {
            Some(url) => tracker.auth_callback(url).await,
            None => tracker.login_impl(username, password).await,
        }
    }

    pub async fn logout(&self, tracker_id: i32) -> Result<()> {
        self.get(tracker_id)?.logout().await
    }

    /// 对应上游 `Track.search()`：搜索并**落 `track_search`**，返回落库后的行。
    pub async fn search(&self, tracker_id: i32, query: &str) -> Result<Vec<TrackSearch>> {
        let tracker = self.get(tracker_id)?;
        let hits = tracker.search(query).await?;
        if hits.is_empty() {
            return Ok(Vec::new());
        }
        let rows = self.insert_track_searches(&hits).await?;
        Ok(rows.iter().map(TrackSearch::from_row).collect())
    }

    /// 对应上游 `Track.bind()`。
    pub async fn bind(&self, manga_id: i32, tracker_id: i32, remote_id: i64, private: bool) -> Result<i32> {
        let tracker = self.get(tracker_id)?;

        let mut track = match self.track_from_search(tracker_id, remote_id, manga_id).await? {
            Some(t) => t,
            None => {
                let row = suwayomi_db::query_as::<TrackRecordRow>(
                    "SELECT * FROM track_record WHERE sync_id = ? AND remote_id = ?",
                )
                .bind(tracker_id)
                .bind(remote_id)
                .fetch_optional(&self.db)
                .await?
                .ok_or_else(|| {
                    DomainError::not_found(format!("track_record(sync_id={tracker_id}, remote_id={remote_id})"))
                })?;
                Track::from_row(&row)
            }
        };
        track.manga_id = manga_id;
        track.private = private;

        let read_chapter = self.max_read_chapter(manga_id).await?;
        let has_read_chapters = read_chapter.is_some();
        let chapter_number = read_chapter.as_ref().map(|c| c.chapter_number as f64);

        tracker.bind(&mut track, has_read_chapters).await?;
        let record_id = self.upsert_track_record(&track).await?;

        // 绑定后本地阅读进度比站点新时，立刻推一次；否则要等下一次翻页才同步。
        let mut last_chapter_read = None;
        if let Some(n) = chapter_number
            && n > 0.0
            && n > track.last_chapter_read
        {
            last_chapter_read = Some(n);
        }
        let mut start_date = None;
        if track.started_reading_date <= 0
            && let Some(oldest) = self.oldest_read_chapter(manga_id).await?
        {
            start_date = Some(oldest.last_read_at * 1000);
        }
        if last_chapter_read.is_some() || start_date.is_some() {
            self.update(TrackUpdate { record_id, last_chapter_read, start_date, ..Default::default() })
                .await?;
        }
        Ok(record_id)
    }

    /// 对应上游 `Track.bindTrackRecord()`。
    pub async fn bind_track_record(&self, manga_id: i32, record_id: i32) -> Result<i32> {
        let source = self.track_record_row(record_id).await?;
        let source_track = Track::from_row(&source);

        if source.manga_id == manga_id {
            return Ok(record_id);
        }

        let existing: Option<i32> =
            suwayomi_db::query_scalar("SELECT id FROM track_record WHERE manga_id = ? AND sync_id = ?")
                .bind(manga_id)
                .bind(source.sync_id)
                .fetch_optional(&self.db)
                .await?;

        match existing {
            // 目标漫画已有该追踪器的记录：把来源记录的内容并进目标行。来源行**不删** ——
            // 客户端迁移流程的 cleanup 步骤会自己调 `unbindTrack` 收拾它，这里先删会让
            // 那次调用打到已不存在的记录上报错。
            Some(target_id) => {
                let mut moved = source_track;
                moved.id = Some(target_id);
                moved.manga_id = manga_id;
                self.update_track_record(&moved).await?;
                Ok(target_id)
            }
            None => {
                let mut moved = source_track;
                moved.id = None;
                moved.manga_id = manga_id;
                self.insert_track_record(&moved).await
            }
        }
    }

    /// 对应上游 `Track.refresh()`。
    pub async fn refresh(&self, record_id: i32) -> Result<()> {
        let row = self.track_record_row(record_id).await?;
        let tracker = self.get(row.sync_id)?;
        let mut track = Track::from_row(&row);
        tracker.refresh(&mut track).await?;
        self.upsert_track_record(&track).await?;
        Ok(())
    }

    /// 对应上游 `Track.unbind()`。`delete_remote_track` 只在站点支持删除时生效。
    pub async fn unbind(&self, record_id: i32, delete_remote_track: bool) -> Result<()> {
        let row = self.track_record_row(record_id).await?;
        if delete_remote_track
            && let Some(tracker) = self.find(row.sync_id)
            && tracker.supports_track_deletion()
        {
            tracker.delete(&Track::from_row(&row)).await?;
        }
        suwayomi_db::query("DELETE FROM track_record WHERE id = ?")
            .bind(record_id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    /// 对应上游 `Track.update()`。
    pub async fn update(&self, input: TrackUpdate) -> Result<i32> {
        if input.unbind == Some(true) {
            self.unbind(input.record_id, false).await?;
            return Ok(input.record_id);
        }

        let row = self.track_record_row(input.record_id).await?;
        let tracker = self.get(row.sync_id)?;
        let mut track = Track::from_row(&row);

        if let Some(status) = input.status {
            track.status = status;
            if status == tracker.completion_status() && track.total_chapters != 0 {
                track.last_chapter_read = track.total_chapters as f64;
            }
        }
        if let Some(progress) = input.last_chapter_read {
            if track.last_chapter_read == 0.0
                && track.last_chapter_read < progress
                && track.status != tracker.rereading_status()
            {
                track.status = tracker.reading_status();
            }
            track.last_chapter_read = progress;
            if track.total_chapters != 0 && progress as i32 == track.total_chapters {
                track.status = tracker.completion_status();
                track.finished_reading_date = service::now_millis();
            }
        }
        if let Some(score_string) = input.score_string {
            let list = tracker.score_list().await?;
            let index = list.iter().position(|s| s == &score_string).map(|i| i as i32).unwrap_or(-1);
            track.score = tracker.index_to_score(index).await?;
        }
        if let Some(v) = input.start_date {
            track.started_reading_date = v;
        }
        if let Some(v) = input.finish_date {
            track.finished_reading_date = v;
        }
        if let Some(v) = input.private {
            track.private = v;
        }

        tracker.update(&mut track, false).await?;
        self.upsert_track_record(&track).await
    }

    /// 对应上游 `Track.asyncTrackChapter()` 的单漫画版本 `trackChapter(mangaId)`。
    pub async fn track_chapter(&self, manga_id: i32) -> Result<()> {
        let Some(chapter) = self.max_read_chapter(manga_id).await? else {
            return Ok(());
        };
        let number = chapter.chapter_number as f64;
        if number <= 0.0 {
            return Ok(());
        }
        self.track_chapter_for_manga(manga_id, number).await;
        Ok(())
    }

    /// 对应上游 `Track.asyncTrackChapter()`：整库更新跑完后按漫画批量推进。
    pub async fn track_chapters(&self, manga_ids: &[i32]) {
        if !self.has_logged_tracker().await {
            return;
        }
        for id in manga_ids {
            let _ = self.track_chapter(*id).await;
        }
    }

    /// 单个追踪器失败不影响同一漫画的其它追踪器（上游逐个 try/catch）。
    async fn track_chapter_for_manga(&self, manga_id: i32, chapter_number: f64) {
        let records = suwayomi_db::query_as::<TrackRecordRow>("SELECT * FROM track_record WHERE manga_id = ?")
            .bind(manga_id)
            .fetch_all(&self.db)
            .await;
        let Ok(records) = records else {
            return;
        };
        for row in &records {
            let Some(tracker) = self.find(row.sync_id) else {
                continue;
            };
            let mut track = Track::from_row(row);

            // 本地已记到这一章就不用推；站点上可能更超前，交给 refresh 判断。
            if row.last_chapter_read == chapter_number {
                continue;
            }
            if !tracker.is_logged_in().await.unwrap_or(false) {
                let _ = self.upsert_track_record(&track).await;
                continue;
            }
            if tracker.refresh(&mut track).await.is_err() {
                continue;
            }
            let _ = self.upsert_track_record(&track).await;

            if chapter_number > track.last_chapter_read {
                track.last_chapter_read = chapter_number;
                if tracker.update(&mut track, true).await.is_ok() {
                    let _ = self.upsert_track_record(&track).await;
                }
            }
        }
    }

    // ---- DB ----

    async fn track_record_row(&self, record_id: i32) -> Result<TrackRecordRow> {
        suwayomi_db::query_as::<TrackRecordRow>("SELECT * FROM track_record WHERE id = ?")
            .bind(record_id)
            .fetch_optional(&self.db)
            .await?
            .ok_or_else(|| DomainError::not_found(format!("track_record(id={record_id})")))
    }

    async fn track_from_search(&self, tracker_id: i32, remote_id: i64, manga_id: i32) -> Result<Option<Track>> {
        let row = suwayomi_db::query_as::<TrackSearchRow>(
            "SELECT * FROM track_search WHERE tracker_id = ? AND remote_id = ?",
        )
        .bind(tracker_id)
        .bind(remote_id)
        .fetch_optional(&self.db)
        .await?;
        Ok(row.map(|r| {
            let mut t = Track::create(tracker_id);
            t.manga_id = manga_id;
            t.remote_id = r.remote_id;
            t.title = r.title;
            t.total_chapters = r.total_chapters;
            t.tracking_url = r.tracking_url;
            t
        }))
    }

    /// 已读章节里章节号最大的那条（`chapter_number` 降序、`read = true`）。
    async fn max_read_chapter(&self, manga_id: i32) -> Result<Option<ChapterRow>> {
        Ok(suwayomi_db::query_as::<ChapterRow>(
            "SELECT * FROM chapter WHERE manga = ? AND read = TRUE ORDER BY chapter_number DESC LIMIT 1",
        )
        .bind(manga_id)
        .fetch_optional(&self.db)
        .await?)
    }

    /// 最早读的那一章，用来补 `start_date`。
    async fn oldest_read_chapter(&self, manga_id: i32) -> Result<Option<ChapterRow>> {
        Ok(suwayomi_db::query_as::<ChapterRow>(
            "SELECT * FROM chapter WHERE manga = ? AND read = TRUE ORDER BY last_read_at ASC LIMIT 1",
        )
        .bind(manga_id)
        .fetch_optional(&self.db)
        .await?)
    }

    /// 对应上游 `Track.upsertTrackRecord()`：按 (manga_id, tracker_id) 判定新增还是更新。
    pub async fn upsert_track_record(&self, track: &Track) -> Result<i32> {
        let existing: Option<i32> =
            suwayomi_db::query_scalar("SELECT id FROM track_record WHERE manga_id = ? AND sync_id = ?")
                .bind(track.manga_id)
                .bind(track.tracker_id)
                .fetch_optional(&self.db)
                .await?;
        match existing {
            Some(id) => {
                let mut owned = track.clone();
                owned.id = Some(id);
                self.update_track_record(&owned).await?;
                Ok(id)
            }
            None => self.insert_track_record(track).await,
        }
    }

    async fn update_track_record(&self, track: &Track) -> Result<()> {
        let id = track.id.ok_or_else(|| DomainError::invalid("track.id 为空，不能更新"))?;
        suwayomi_db::query(
            "UPDATE track_record SET remote_id = ?, library_id = ?, title = ?, last_chapter_read = ?, \
             total_chapters = ?, status = ?, score = ?, remote_url = ?, start_date = ?, finish_date = ?, private = ? \
             WHERE id = ?",
        )
        .bind(track.remote_id)
        .bind(track.library_id)
        .bind(&track.title)
        .bind(track.last_chapter_read)
        .bind(track.total_chapters)
        .bind(track.status)
        .bind(track.score)
        .bind(&track.tracking_url)
        .bind(track.started_reading_date)
        .bind(track.finished_reading_date)
        .bind(track.private)
        .bind(id)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    async fn insert_track_record(&self, track: &Track) -> Result<i32> {
        let id: i32 = suwayomi_db::query_scalar(
            "INSERT INTO track_record (manga_id, sync_id, remote_id, library_id, title, last_chapter_read, \
             total_chapters, status, score, remote_url, start_date, finish_date, private) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(track.manga_id)
        .bind(track.tracker_id)
        .bind(track.remote_id)
        .bind(track.library_id)
        .bind(&track.title)
        .bind(track.last_chapter_read)
        .bind(track.total_chapters)
        .bind(track.status)
        .bind(track.score)
        .bind(&track.tracking_url)
        .bind(track.started_reading_date)
        .bind(track.finished_reading_date)
        .bind(track.private)
        .fetch_one(&self.db)
        .await?;
        Ok(id)
    }

    async fn insert_track_searches(&self, hits: &[TrackSearch]) -> Result<Vec<TrackSearchRow>> {
        let mut rows = Vec::with_capacity(hits.len());
        for hit in hits {
            let row = suwayomi_db::query_as::<TrackSearchRow>(
                "INSERT INTO track_search (tracker_id, remote_id, title, total_chapters, tracking_url, cover_url, \
                 summary, publishing_status, publishing_type, start_date, library_id, last_chapter_read, status, \
                 score, started_reading_date, finished_reading_date, private, authors, artists) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING *",
            )
            .bind(hit.tracker_id)
            .bind(hit.remote_id)
            .bind(&hit.title)
            .bind(hit.total_chapters)
            .bind(&hit.tracking_url)
            .bind(&hit.cover_url)
            .bind(&hit.summary)
            .bind(&hit.publishing_status)
            .bind(&hit.publishing_type)
            .bind(&hit.start_date)
            .bind(hit.library_id)
            .bind(hit.last_chapter_read)
            .bind(hit.status)
            .bind(hit.score)
            .bind(hit.started_reading_date)
            .bind(hit.finished_reading_date)
            .bind(hit.private)
            .bind(join_list(&hit.authors))
            .bind(join_list(&hit.artists))
            .fetch_one(&self.db)
            .await?;
            rows.push(row);
        }
        Ok(rows)
    }

    /// 备份导出：`track_record` 整表 + 凭据整表。
    pub async fn export_all(&self) -> Result<(Vec<TrackRecordRow>, Vec<(i32, TrackerCredential)>)> {
        let records = suwayomi_db::query_as::<TrackRecordRow>("SELECT * FROM track_record ORDER BY id")
            .fetch_all(&self.db)
            .await?;
        let credentials = self.store.dump_all().await?;
        Ok((records, credentials))
    }

    /// 备份恢复：按 `id` 覆盖写回 `track_record`。
    pub async fn import_record(&self, row: &TrackRecordRow) -> Result<()> {
        suwayomi_db::query(
            "INSERT INTO track_record (id, manga_id, sync_id, remote_id, library_id, title, last_chapter_read, \
             total_chapters, status, score, remote_url, start_date, finish_date, private) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (id) DO UPDATE SET manga_id = ?, sync_id = ?, remote_id = ?, library_id = ?, title = ?, \
             last_chapter_read = ?, total_chapters = ?, status = ?, score = ?, remote_url = ?, start_date = ?, \
             finish_date = ?, private = ?",
        )
        .bind(row.id)
        .bind(row.manga_id)
        .bind(row.sync_id)
        .bind(row.remote_id)
        .bind(row.library_id)
        .bind(&row.title)
        .bind(row.last_chapter_read)
        .bind(row.total_chapters)
        .bind(row.status)
        .bind(row.score)
        .bind(&row.remote_url)
        .bind(row.start_date)
        .bind(row.finish_date)
        .bind(row.private)
        .bind(row.manga_id)
        .bind(row.sync_id)
        .bind(row.remote_id)
        .bind(row.library_id)
        .bind(&row.title)
        .bind(row.last_chapter_read)
        .bind(row.total_chapters)
        .bind(row.status)
        .bind(row.score)
        .bind(&row.remote_url)
        .bind(row.start_date)
        .bind(row.finish_date)
        .bind(row.private)
        .execute(&self.db)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn manager() -> TrackerManager {
        let db = suwayomi_core::db::Db::sqlite_in_memory().await.expect("connect");
        TrackerManager::new(db)
    }

    /// 列表顺序对外可见（WebUI 按它排列），改动即破坏既有界面顺序。
    #[tokio::test]
    async fn service_order_is_stable() {
        let ids: Vec<i32> = manager().await.services().iter().map(|s| s.id()).collect();
        assert_eq!(ids, vec![MYANIMELIST, ANILIST, KITSU, SHIKIMORI, BANGUMI, MANGA_UPDATES]);
    }

    /// `suwayomi_core::backup` 用自己那份 id 清单丢弃备份里不支持的追踪器，
    /// 两份必须一致，否则备份恢复会静默丢掉能用的记录。
    #[tokio::test]
    async fn supported_ids_match_backup_module() {
        let mut ids: Vec<i32> = manager().await.services().iter().map(|s| s.id()).collect();
        ids.sort_unstable();
        let mut backup_ids = suwayomi_core::backup::SUPPORTED_TRACKER_IDS.to_vec();
        backup_ids.sort_unstable();
        assert_eq!(ids, backup_ids);
    }
}
