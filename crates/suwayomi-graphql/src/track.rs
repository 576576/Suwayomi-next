//! Tracker / TrackRecord types — mirrors `graphql/types/TrackType.kt`.

use async_graphql::{Context, Object, SimpleObject};

use suwayomi_core::schema::TrackRecordRow;
use suwayomi_domain::sql::bind_placeholders;

use crate::scalars::{Cursor, LongString};
use crate::state::GraphQLState;
use crate::types::{MangaType, PageInfo};

/// One built-in tracker descriptor (mirrors `TrackerManager.services`).
pub struct TrackerInfo {
    pub id: i32,
    pub name: &'static str,
    pub supports_reading_dates: bool,
    pub supports_private_tracking: bool,
    pub supports_track_deletion: bool,
    pub auth_url: Option<&'static str>,
}

/// Built-in tracker registry (mirrors `TrackerManager.services`).
pub const TRACKERS: &[TrackerInfo] = &[
    TrackerInfo {
        id: 1,
        name: "MyAnimeList",
        supports_reading_dates: true,
        supports_private_tracking: false,
        supports_track_deletion: false,
        auth_url: Some("https://myanimelist.net/"),
    },
    TrackerInfo {
        id: 2,
        name: "Anilist",
        supports_reading_dates: true,
        supports_private_tracking: true,
        supports_track_deletion: false,
        auth_url: Some("https://anilist.co/api/v2/oauth/authorize"),
    },
    TrackerInfo {
        id: 3,
        name: "Kitsu",
        supports_reading_dates: true,
        supports_private_tracking: true,
        supports_track_deletion: false,
        auth_url: None,
    },
    TrackerInfo {
        id: 4,
        name: "Shikimori",
        supports_reading_dates: false,
        supports_private_tracking: false,
        supports_track_deletion: false,
        auth_url: Some("https://shikimori.one/oauth/authorize"),
    },
    TrackerInfo {
        id: 5,
        name: "Bangumi",
        supports_reading_dates: false,
        supports_private_tracking: true,
        supports_track_deletion: false,
        auth_url: Some("https://bgm.tv/oauth/authorize"),
    },
    TrackerInfo {
        id: 7,
        name: "MangaUpdates",
        supports_reading_dates: false,
        supports_private_tracking: false,
        supports_track_deletion: false,
        auth_url: None,
    },
];

/// Mirrors `TrackerType`.
#[derive(Clone)]
pub struct TrackerType {
    pub id: i32,
    pub name: String,
    pub is_logged_in: bool,
    pub auth_url: Option<String>,
    pub supports_track_deletion: bool,
    pub supports_reading_dates: bool,
    pub supports_private_tracking: bool,
}

impl TrackerType {
    pub fn by_id(id: i32, is_logged_in: bool) -> Option<Self> {
        TRACKERS.iter().find(|t| t.id == id).map(|t| Self {
            id: t.id,
            name: t.name.to_string(),
            is_logged_in,
            auth_url: if is_logged_in { None } else { t.auth_url.map(|s| s.to_string()) },
            supports_track_deletion: t.supports_track_deletion,
            supports_reading_dates: t.supports_reading_dates,
            supports_private_tracking: t.supports_private_tracking,
        })
    }

    pub fn all() -> Vec<Self> {
        TRACKERS.iter().map(|t| Self::by_id(t.id, false).expect("known tracker")).collect()
    }
}

#[Object]
impl TrackerType {
    async fn id(&self) -> i32 {
        self.id
    }
    async fn name(&self) -> &str {
        &self.name
    }
    async fn icon(&self) -> String {
        format!("/api/v1/track/{}/thumbnail", self.id)
    }
    async fn is_logged_in(&self) -> bool {
        self.is_logged_in
    }
    async fn auth_url(&self) -> Option<&str> {
        self.auth_url.as_deref()
    }
    async fn supports_track_deletion(&self) -> bool {
        self.supports_track_deletion
    }
    async fn supports_reading_dates(&self) -> bool {
        self.supports_reading_dates
    }
    async fn supports_private_tracking(&self) -> bool {
        self.supports_private_tracking
    }
    async fn is_token_expired(&self) -> bool {
        false
    }
    async fn scores(&self) -> Vec<String> {
        vec![]
    }
    async fn statuses(&self) -> Vec<TrackStatusType> {
        vec![]
    }
    async fn track_records(&self, ctx: &Context<'_>) -> async_graphql::Result<TrackRecordNodeList> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM track_record WHERE sync_id = ?");
        let rows = sqlx::query_as::<_, TrackRecordRow>(&sql)
            .bind(self.id)
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
    async fn display_score(&self) -> String {
        self.score.to_string()
    }
    async fn manga(&self, ctx: &Context<'_>) -> async_graphql::Result<MangaType> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM manga WHERE id = ?");
        let row = sqlx::query_as::<_, suwayomi_core::schema::MangaRow>(&sql)
            .bind(self.manga_id)
            .fetch_one(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(MangaType::from_row(&row))
    }
    async fn tracker(&self) -> Option<TrackerType> {
        TrackerType::by_id(self.tracker_id, false)
    }
}

/// Mirrors `TrackSearchType` — minimal (search runs against tracker APIs).
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
