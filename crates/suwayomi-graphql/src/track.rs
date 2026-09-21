//! Tracker / TrackRecord types — mirrors `graphql/types/TrackType.kt`.
//!
//! `TrackerType` 持一个追踪器句柄，各字段按需解析：`isLoggedIn` / `isTokenExpired`
//! 读 `tracker_credential`，`scores` / `statuses` 问站点适配器，`authUrl` 只在未
//! 登录时给（且**只在客户端真的取这个字段时才生成** —— MyAnimeList 的 `authUrl`
//! 会顺带落一个新的 PKCE code_verifier）。

use std::sync::Arc;

use async_graphql::{Context, Object, SimpleObject};

use suwayomi_core::schema::TrackRecordRow;
use suwayomi_domain::sql::bind_placeholders;
use suwayomi_domain::tracker::{Track, TrackSearch, TrackerService};

use crate::scalars::{Cursor, LongString};
use crate::state::GraphQLState;
use crate::types::{MangaType, PageInfo};

/// 一个追踪器（对应上游 `TrackerType`）。
#[derive(Clone)]
pub struct TrackerType {
    service: Arc<dyn TrackerService>,
}

/// 追踪器 logo 的代理地址（上游 `Track.proxyThumbnailUrl`）。
pub fn thumbnail_url(id: i32) -> String {
    format!("/api/v1/track/{id}/thumbnail")
}

impl TrackerType {
    pub fn from_service(service: Arc<dyn TrackerService>) -> Self {
        Self { service }
    }

    /// 按 id 取；未知 id 返回 `None`（上游这里是 `TrackerManager.getTracker(id)`，
    /// 调用方 `requireNotNull`）。
    pub fn find(state: &GraphQLState, id: i32) -> Option<Self> {
        state.tracker.find(id).map(Self::from_service)
    }
}

/// 站点应用凭据（`trackers.json` 里的那一份）。
#[derive(SimpleObject, Clone)]
pub struct TrackerOAuthAppType {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

#[Object]
impl TrackerType {
    async fn id(&self) -> i32 {
        self.service.id()
    }
    async fn name(&self) -> &str {
        self.service.name()
    }
    async fn icon(&self) -> String {
        thumbnail_url(self.service.id())
    }
    /// 站点应用凭据；非 OAuth 站点（MangaUpdates）为 null。
    async fn oauth_app(&self) -> Option<TrackerOAuthAppType> {
        self.service.oauth_app().map(|app| TrackerOAuthAppType {
            client_id: app.client_id,
            client_secret: app.client_secret,
            redirect_uri: app.redirect_uri,
        })
    }
    async fn is_logged_in(&self) -> async_graphql::Result<bool> {
        Ok(self.service.is_logged_in().await?)
    }
    /// 已登录时给 null（上游 `TrackerType` 构造时就是这么定的）。
    async fn auth_url(&self) -> async_graphql::Result<Option<String>> {
        if self.service.is_logged_in().await? {
            return Ok(None);
        }
        Ok(self.service.auth_url().await?)
    }
    async fn supports_track_deletion(&self) -> bool {
        self.service.supports_track_deletion()
    }
    async fn supports_reading_dates(&self) -> bool {
        self.service.supports_reading_dates()
    }
    async fn supports_private_tracking(&self) -> bool {
        self.service.supports_private_tracking()
    }
    async fn is_token_expired(&self) -> async_graphql::Result<bool> {
        Ok(self.service.is_token_expired().await?)
    }
    async fn scores(&self) -> async_graphql::Result<Vec<String>> {
        Ok(self.service.score_list().await?)
    }
    async fn statuses(&self) -> Vec<TrackStatusType> {
        self.service
            .status_list()
            .into_iter()
            .map(|value| TrackStatusType {
                value,
                name: self.service.status_name(value).unwrap_or_default().to_string(),
            })
            .collect()
    }
    async fn track_records(&self, ctx: &Context<'_>) -> async_graphql::Result<TrackRecordNodeList> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM track_record WHERE sync_id = ?");
        let rows = suwayomi_db::query_as::<TrackRecordRow>(&sql)
            .bind(self.service.id())
            .fetch_all(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let nodes: Vec<TrackRecordType> = rows.iter().map(TrackRecordType::from_row).collect();
        Ok(TrackRecordNodeList::from_nodes(nodes))
    }
}

#[derive(SimpleObject, Clone)]
pub struct TrackStatusType {
    pub value: i32,
    pub name: String,
}

/// Mirrors `TrackRecordType`.
#[derive(Clone)]
pub struct TrackRecordType {
    pub id: i32,
    pub manga_id: i32,
    pub tracker_id: i32,
    pub remote_id: i64,
    pub library_id: Option<i64>,
    pub title: String,
    pub last_chapter_read: f64,
    pub total_chapters: i32,
    pub status: i32,
    pub score: f64,
    pub remote_url: String,
    pub start_date: i64,
    pub finish_date: i64,
    pub private: bool,
}

impl TrackRecordType {
    pub fn from_row(row: &TrackRecordRow) -> Self {
        Self {
            id: row.id,
            manga_id: row.manga_id,
            tracker_id: row.sync_id,
            remote_id: row.remote_id,
            library_id: row.library_id,
            title: row.title.clone(),
            last_chapter_read: row.last_chapter_read,
            total_chapters: row.total_chapters,
            status: row.status,
            score: row.score,
            remote_url: row.remote_url.clone(),
            start_date: row.start_date,
            finish_date: row.finish_date,
            private: row.private,
        }
    }

    /// 落库行 → 领域模型（对应上游 `TrackRecordType.toTrack()`）。
    pub fn to_track(&self) -> Track {
        let mut track = Track::create(self.tracker_id);
        track.id = Some(self.id);
        track.manga_id = self.manga_id;
        track.remote_id = self.remote_id;
        track.library_id = self.library_id;
        track.title = self.title.clone();
        track.last_chapter_read = self.last_chapter_read;
        track.total_chapters = self.total_chapters;
        track.status = self.status;
        track.score = self.score;
        track.tracking_url = self.remote_url.clone();
        track.started_reading_date = self.start_date;
        track.finished_reading_date = self.finish_date;
        track.private = self.private;
        track
    }
}

#[Object]
impl TrackRecordType {
    async fn id(&self) -> i32 {
        self.id
    }
    async fn manga_id(&self) -> i32 {
        self.manga_id
    }
    async fn tracker_id(&self) -> i32 {
        self.tracker_id
    }
    async fn remote_id(&self) -> LongString {
        LongString(self.remote_id)
    }
    async fn library_id(&self) -> Option<LongString> {
        self.library_id.map(LongString)
    }
    async fn title(&self) -> &str {
        &self.title
    }
    async fn last_chapter_read(&self) -> f64 {
        self.last_chapter_read
    }
    async fn total_chapters(&self) -> i32 {
        self.total_chapters
    }
    async fn status(&self) -> i32 {
        self.status
    }
    async fn score(&self) -> f64 {
        self.score
    }
    async fn remote_url(&self) -> &str {
        &self.remote_url
    }
    async fn start_date(&self) -> LongString {
        LongString(self.start_date)
    }
    async fn finish_date(&self) -> LongString {
        LongString(self.finish_date)
    }
    async fn private(&self) -> bool {
        self.private
    }

    /// Mirrors `displayScore` — 用追踪器的展示口径渲染 `score`。
    async fn display_score(&self, ctx: &Context<'_>) -> async_graphql::Result<String> {
        let state = ctx.data::<GraphQLState>()?;
        let Some(tracker) = state.tracker.find(self.tracker_id) else {
            return Ok(self.score.to_string());
        };
        let track = self.to_track();
        Ok(tracker.display_score(&track).await?)
    }

    async fn manga(&self, ctx: &Context<'_>) -> async_graphql::Result<MangaType> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM manga WHERE id = ?");
        let row = suwayomi_db::query_as::<suwayomi_core::schema::MangaRow>(&sql)
            .bind(self.manga_id)
            .fetch_one(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(MangaType::from_row(&row))
    }

    async fn tracker(&self, ctx: &Context<'_>) -> Option<TrackerType> {
        let state = ctx.data::<GraphQLState>().ok()?;
        TrackerType::find(state, self.tracker_id)
    }
}

/// Mirrors `TrackSearchType`.
#[derive(SimpleObject, Clone)]
pub struct TrackSearchType {
    pub id: i32,
    pub tracker_id: i32,
    pub remote_id: LongString,
    pub title: String,
    pub total_chapters: i32,
    pub tracking_url: String,
    pub cover_url: String,
    pub summary: String,
    pub publishing_status: String,
    pub publishing_type: String,
    pub start_date: String,
    pub library_id: Option<LongString>,
    pub last_chapter_read: f64,
    pub status: i32,
    pub score: f64,
    pub started_reading_date: LongString,
    pub finished_reading_date: LongString,
    pub private: bool,
}

impl TrackSearchType {
    pub fn from_search(s: &TrackSearch) -> Self {
        Self {
            id: s.id,
            tracker_id: s.tracker_id,
            remote_id: LongString(s.remote_id),
            title: s.title.clone(),
            total_chapters: s.total_chapters,
            tracking_url: s.tracking_url.clone(),
            cover_url: s.cover_url.clone(),
            summary: s.summary.clone(),
            publishing_status: s.publishing_status.clone(),
            publishing_type: s.publishing_type.clone(),
            start_date: s.start_date.clone(),
            library_id: s.library_id.map(LongString),
            last_chapter_read: s.last_chapter_read,
            status: s.status,
            score: s.score,
            started_reading_date: LongString(s.started_reading_date),
            finished_reading_date: LongString(s.finished_reading_date),
            private: s.private,
        }
    }
}

/// Mirrors `SearchTrackerPayload`.
#[derive(SimpleObject, Clone)]
pub struct SearchTrackerPayload {
    pub track_searches: Vec<TrackSearchType>,
}

// ---- NodeLists ----

#[derive(SimpleObject, Clone)]
pub struct TrackerEdge {
    pub cursor: Cursor,
    pub node: TrackerType,
}

#[derive(SimpleObject, Clone)]
pub struct TrackerNodeList {
    pub nodes: Vec<TrackerType>,
    pub edges: Vec<TrackerEdge>,
    pub page_info: PageInfo,
    pub total_count: i32,
}

impl TrackerNodeList {
    pub fn from_nodes(nodes: Vec<TrackerType>) -> Self {
        let total = nodes.len() as i32;
        let edges = if nodes.is_empty() {
            vec![]
        } else if nodes.len() == 1 {
            vec![TrackerEdge { cursor: Cursor("0".into()), node: nodes[0].clone() }]
        } else {
            vec![
                TrackerEdge { cursor: Cursor("0".into()), node: nodes[0].clone() },
                TrackerEdge { cursor: Cursor((nodes.len() - 1).to_string()), node: nodes[nodes.len() - 1].clone() },
            ]
        };
        Self {
            page_info: PageInfo {
                start_cursor: Some(Cursor("0".into())),
                end_cursor: Some(Cursor(total.saturating_sub(1).to_string())),
                has_next_page: false,
                has_previous_page: false,
            },
            nodes,
            edges,
            total_count: total,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct TrackRecordEdge {
    pub cursor: Cursor,
    pub node: TrackRecordType,
}

#[derive(SimpleObject, Clone)]
pub struct TrackRecordNodeList {
    pub nodes: Vec<TrackRecordType>,
    pub edges: Vec<TrackRecordEdge>,
    pub page_info: PageInfo,
    pub total_count: i32,
}

impl TrackRecordNodeList {
    pub fn from_nodes(nodes: Vec<TrackRecordType>) -> Self {
        let total = nodes.len() as i32;
        let edges = if nodes.is_empty() {
            vec![]
        } else if nodes.len() == 1 {
            vec![TrackRecordEdge { cursor: Cursor("0".into()), node: nodes[0].clone() }]
        } else {
            vec![
                TrackRecordEdge { cursor: Cursor("0".into()), node: nodes[0].clone() },
                TrackRecordEdge { cursor: Cursor((nodes.len() - 1).to_string()), node: nodes[nodes.len() - 1].clone() },
            ]
        };
        Self {
            page_info: PageInfo {
                start_cursor: Some(Cursor("0".into())),
                end_cursor: Some(Cursor(total.saturating_sub(1).to_string())),
                has_next_page: false,
                has_previous_page: false,
            },
            nodes,
            edges,
            total_count: total,
        }
    }

    pub fn empty() -> Self {
        Self::from_nodes(vec![])
    }
}
