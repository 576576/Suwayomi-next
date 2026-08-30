//! Subscription root — mirrors `graphql/subscriptions/*.kt`.
//! Emits one initial snapshot per connection (Phase 6 wires the real
//! broadcast channels for download/update/sync events).

use async_graphql::{Context, InputObject, SimpleObject, Subscription};
use futures::stream::{self, Stream};

use crate::mutation_b4::{
    DownloadUpdateType, DownloadUpdates, LibraryUpdateStatus, UpdateState, WebUIUpdateInfo, WebUIUpdateStatus,
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
    async fn library_update_status_changed(
        &self,
        _ctx: &Context<'_>,
        _input: LibraryUpdateStatusChangedInput,
    ) -> impl Stream<Item = UpdaterUpdates> {
        stream::once(async {
            let idle = LibraryUpdateStatus::idle();
            UpdaterUpdates {
                category_updates: vec![],
                initial: Some(idle),
                jobs_info: crate::mutation_b4::UpdaterJobsInfoType {
                    finished_jobs: 0,
                    is_running: false,
                    skipped_categories_count: 0,
                    skipped_mangas_count: 0,
                    total_jobs: 0,
                },
                manga_updates: vec![],
                omitted_updates: false,
            }
        })
    }

    /// Mirrors `updateStatusChanged` (deprecated).
    async fn update_status_changed(&self, _ctx: &Context<'_>) -> impl Stream<Item = UpdateStatusPayload> {
        stream::once(async { UpdateStatusPayload::idle() })
    }
}

#[allow(dead_code)]
fn _keep(_: DownloadUpdateType) {}
