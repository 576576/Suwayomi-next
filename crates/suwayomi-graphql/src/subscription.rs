//! Subscription root — mirrors `graphql/subscriptions/*.kt`.
//! Emits one initial snapshot per connection (Phase 6 wires the real
//! broadcast channels for download/update/sync events).

use async_graphql::{Context, InputObject, SimpleObject, Subscription};
use futures::stream::{self, Stream};
use futures::StreamExt;

use crate::mutation_b4::{
    DownloadUpdates, LibraryUpdateStatus, UpdateState, WebUIUpdateInfo, WebUIUpdateStatus,
};
use crate::query::UpdateStatusPayload;
use crate::settings::WebUIChannel;

#[derive(SimpleObject, Clone)]
pub struct UpdaterUpdates {
    pub category_updates: Vec<crate::mutation_b4::CategoryUpdateType>,
    pub initial: Option<LibraryUpdateStatus>,
    pub jobs_info: crate::mutation_b4::UpdaterJobsInfoType,
    pub manga_updates: Vec<crate::mutation_b4::MangaUpdateType>,
    pub omitted_updates: bool,
}

#[derive(InputObject)]
pub struct DownloadChangedInput {
    pub max_updates: Option<i32>,
}

#[derive(InputObject)]
pub struct LibraryUpdateStatusChangedInput {
    pub max_updates: Option<i32>,
}

#[derive(Default)]
pub struct SubscriptionRoot;

#[Subscription(name = "Subscription")]
impl SubscriptionRoot {
    /// Mirrors `downloadChanged` (deprecated).
    async fn download_changed(&self, _ctx: &Context<'_>) -> impl Stream<Item = crate::mutation_b4::DownloadStatus> {
        stream::once(async { crate::mutation_b4::DownloadStatus::idle() })
    }

    /// Mirrors `downloadStatusChanged(input:)`.
    async fn download_status_changed(
        &self,
        _ctx: &Context<'_>,
        _input: DownloadChangedInput,
    ) -> impl Stream<Item = DownloadUpdates> {
        stream::once(async {
            DownloadUpdates {
                initial: Some(vec![]),
                omitted_updates: false,
                state: crate::mutation_b4::DownloaderState::Stopped,
            }
        })
    }

    /// Mirrors `webUIUpdateStatusChange`.
    #[graphql(name = "webUIUpdateStatusChange")]
    async fn web_ui_update_status_change(&self, _ctx: &Context<'_>) -> impl Stream<Item = WebUIUpdateStatus> {
        stream::once(async {
            WebUIUpdateStatus {
                info: WebUIUpdateInfo { channel: WebUIChannel::Stable, tag: String::new() },
                progress: 0,
                state: UpdateState::Idle,
            }
        })
    }

    /// Mirrors `syncStatusChanged`.
    async fn sync_status_changed(&self, _ctx: &Context<'_>) -> impl Stream<Item = crate::query::SyncStatus> {
        stream::once(async {
            crate::query::SyncStatus {
                backup_restore_id: None,
                end_date: None,
                error_message: None,
                start_date: crate::scalars::LongString(0),
                state: crate::query::SyncState::Success,
            }
        })
    }

    /// Mirrors `libraryUpdateStatusChanged(input:)`.
    /// Streams live `LibraryUpdateStatus` snapshots from the updater's
    /// broadcast channel (real events once `updateLibrary` starts a job).
    async fn library_update_status_changed(
        &self,
        ctx: &Context<'_>,
        _input: LibraryUpdateStatusChangedInput,
    ) -> impl Stream<Item = UpdaterUpdates> {
        let state = match ctx.data::<crate::state::GraphQLState>() {
            Ok(s) => s.clone(),
            Err(_) => {
                return futures::stream::empty().boxed();
            }
        };
        let rx = state.update.subscribe();
        futures::stream::unfold(rx, |mut rx| async move {
            let status = loop {
                match rx.recv().await {
                    Ok(s) => break s,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            };
            Some((
                UpdaterUpdates {
                    category_updates: status.category_updates,
                    initial: None,
                    jobs_info: status.jobs_info,
                    manga_updates: status.manga_updates,
                    omitted_updates: false,
                },
                rx,
            ))
        })
        .boxed()
    }

    /// Mirrors `updateStatusChanged` (deprecated).
    async fn update_status_changed(&self, _ctx: &Context<'_>) -> impl Stream<Item = UpdateStatusPayload> {
        stream::once(async { UpdateStatusPayload::idle() })
    }
}
