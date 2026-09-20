//! Track REST endpoints — mirrors `TrackController.kt` + `impl/track/Track.kt`.
//!
//! `/list` 与 `/search` 会把追踪器的真实登录态与搜索结果带出来；`login` / `logout`
//! / `bind` / `update` 都会打到站点 API。未知 tracker id 一律 404（上游
//! `TrackerManager.getTracker(id)!!` 抛 NPE → 404）。
//!
//! `/track/{id}/thumbnail` 读编在二进制里的 PNG（对齐上游从 classpath 读
//! `/static/tracker/*.png`），不再联网抓图，响应带 `cache-control: max-age=86400`。

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use suwayomi_domain::tracker::{TrackSearch, TrackUpdate};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Mirrors `TrackerDataClass`.
#[derive(serde::Serialize)]
pub struct TrackerDataClass {
    pub id: i32,
    pub name: String,
    pub icon: String,
    #[serde(rename = "isLogin")]
    pub is_login: bool,
    #[serde(rename = "authUrl")]
    pub auth_url: Option<String>,
}

/// 上游 `Track.getTrackerList()`：`authUrl` 只在未登录时给。
async fn list(State(s): State<AppState>) -> Json<Vec<TrackerDataClass>> {
    let trackers = s
        .tracker
        .list()
        .await
        .into_iter()
        .map(|t| TrackerDataClass {
            id: t.id,
            name: t.name,
            icon: format!("/api/v1/track/{}/thumbnail", t.id),
            is_login: t.is_login,
            auth_url: t.auth_url,
        })
        .collect();
    Json(trackers)
}

/// Mirrors `Track.LoginInput`.
#[derive(Deserialize)]
pub struct LoginInput {
    #[serde(rename = "trackerId")]
    pub tracker_id: i32,
    #[serde(rename = "callbackUrl", default)]
    pub callback_url: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

async fn login(State(s): State<AppState>, Json(input): Json<LoginInput>) -> ApiResult<StatusCode> {
    // 未知 tracker 先拦下来，否则站点调用会以「未登录」之类的形式报出来。
    s.tracker.get(input.tracker_id)?;
    s.tracker
        .login(
            input.tracker_id,
            input.callback_url.as_deref(),
            input.username.as_deref().unwrap_or_default(),
            input.password.as_deref().unwrap_or_default(),
        )
        .await?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct LogoutInput {
    #[serde(rename = "trackerId")]
    pub tracker_id: i32,
}

async fn logout(State(s): State<AppState>, Json(input): Json<LogoutInput>) -> ApiResult<StatusCode> {
    s.tracker.logout(input.tracker_id).await?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct SearchInput {
    #[serde(rename = "trackerId")]
    pub tracker_id: i32,
    pub title: String,
}

async fn search(State(s): State<AppState>, Json(input): Json<SearchInput>) -> ApiResult<Json<Vec<TrackSearch>>> {
    Ok(Json(s.tracker.search(input.tracker_id, &input.title).await?))
}

#[derive(Deserialize)]
pub struct BindParams {
    #[serde(rename = "mangaId")]
    pub manga_id: i32,
    #[serde(rename = "trackerId")]
    pub tracker_id: i32,
    #[serde(rename = "remoteId")]
    pub remote_id: String,
    #[serde(default)]
    pub private: bool,
}

async fn bind(State(s): State<AppState>, axum::extract::Query(q): axum::extract::Query<BindParams>) -> ApiResult<StatusCode> {
    let remote_id: i64 = q
        .remote_id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("remoteId 「{}」不是数字", q.remote_id)))?;
    s.tracker.bind(q.manga_id, q.tracker_id, remote_id, q.private).await?;
    Ok(StatusCode::OK)
}

/// Mirrors `Track.UpdateInput`.
#[derive(Deserialize)]
pub struct UpdateInput {
    #[serde(rename = "recordId")]
    pub record_id: i32,
    #[serde(default)]
    pub status: Option<i32>,
    #[serde(rename = "lastChapterRead", default)]
    pub last_chapter_read: Option<f64>,
    #[serde(rename = "scoreString", default)]
    pub score_string: Option<String>,
    #[serde(rename = "startDate", default)]
    pub start_date: Option<i64>,
    #[serde(rename = "finishDate", default)]
    pub finish_date: Option<i64>,
    #[serde(default)]
    pub unbind: Option<bool>,
    #[serde(default)]
    pub private: Option<bool>,
}

async fn update(State(s): State<AppState>, Json(input): Json<UpdateInput>) -> ApiResult<StatusCode> {
    s.tracker
        .update(TrackUpdate {
            record_id: input.record_id,
            status: input.status,
            last_chapter_read: input.last_chapter_read,
            score_string: input.score_string,
            start_date: input.start_date,
            finish_date: input.finish_date,
            unbind: input.unbind,
            private: input.private,
        })
        .await?;
    Ok(StatusCode::OK)
}

/// 上游 `Track.getTrackerThumbnail`：资源里的 PNG + 一天缓存。
async fn thumbnail(State(s): State<AppState>, Path(tracker_id): Path<i32>) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;

    let tracker = s.tracker.get(tracker_id)?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "max-age=86400"),
        ],
        tracker.logo(),
    )
        .into_response())
}

pub fn track_router() -> Router<AppState> {
    Router::new()
        .route("/list", get(list))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/search", post(search))
        .route("/bind", post(bind))
        .route("/update", post(update))
        .route("/{trackerId}/thumbnail", get(thumbnail))
}
