//! Mutation batch B4 — Download/Update/Backup/Track/Extension/Sync/User/WebUI
//! mutations. DB-driven parts are fully implemented; manager-dependent parts
//! return Kotlin-compatible defaults until Phase 6 services land.

use async_graphql::{Context, Enum, InputObject, Object, SimpleObject};

use suwayomi_core::schema::TrackRecordRow;
use suwayomi_domain::sql::bind_placeholders;

use crate::scalars::LongString;
use crate::state::GraphQLState;
use crate::track::{TrackRecordType, TrackerType};
use crate::types::{CategoryType, ChapterType, ExtensionStoreType, ExtensionType, MangaType};

// ---------------------------------------------------------------------------
// Download domain
// ---------------------------------------------------------------------------

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum DownloaderState {
    Started,
    Stopped,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum DownloadState {
    Queued,
    Downloading,
    Finished,
    Error,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum DownloadUpdateType {
    Queued,
    Dequeued,
    Paused,
    Stopped,
    Progress,
    Finished,
    Error,
    Position,
}

#[derive(SimpleObject, Clone)]
pub struct DownloadType {
    pub position: i32,
    pub progress: f64,
    pub state: DownloadState,
    pub tries: i32,
    pub chapter: ChapterType,
    pub manga: MangaType,
}

#[derive(SimpleObject, Clone)]
pub struct DownloadStatus {
    pub queue: Vec<DownloadType>,
    pub state: DownloaderState,
}

impl DownloadStatus {
    pub fn idle() -> Self {
        Self { queue: vec![], state: DownloaderState::Stopped }
    }
}

#[derive(SimpleObject, Clone)]
pub struct DownloadUpdate {
    pub download: DownloadType,
    #[graphql(name = "type")]
    pub r#type: DownloadUpdateType,
}

#[derive(SimpleObject, Clone)]
pub struct DownloadUpdates {
    pub initial: Option<Vec<DownloadType>>,
    pub omitted_updates: bool,
    pub state: DownloaderState,
}

#[derive(InputObject)]
pub struct StartDownloaderInput {
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct StartDownloaderPayload {
    pub client_mutation_id: Option<String>,
    pub download_status: DownloadStatus,
}

#[derive(InputObject)]
pub struct StopDownloaderInput {
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct StopDownloaderPayload {
    pub client_mutation_id: Option<String>,
    pub download_status: DownloadStatus,
}

#[derive(InputObject)]
pub struct ClearDownloaderInput {
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct ClearDownloaderPayload {
    pub client_mutation_id: Option<String>,
    pub download_status: DownloadStatus,
}

#[derive(InputObject)]
pub struct EnqueueChapterDownloadInput {
    pub client_mutation_id: Option<String>,
    pub id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct EnqueueChapterDownloadPayload {
    pub client_mutation_id: Option<String>,
    pub download_status: DownloadStatus,
}

#[derive(InputObject)]
pub struct EnqueueChapterDownloadsInput {
    pub client_mutation_id: Option<String>,
    pub ids: Vec<i32>,
}

#[derive(SimpleObject, Clone)]
pub struct EnqueueChapterDownloadsPayload {
    pub client_mutation_id: Option<String>,
    pub download_status: DownloadStatus,
}

#[derive(InputObject)]
pub struct DequeueChapterDownloadInput {
    pub client_mutation_id: Option<String>,
    pub id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct DequeueChapterDownloadPayload {
    pub client_mutation_id: Option<String>,
    pub download_status: DownloadStatus,
}

#[derive(InputObject)]
pub struct DequeueChapterDownloadsInput {
    pub client_mutation_id: Option<String>,
    pub ids: Vec<i32>,
}

#[derive(SimpleObject, Clone)]
pub struct DequeueChapterDownloadsPayload {
    pub client_mutation_id: Option<String>,
    pub download_status: DownloadStatus,
}

#[derive(InputObject)]
pub struct ChapterDownloadReorderInput {
    pub chapter_id: i32,
    pub to: i32,
}

#[derive(InputObject)]
pub struct ReorderChapterDownloadInput {
    pub chapter_id: i32,
    pub client_mutation_id: Option<String>,
    pub to: i32,
}

#[derive(SimpleObject, Clone)]
pub struct ReorderChapterDownloadPayload {
    pub client_mutation_id: Option<String>,
    pub download_status: DownloadStatus,
}

#[derive(InputObject)]
pub struct ReorderChapterDownloadsInput {
    pub client_mutation_id: Option<String>,
    pub reorders: Vec<ChapterDownloadReorderInput>,
}

#[derive(SimpleObject, Clone)]
pub struct ReorderChapterDownloadsPayload {
    pub client_mutation_id: Option<String>,
    pub download_status: DownloadStatus,
}

#[derive(InputObject)]
pub struct DeleteDownloadedChapterInput {
    pub client_mutation_id: Option<String>,
    pub id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct DeleteDownloadedChapterPayload {
    pub chapters: ChapterType,
    pub client_mutation_id: Option<String>,
}

#[derive(InputObject)]
pub struct DeleteDownloadedChaptersInput {
    pub client_mutation_id: Option<String>,
    pub ids: Vec<i32>,
}

#[derive(SimpleObject, Clone)]
pub struct DeleteDownloadedChaptersPayload {
    pub chapters: Vec<ChapterType>,
    pub client_mutation_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Update domain
// ---------------------------------------------------------------------------

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum MangaJobStatus {
    Pending,
    Running,
    Complete,
    Failed,
    Skipped,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum CategoryJobStatus {
    Updating,
    Skipped,
}

#[derive(SimpleObject, Clone)]
pub struct MangaUpdateType {
    pub status: MangaJobStatus,
    pub manga: MangaType,
}

#[derive(SimpleObject, Clone)]
pub struct CategoryUpdateType {
    pub category: CategoryType,
    pub status: CategoryJobStatus,
}

#[derive(SimpleObject, Clone)]
pub struct UpdaterJobsInfoType {
    pub finished_jobs: i32,
    pub is_running: bool,
    pub skipped_categories_count: i32,
    pub skipped_mangas_count: i32,
    pub total_jobs: i32,
}

#[derive(SimpleObject, Clone)]
pub struct LibraryUpdateStatus {
    pub category_updates: Vec<CategoryUpdateType>,
    pub jobs_info: UpdaterJobsInfoType,
    pub manga_updates: Vec<MangaUpdateType>,
}

impl LibraryUpdateStatus {
    pub fn idle() -> Self {
        Self {
            category_updates: vec![],
            jobs_info: UpdaterJobsInfoType {
                finished_jobs: 0,
                is_running: false,
                skipped_categories_count: 0,
                skipped_mangas_count: 0,
                total_jobs: 0,
            },
            manga_updates: vec![],
        }
    }
}

#[derive(InputObject)]
pub struct UpdateLibraryInput {
    pub categories: Option<Vec<i32>>,
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateLibraryPayload {
    pub client_mutation_id: Option<String>,
    pub update_status: LibraryUpdateStatus,
}

#[derive(InputObject)]
pub struct UpdateStopInput {
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateStopPayload {
    pub client_mutation_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Backup domain
// ---------------------------------------------------------------------------

#[derive(InputObject)]
pub struct PartialBackupFlagsInput {
    pub include_categories: Option<bool>,
    pub include_chapters: Option<bool>,
    pub include_client_data: Option<bool>,
    pub include_history: Option<bool>,
    pub include_manga: Option<bool>,
    pub include_server_settings: Option<bool>,
    pub include_tracking: Option<bool>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum BackupRestoreState {
    Idle,
    Success,
    Failure,
    RestoringCategories,
    RestoringManga,
    RestoringMeta,
    RestoringSettings,
}

#[derive(SimpleObject, Clone)]
pub struct BackupRestoreStatus {
    pub manga_progress: i32,
    pub state: BackupRestoreState,
    pub total_manga: i32,
}

#[derive(InputObject)]
pub struct CreateBackupInput {
    pub client_mutation_id: Option<String>,
    pub flags: Option<PartialBackupFlagsInput>,
}

#[derive(SimpleObject, Clone)]
pub struct CreateBackupPayload {
    pub client_mutation_id: Option<String>,
    pub url: String,
}

#[derive(InputObject)]
pub struct RestoreBackupInput {
    pub backup: async_graphql::Upload,
    pub client_mutation_id: Option<String>,
    pub flags: Option<PartialBackupFlagsInput>,
}

#[derive(SimpleObject, Clone)]
pub struct RestoreBackupPayload {
    pub client_mutation_id: Option<String>,
    pub id: String,
    pub status: Option<BackupRestoreStatus>,
}

#[derive(InputObject)]
pub struct ValidateBackupInput {
    pub backup: async_graphql::Upload,
}

#[derive(SimpleObject, Clone)]
pub struct ValidateBackupSource {
    pub id: LongString,
    pub name: String,
}

#[derive(SimpleObject, Clone)]
pub struct ValidateBackupTracker {
    pub name: String,
}

#[derive(SimpleObject, Clone)]
pub struct ValidateBackupResult {
    pub missing_sources: Vec<ValidateBackupSource>,
    pub missing_trackers: Vec<ValidateBackupTracker>,
}

// ---------------------------------------------------------------------------
// Track domain
// ---------------------------------------------------------------------------

#[derive(InputObject)]
pub struct BindTrackInput {
    pub client_mutation_id: Option<String>,
    pub manga_id: i32,
    pub private: Option<bool>,
    pub remote_id: LongString,
    pub tracker_id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct BindTrackPayload {
    pub client_mutation_id: Option<String>,
    pub track_record: TrackRecordType,
}

#[derive(InputObject)]
pub struct BindTrackRecordInput {
    pub client_mutation_id: Option<String>,
    pub manga_id: i32,
    pub track_record_id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct BindTrackRecordPayload {
    pub client_mutation_id: Option<String>,
    pub track_record: TrackRecordType,
}

#[derive(InputObject)]
pub struct UnbindTrackInput {
    pub client_mutation_id: Option<String>,
    pub delete_remote_track: Option<bool>,
    pub record_id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct UnbindTrackPayload {
    pub client_mutation_id: Option<String>,
    pub track_record: Option<TrackRecordType>,
}

#[derive(InputObject)]
pub struct TrackProgressInput {
    pub client_mutation_id: Option<String>,
    pub manga_id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct TrackProgressPayload {
    pub client_mutation_id: Option<String>,
    pub track_records: Vec<TrackRecordType>,
}

#[derive(InputObject)]
pub struct UpdateTrackInput {
    pub client_mutation_id: Option<String>,
    pub finish_date: Option<LongString>,
    pub last_chapter_read: Option<f64>,
    pub private: Option<bool>,
    pub record_id: i32,
    pub score_string: Option<String>,
    pub start_date: Option<LongString>,
    pub status: Option<i32>,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateTrackPayload {
    pub client_mutation_id: Option<String>,
    pub track_record: Option<TrackRecordType>,
}

#[derive(InputObject)]
pub struct FetchTrackInput {
    pub client_mutation_id: Option<String>,
    pub record_id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct FetchTrackPayload {
    pub client_mutation_id: Option<String>,
    pub track_record: TrackRecordType,
}

#[derive(InputObject)]
pub struct LoginTrackerCredentialsInput {
    pub client_mutation_id: Option<String>,
    pub password: String,
    pub tracker_id: i32,
    pub username: String,
}

#[derive(SimpleObject, Clone)]
pub struct LoginTrackerCredentialsPayload {
    pub client_mutation_id: Option<String>,
    pub is_logged_in: bool,
    pub tracker: TrackerType,
}

#[derive(InputObject)]
pub struct LoginTrackerOAuthInput {
    pub client_mutation_id: Option<String>,
    pub tracker_id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct LoginTrackerOAuthPayload {
    pub client_mutation_id: Option<String>,
    pub is_logged_in: bool,
    pub tracker: TrackerType,
}

#[derive(InputObject)]
pub struct LogoutTrackerInput {
    pub client_mutation_id: Option<String>,
    pub tracker_id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct LogoutTrackerPayload {
    pub client_mutation_id: Option<String>,
    pub is_logged_in: bool,
    pub tracker: TrackerType,
}

// ---------------------------------------------------------------------------
// Extension / Sync / User / WebUI
// ---------------------------------------------------------------------------

#[derive(InputObject)]
pub struct FetchExtensionsInput {
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct FetchExtensionsPayload {
    pub client_mutation_id: Option<String>,
    pub extension_stores: Vec<ExtensionStoreType>,
    pub extensions: Vec<ExtensionType>,
}

#[derive(InputObject)]
pub struct UpdateExtensionPatchInput {
    pub install: Option<bool>,
    pub uninstall: Option<bool>,
    pub update: Option<bool>,
}

#[derive(InputObject)]
pub struct UpdateExtensionInput {
    pub client_mutation_id: Option<String>,
    pub id: String,
    pub patch: UpdateExtensionPatchInput,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateExtensionPayload {
    pub client_mutation_id: Option<String>,
    pub extension: Option<ExtensionType>,
}

#[derive(InputObject)]
pub struct UpdateExtensionsInput {
    pub client_mutation_id: Option<String>,
    pub ids: Vec<String>,
    pub patch: UpdateExtensionPatchInput,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateExtensionsPayload {
    pub client_mutation_id: Option<String>,
    pub extensions: Vec<ExtensionType>,
}

#[derive(InputObject)]
pub struct InstallExternalExtensionInput {
    pub client_mutation_id: Option<String>,
    pub extension_file: async_graphql::Upload,
}

#[derive(SimpleObject, Clone)]
pub struct InstallExternalExtensionPayload {
    pub client_mutation_id: Option<String>,
    pub extension: ExtensionType,
}

#[derive(InputObject)]
pub struct AddExtensionStoreInput {
    pub client_mutation_id: Option<String>,
    pub index_url: String,
}

#[derive(SimpleObject, Clone)]
pub struct AddExtensionStorePayload {
    pub client_mutation_id: Option<String>,
    pub extension_store: ExtensionStoreType,
}

#[derive(InputObject)]
pub struct RemoveExtensionStoreInput {
    pub client_mutation_id: Option<String>,
    pub index_url: String,
}

#[derive(SimpleObject, Clone)]
pub struct RemoveExtensionStorePayload {
    pub client_mutation_id: Option<String>,
    pub extension_store: Option<ExtensionStoreType>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum StartSyncResult {
    Success,
    SyncInProgress,
    SyncDisabled,
}

#[derive(InputObject)]
pub struct StartSyncInput {
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct StartSyncPayload {
    pub client_mutation_id: Option<String>,
    pub result: StartSyncResult,
}

#[derive(InputObject)]
pub struct ClearCachedImagesInput {
    pub cached_pages: Option<bool>,
    pub cached_thumbnails: Option<bool>,
    pub client_mutation_id: Option<String>,
    pub downloaded_thumbnails: Option<bool>,
}

#[derive(SimpleObject, Clone)]
pub struct ClearCachedImagesPayload {
    pub cached_pages: Option<bool>,
    pub cached_thumbnails: Option<bool>,
    pub client_mutation_id: Option<String>,
    pub downloaded_thumbnails: Option<bool>,
}

#[derive(InputObject)]
pub struct ClearCookiesAndCacheInput {
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct ClearCookiesAndCachePayload {
    pub client_mutation_id: Option<String>,
}

#[derive(InputObject)]
pub struct ResetSettingsInput {
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct ResetSettingsPayload {
    pub client_mutation_id: Option<String>,
    pub settings: crate::settings::SettingsType,
}

#[derive(InputObject)]
pub struct SetSettingsInput {
    pub client_mutation_id: Option<String>,
    pub settings: PartialSettingsTypeInput,
}

#[derive(SimpleObject, Clone)]
pub struct SetSettingsPayload {
    pub client_mutation_id: Option<String>,
    pub settings: crate::settings::SettingsType,
}

#[derive(InputObject)]
pub struct LoginInput {
    pub client_mutation_id: Option<String>,
    pub password: String,
    pub username: String,
}

#[derive(SimpleObject, Clone)]
pub struct LoginPayload {
    pub access_token: String,
    pub client_mutation_id: Option<String>,
    pub refresh_token: String,
}

#[derive(InputObject)]
pub struct RefreshTokenInput {
    pub client_mutation_id: Option<String>,
    pub refresh_token: String,
}

#[derive(SimpleObject, Clone)]
pub struct RefreshTokenPayload {
    pub access_token: String,
    pub client_mutation_id: Option<String>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum UpdateState {
    Idle,
    Downloading,
    Finished,
    Error,
}

#[derive(SimpleObject, Clone)]
pub struct WebUIUpdateInfo {
    pub channel: crate::settings::WebUIChannel,
    pub tag: String,
}

#[derive(SimpleObject, Clone)]
pub struct WebUIUpdateStatus {
    pub info: WebUIUpdateInfo,
    pub progress: i32,
    pub state: UpdateState,
}

#[derive(InputObject)]
pub struct WebUIUpdateInput {
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct WebUIUpdatePayload {
    pub client_mutation_id: Option<String>,
    pub update_status: WebUIUpdateStatus,
}

/// Mirrors `PartialSettingsTypeInput` — core mutable settings (Phase 6
/// extends to the full registry).
#[derive(InputObject, Default)]
pub struct PartialSettingsTypeInput {
    pub auth_mode: Option<crate::settings::AuthMode>,
    pub auth_password: Option<String>,
    pub auth_username: Option<String>,
    pub download_as_cbz: Option<bool>,
    pub downloads_path: Option<String>,
    pub port: Option<i32>,
    pub ip: Option<String>,
    pub initial_open_in_browser_enabled: Option<bool>,
}

#[derive(InputObject)]
pub struct ConnectKoSyncAccountInput {
    pub client_mutation_id: Option<String>,
    pub password: String,
    pub server_address: String,
    pub username: String,
}

#[derive(SimpleObject, Clone)]
pub struct KoSyncStatusPayload {
    pub is_logged_in: bool,
    pub server_address: Option<String>,
    pub username: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct KoSyncConnectPayload {
    pub client_mutation_id: Option<String>,
    pub message: Option<String>,
    pub status: KoSyncStatusPayload,
}

#[derive(InputObject)]
pub struct LogoutKoSyncAccountInput {
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct LogoutKoSyncAccountPayload {
    pub client_mutation_id: Option<String>,
    pub status: KoSyncStatusPayload,
}

#[derive(InputObject)]
pub struct PullKoSyncProgressInput {
    pub chapter_id: i32,
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct PullKoSyncProgressPayload {
    pub chapter: Option<ChapterType>,
    pub client_mutation_id: Option<String>,
    pub sync_conflict: Option<crate::mutation::SyncConflictInfoType>,
}

#[derive(InputObject)]
pub struct PushKoSyncProgressInput {
    pub chapter_id: i32,
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct PushKoSyncProgressPayload {
    pub chapter: Option<ChapterType>,
    pub client_mutation_id: Option<String>,
    pub success: bool,
}

#[derive(InputObject)]
pub struct UpdateCategoryMangaInput {
    pub categories: Vec<i32>,
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateCategoryMangaPayload {
    pub client_mutation_id: Option<String>,
    pub update_status: crate::query::UpdateStatusPayload,
}

#[derive(InputObject)]
pub struct UpdateLibraryMangaInput {
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateLibraryMangaPayload {
    pub client_mutation_id: Option<String>,
    pub update_status: crate::query::UpdateStatusPayload,
}

#[derive(InputObject)]
pub struct SourcePreferenceChangeInput {
    pub check_box_state: Option<bool>,
    pub edit_text_state: Option<String>,
    pub list_state: Option<String>,
    pub multi_select_state: Option<Vec<String>>,
    pub position: Option<i32>,
    pub switch_state: Option<bool>,
}

#[derive(InputObject)]
pub struct UpdateSourcePreferenceInput {
    pub change: SourcePreferenceChangeInput,
    pub client_mutation_id: Option<String>,
    pub source: LongString,
}

#[derive(SimpleObject, Clone)]
pub struct UpdateSourcePreferencePayload {
    pub client_mutation_id: Option<String>,
    pub preferences: Vec<crate::types::Preference>,
    pub source: crate::types::SourceType,
}

// ---------------------------------------------------------------------------
// B4 Mutation root
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MutationRootB4;

#[Object]
impl MutationRootB4 {
    // ---- Download ----

    async fn start_downloader(
        &self,
        _ctx: &Context<'_>,
        input: StartDownloaderInput,
    ) -> async_graphql::Result<StartDownloaderPayload> {
        Ok(StartDownloaderPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: DownloadStatus::idle(),
        })
    }

    async fn stop_downloader(
        &self,
        _ctx: &Context<'_>,
        input: StopDownloaderInput,
    ) -> async_graphql::Result<StopDownloaderPayload> {
        Ok(StopDownloaderPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: DownloadStatus::idle(),
        })
    }

    async fn clear_downloader(
        &self,
        _ctx: &Context<'_>,
        input: ClearDownloaderInput,
    ) -> async_graphql::Result<ClearDownloaderPayload> {
        Ok(ClearDownloaderPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: DownloadStatus::idle(),
        })
    }

    async fn enqueue_chapter_download(
        &self,
        _ctx: &Context<'_>,
        input: EnqueueChapterDownloadInput,
    ) -> async_graphql::Result<EnqueueChapterDownloadPayload> {
        Ok(EnqueueChapterDownloadPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: DownloadStatus::idle(),
        })
    }

    async fn enqueue_chapter_downloads(
        &self,
        _ctx: &Context<'_>,
        input: EnqueueChapterDownloadsInput,
    ) -> async_graphql::Result<EnqueueChapterDownloadsPayload> {
        Ok(EnqueueChapterDownloadsPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: DownloadStatus::idle(),
        })
    }

    async fn dequeue_chapter_download(
        &self,
        _ctx: &Context<'_>,
        input: DequeueChapterDownloadInput,
    ) -> async_graphql::Result<DequeueChapterDownloadPayload> {
        Ok(DequeueChapterDownloadPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: DownloadStatus::idle(),
        })
    }

    async fn dequeue_chapter_downloads(
        &self,
        _ctx: &Context<'_>,
        input: DequeueChapterDownloadsInput,
    ) -> async_graphql::Result<DequeueChapterDownloadsPayload> {
        Ok(DequeueChapterDownloadsPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: DownloadStatus::idle(),
        })
    }

    async fn reorder_chapter_download(
        &self,
        _ctx: &Context<'_>,
        input: ReorderChapterDownloadInput,
    ) -> async_graphql::Result<ReorderChapterDownloadPayload> {
        Ok(ReorderChapterDownloadPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: DownloadStatus::idle(),
        })
    }

    async fn reorder_chapter_downloads(
        &self,
        _ctx: &Context<'_>,
        input: ReorderChapterDownloadsInput,
    ) -> async_graphql::Result<ReorderChapterDownloadsPayload> {
        Ok(ReorderChapterDownloadsPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: DownloadStatus::idle(),
        })
    }

    /// Mirrors `deleteDownloadedChapter` — clears the downloaded flag.
    async fn delete_downloaded_chapter(
        &self,
        ctx: &Context<'_>,
        input: DeleteDownloadedChapterInput,
    ) -> async_graphql::Result<DeleteDownloadedChapterPayload> {
        let state = ctx.data::<GraphQLState>()?;
        sqlx::query(bind_placeholders("UPDATE chapter SET is_downloaded = FALSE WHERE id = ?").as_str())
            .bind(input.id)
            .execute(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let chapter = crate::types::ChapterType::from_row(&fetch_chapter_row(state, input.id).await?);
        Ok(DeleteDownloadedChapterPayload { chapters: chapter, client_mutation_id: input.client_mutation_id })
    }

    async fn delete_downloaded_chapters(
        &self,
        ctx: &Context<'_>,
        input: DeleteDownloadedChaptersInput,
    ) -> async_graphql::Result<DeleteDownloadedChaptersPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let mut chapters = Vec::new();
        for id in &input.ids {
            sqlx::query(bind_placeholders("UPDATE chapter SET is_downloaded = FALSE WHERE id = ?").as_str())
                .bind(id)
                .execute(state.db.pool())
                .await
                .map_err(async_graphql::Error::from)?;
            chapters.push(crate::types::ChapterType::from_row(&fetch_chapter_row(state, *id).await?));
        }
        Ok(DeleteDownloadedChaptersPayload { chapters, client_mutation_id: input.client_mutation_id })
    }

    // ---- Update ----

    async fn update_library(
        &self,
        _ctx: &Context<'_>,
        input: UpdateLibraryInput,
    ) -> async_graphql::Result<UpdateLibraryPayload> {
        let _ = input.categories;
        Ok(UpdateLibraryPayload {
            client_mutation_id: input.client_mutation_id,
            update_status: LibraryUpdateStatus::idle(),
        })
    }

    async fn update_stop(
        &self,
        _ctx: &Context<'_>,
        input: UpdateStopInput,
    ) -> async_graphql::Result<UpdateStopPayload> {
        Ok(UpdateStopPayload { client_mutation_id: input.client_mutation_id })
    }

    // ---- Backup ----

    async fn create_backup(
        &self,
        _ctx: &Context<'_>,
        input: CreateBackupInput,
    ) -> async_graphql::Result<CreateBackupPayload> {
        let _ = input.flags;
        Ok(CreateBackupPayload { client_mutation_id: input.client_mutation_id, url: String::new() })
    }

    async fn restore_backup(
        &self,
        _ctx: &Context<'_>,
        input: RestoreBackupInput,
    ) -> async_graphql::Result<RestoreBackupPayload> {
        let _ = (input.backup, input.flags);
        Ok(RestoreBackupPayload {
            client_mutation_id: input.client_mutation_id,
            id: String::new(),
            status: Some(BackupRestoreStatus { manga_progress: 0, state: BackupRestoreState::Idle, total_manga: 0 }),
        })
    }

    // ---- Track ----

    /// Mirrors `bindTrack` — creates a track record binding.
    async fn bind_track(&self, ctx: &Context<'_>, input: BindTrackInput) -> async_graphql::Result<BindTrackPayload> {
        let state = ctx.data::<GraphQLState>()?;
        // upsert track_record for (manga, tracker)
        let existing: Option<i32> = sqlx::query_scalar(
            bind_placeholders("SELECT id FROM track_record WHERE manga_id = ? AND sync_id = ?").as_str(),
        )
        .bind(input.manga_id)
        .bind(input.tracker_id)
        .fetch_optional(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
        let id: i32 = if let Some(id) = existing {
            sqlx::query(bind_placeholders("UPDATE track_record SET remote_id = ?, private = ? WHERE id = ?").as_str())
                .bind(input.remote_id.0)
                .bind(input.private.unwrap_or(false))
                .bind(id)
                .execute(state.db.pool())
                .await
                .map_err(async_graphql::Error::from)?;
            id
        } else {
            sqlx::query_scalar(
                bind_placeholders(
                    "INSERT INTO track_record (manga_id, sync_id, remote_id, title, last_chapter_read, total_chapters, status, score, remote_url, start_date, finish_date, private) VALUES (?, ?, ?, '', 0, 0, 0, 0, '', 0, 0, ?) RETURNING id",
                )
                .as_str(),
            )
            .bind(input.manga_id)
            .bind(input.tracker_id)
            .bind(input.remote_id.0)
            .bind(input.private.unwrap_or(false))
            .fetch_one(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?
        };
        let row = fetch_track_record_row(state, id).await?;
        Ok(BindTrackPayload {
            client_mutation_id: input.client_mutation_id,
            track_record: TrackRecordType::from_row(&row),
        })
    }

    /// Mirrors `bindTrackRecord` — binds an existing track search to a manga.
    async fn bind_track_record(
        &self,
        ctx: &Context<'_>,
        input: BindTrackRecordInput,
    ) -> async_graphql::Result<BindTrackRecordPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM track_record WHERE id = ?");
        let row = sqlx::query_as::<_, TrackRecordRow>(&sql)
            .bind(input.track_record_id)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?
            .ok_or_else(|| async_graphql::Error::new("TrackRecord not found"))?;
        sqlx::query(bind_placeholders("UPDATE track_record SET manga_id = ? WHERE id = ?").as_str())
            .bind(input.manga_id)
            .bind(input.track_record_id)
            .execute(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(BindTrackRecordPayload {
            client_mutation_id: input.client_mutation_id,
            track_record: TrackRecordType::from_row(&row),
        })
    }

    /// Mirrors `unbindTrack` — deletes the local record (Phase 6 adds remote deletion).
    async fn unbind_track(
        &self,
        ctx: &Context<'_>,
        input: UnbindTrackInput,
    ) -> async_graphql::Result<UnbindTrackPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let _ = input.delete_remote_track;
        let sql = bind_placeholders("SELECT * FROM track_record WHERE id = ?");
        let row = sqlx::query_as::<_, TrackRecordRow>(&sql)
            .bind(input.record_id)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        sqlx::query(bind_placeholders("DELETE FROM track_record WHERE id = ?").as_str())
            .bind(input.record_id)
            .execute(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(UnbindTrackPayload {
            client_mutation_id: input.client_mutation_id,
            track_record: row.map(|r| TrackRecordType::from_row(&r)),
        })
    }

    /// Mirrors `trackProgress` — all track records of a manga.
    async fn track_progress(
        &self,
        ctx: &Context<'_>,
        input: TrackProgressInput,
    ) -> async_graphql::Result<TrackProgressPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM track_record WHERE manga_id = ?");
        let rows = sqlx::query_as::<_, TrackRecordRow>(&sql)
            .bind(input.manga_id)
            .fetch_all(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let track_records = rows.iter().map(TrackRecordType::from_row).collect();
        Ok(TrackProgressPayload { client_mutation_id: input.client_mutation_id, track_records })
    }

    /// Mirrors `updateTrack` — updates local track record fields.
    async fn update_track(
        &self,
        ctx: &Context<'_>,
        input: UpdateTrackInput,
    ) -> async_graphql::Result<UpdateTrackPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let score_string = input.score_string.clone();
        let start_date = input.start_date;
        let finish_date = input.finish_date;
        let last_chapter_read = input.last_chapter_read;
        let status = input.status;
        let private = input.private;
        let mut sets: Vec<&str> = Vec::new();
        if last_chapter_read.is_some() {
            sets.push("last_chapter_read = ?");
        }
        if status.is_some() {
            sets.push("status = ?");
        }
        if score_string.is_some() {
            sets.push("score = ?");
        }
        if start_date.is_some() {
            sets.push("start_date = ?");
        }
        if finish_date.is_some() {
            sets.push("finish_date = ?");
        }
        if private.is_some() {
            sets.push("private = ?");
        }
        if !sets.is_empty() {
            let sql = bind_placeholders(&format!("UPDATE track_record SET {} WHERE id = ?", sets.join(", ")));
            let mut q = sqlx::query(sql.as_str());
            if let Some(v) = last_chapter_read {
                q = q.bind(v);
            }
            if let Some(v) = status {
                q = q.bind(v);
            }
            if let Some(v) = score_string {
                if let Ok(f) = v.parse::<f64>() {
                    q = q.bind(f);
                } else {
                    q = q.bind(0.0);
                }
            }
            if let Some(v) = start_date {
                q = q.bind(v.0);
            }
            if let Some(v) = finish_date {
                q = q.bind(v.0);
            }
            if let Some(v) = private {
                q = q.bind(v);
            }
            q.bind(input.record_id).execute(state.db.pool()).await.map_err(async_graphql::Error::from)?;
        }
        let sql = bind_placeholders("SELECT * FROM track_record WHERE id = ?");
        let row = sqlx::query_as::<_, TrackRecordRow>(&sql)
            .bind(input.record_id)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(UpdateTrackPayload {
            client_mutation_id: input.client_mutation_id,
            track_record: row.map(|r| TrackRecordType::from_row(&r)),
        })
    }

    /// Mirrors `fetchTrack` — re-fetch remote track (Phase 6), returns local row.
    async fn fetch_track(&self, ctx: &Context<'_>, input: FetchTrackInput) -> async_graphql::Result<FetchTrackPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let sql = bind_placeholders("SELECT * FROM track_record WHERE id = ?");
        let row = sqlx::query_as::<_, TrackRecordRow>(&sql)
            .bind(input.record_id)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?
            .ok_or_else(|| async_graphql::Error::new("TrackRecord not found"))?;
        Ok(FetchTrackPayload {
            client_mutation_id: input.client_mutation_id,
            track_record: TrackRecordType::from_row(&row),
        })
    }

    /// Mirrors `loginTrackerCredentials` — tracker login (Phase 6).
    async fn login_tracker_credentials(
        &self,
        _ctx: &Context<'_>,
        input: LoginTrackerCredentialsInput,
    ) -> async_graphql::Result<LoginTrackerCredentialsPayload> {
        let tracker = TrackerType::by_id(input.tracker_id, false)
            .ok_or_else(|| async_graphql::Error::new("Tracker not found"))?;
        let _ = (input.username, input.password);
        Ok(LoginTrackerCredentialsPayload {
            client_mutation_id: input.client_mutation_id,
            is_logged_in: false,
            tracker,
        })
    }

    async fn login_tracker_o_auth(
        &self,
        _ctx: &Context<'_>,
        input: LoginTrackerOAuthInput,
    ) -> async_graphql::Result<LoginTrackerOAuthPayload> {
        let tracker = TrackerType::by_id(input.tracker_id, false)
            .ok_or_else(|| async_graphql::Error::new("Tracker not found"))?;
        Ok(LoginTrackerOAuthPayload { client_mutation_id: input.client_mutation_id, is_logged_in: false, tracker })
    }

    async fn logout_tracker(
        &self,
        _ctx: &Context<'_>,
        input: LogoutTrackerInput,
    ) -> async_graphql::Result<LogoutTrackerPayload> {
        let tracker = TrackerType::by_id(input.tracker_id, false)
            .ok_or_else(|| async_graphql::Error::new("Tracker not found"))?;
        Ok(LogoutTrackerPayload { client_mutation_id: input.client_mutation_id, is_logged_in: false, tracker })
    }

    // ---- Extension ----

    /// Mirrors `fetchExtensions` — lists installed extensions & stores from DB.
    async fn fetch_extensions(
        &self,
        ctx: &Context<'_>,
        input: FetchExtensionsInput,
    ) -> async_graphql::Result<FetchExtensionsPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let exts = sqlx::query_as::<_, suwayomi_core::schema::ExtensionRow>("SELECT * FROM extension")
            .fetch_all(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let stores = sqlx::query_as::<_, suwayomi_core::schema::ExtensionStoreRow>("SELECT * FROM extension_store")
            .fetch_all(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(FetchExtensionsPayload {
            client_mutation_id: input.client_mutation_id,
            extension_stores: stores.into_iter().map(ExtensionStoreType::from_row).collect(),
            extensions: exts.into_iter().map(|row| ExtensionType { row }).collect(),
        })
    }

    async fn update_extension(
        &self,
        _ctx: &Context<'_>,
        input: UpdateExtensionInput,
    ) -> async_graphql::Result<UpdateExtensionPayload> {
        let _ = (input.id, input.patch);
        Ok(UpdateExtensionPayload { client_mutation_id: input.client_mutation_id, extension: None })
    }

    async fn update_extensions(
        &self,
        _ctx: &Context<'_>,
        input: UpdateExtensionsInput,
    ) -> async_graphql::Result<UpdateExtensionsPayload> {
        let _ = (input.ids, input.patch);
        Ok(UpdateExtensionsPayload { client_mutation_id: input.client_mutation_id, extensions: vec![] })
    }

    async fn install_external_extension(
        &self,
        _ctx: &Context<'_>,
        input: InstallExternalExtensionInput,
    ) -> async_graphql::Result<InstallExternalExtensionPayload> {
        let _ = input.extension_file;
        Err(async_graphql::Error::new("external extension install requires the JVM sandbox (Phase 5)"))
    }

    /// Mirrors `addExtensionStore` — inserts the store row.
    async fn add_extension_store(
        &self,
        ctx: &Context<'_>,
        input: AddExtensionStoreInput,
    ) -> async_graphql::Result<AddExtensionStorePayload> {
        let state = ctx.data::<GraphQLState>()?;
        let id: i32 = sqlx::query_scalar(
            bind_placeholders(
                "INSERT INTO extension_store (index_url, name, is_legacy, badge_label, contact_website, signing_key) VALUES (?, '', FALSE, '', '', '') ON CONFLICT (index_url) DO UPDATE SET index_url = EXCLUDED.index_url RETURNING id",
            )
            .as_str(),
        )
        .bind(&input.index_url)
        .fetch_one(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
        let row = sqlx::query_as::<_, suwayomi_core::schema::ExtensionStoreRow>(
            bind_placeholders("SELECT * FROM extension_store WHERE id = ?").as_str(),
        )
        .bind(id)
        .fetch_one(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
        Ok(AddExtensionStorePayload {
            client_mutation_id: input.client_mutation_id,
            extension_store: ExtensionStoreType::from_row(row),
        })
    }

    async fn remove_extension_store(
        &self,
        ctx: &Context<'_>,
        input: RemoveExtensionStoreInput,
    ) -> async_graphql::Result<RemoveExtensionStorePayload> {
        let state = ctx.data::<GraphQLState>()?;
        let row = sqlx::query_as::<_, suwayomi_core::schema::ExtensionStoreRow>(
            bind_placeholders("SELECT * FROM extension_store WHERE index_url = ?").as_str(),
        )
        .bind(&input.index_url)
        .fetch_optional(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
        sqlx::query(bind_placeholders("DELETE FROM extension_store WHERE index_url = ?").as_str())
            .bind(&input.index_url)
            .execute(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(RemoveExtensionStorePayload {
            client_mutation_id: input.client_mutation_id,
            extension_store: row.map(ExtensionStoreType::from_row),
        })
    }

    // ---- KoSync (Phase 6 wires account service) ----

    async fn connect_ko_sync_account(
        &self,
        _ctx: &Context<'_>,
        input: ConnectKoSyncAccountInput,
    ) -> async_graphql::Result<KoSyncConnectPayload> {
        let _ = (input.password, input.server_address, input.username);
        Ok(KoSyncConnectPayload {
            client_mutation_id: input.client_mutation_id,
            message: None,
            status: KoSyncStatusPayload { is_logged_in: false, server_address: None, username: None },
        })
    }

    async fn logout_ko_sync_account(
        &self,
        _ctx: &Context<'_>,
        input: LogoutKoSyncAccountInput,
    ) -> async_graphql::Result<LogoutKoSyncAccountPayload> {
        Ok(LogoutKoSyncAccountPayload {
            client_mutation_id: input.client_mutation_id,
            status: KoSyncStatusPayload { is_logged_in: false, server_address: None, username: None },
        })
    }

    async fn pull_ko_sync_progress(
        &self,
        ctx: &Context<'_>,
        input: PullKoSyncProgressInput,
    ) -> async_graphql::Result<PullKoSyncProgressPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let chapter = crate::types::ChapterType::from_row(&fetch_chapter_row(state, input.chapter_id).await?);
        Ok(PullKoSyncProgressPayload {
            chapter: Some(chapter),
            client_mutation_id: input.client_mutation_id,
            sync_conflict: None,
        })
    }

    async fn push_ko_sync_progress(
        &self,
        ctx: &Context<'_>,
        input: PushKoSyncProgressInput,
    ) -> async_graphql::Result<PushKoSyncProgressPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let chapter = crate::types::ChapterType::from_row(&fetch_chapter_row(state, input.chapter_id).await?);
        Ok(PushKoSyncProgressPayload {
            chapter: Some(chapter),
            client_mutation_id: input.client_mutation_id,
            success: false,
        })
    }

    // ---- Update helpers ----

    async fn update_category_manga(
        &self,
        _ctx: &Context<'_>,
        input: UpdateCategoryMangaInput,
    ) -> async_graphql::Result<UpdateCategoryMangaPayload> {
        let _ = input.categories;
        Ok(UpdateCategoryMangaPayload {
            client_mutation_id: input.client_mutation_id,
            update_status: crate::query::UpdateStatusPayload::idle(),
        })
    }

    async fn update_library_manga(
        &self,
        _ctx: &Context<'_>,
        input: UpdateLibraryMangaInput,
    ) -> async_graphql::Result<UpdateLibraryMangaPayload> {
        Ok(UpdateLibraryMangaPayload {
            client_mutation_id: input.client_mutation_id,
            update_status: crate::query::UpdateStatusPayload::idle(),
        })
    }

    async fn update_source_preference(
        &self,
        _ctx: &Context<'_>,
        input: UpdateSourcePreferenceInput,
    ) -> async_graphql::Result<UpdateSourcePreferencePayload> {
        let _ = (input.change, input.source);
        Err(async_graphql::Error::new("source preferences require the extension sandbox (Phase 5)"))
    }

    // ---- Sync / Cache / Settings / User / WebUI ----

    async fn start_sync(&self, _ctx: &Context<'_>, input: StartSyncInput) -> async_graphql::Result<StartSyncPayload> {
        Ok(StartSyncPayload { client_mutation_id: input.client_mutation_id, result: StartSyncResult::SyncDisabled })
    }

    async fn clear_cached_images(
        &self,
        _ctx: &Context<'_>,
        input: ClearCachedImagesInput,
    ) -> async_graphql::Result<ClearCachedImagesPayload> {
        Ok(ClearCachedImagesPayload {
            cached_pages: input.cached_pages,
            cached_thumbnails: input.cached_thumbnails,
            client_mutation_id: input.client_mutation_id,
            downloaded_thumbnails: input.downloaded_thumbnails,
        })
    }

    async fn clear_cookies_and_cache(
        &self,
        _ctx: &Context<'_>,
        input: ClearCookiesAndCacheInput,
    ) -> async_graphql::Result<ClearCookiesAndCachePayload> {
        Ok(ClearCookiesAndCachePayload { client_mutation_id: input.client_mutation_id })
    }

    async fn reset_settings(
        &self,
        ctx: &Context<'_>,
        input: ResetSettingsInput,
    ) -> async_graphql::Result<ResetSettingsPayload> {
        let state = ctx.data::<GraphQLState>()?;
        Ok(ResetSettingsPayload {
            client_mutation_id: input.client_mutation_id,
            settings: crate::settings::SettingsType::from_config(&state.config),
        })
    }

    async fn set_settings(
        &self,
        ctx: &Context<'_>,
        input: SetSettingsInput,
    ) -> async_graphql::Result<SetSettingsPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let _ = input.settings; // Phase 6 wires the mutable settings registry
        Ok(SetSettingsPayload {
            client_mutation_id: input.client_mutation_id,
            settings: crate::settings::SettingsType::from_config(&state.config),
        })
    }

    /// Mirrors `login` — SIMPLE_LOGIN auth (Phase 6 wires JWT issuance).
    async fn login(&self, _ctx: &Context<'_>, input: LoginInput) -> async_graphql::Result<LoginPayload> {
        let _ = (input.username, input.password);
        Ok(LoginPayload {
            access_token: String::new(),
            client_mutation_id: input.client_mutation_id,
            refresh_token: String::new(),
        })
    }

    async fn refresh_token(
        &self,
        _ctx: &Context<'_>,
        input: RefreshTokenInput,
    ) -> async_graphql::Result<RefreshTokenPayload> {
        let _ = input.refresh_token;
        Ok(RefreshTokenPayload { access_token: String::new(), client_mutation_id: input.client_mutation_id })
    }

    #[graphql(name = "updateWebUI")]
    async fn update_web_ui(
        &self,
        _ctx: &Context<'_>,
        input: WebUIUpdateInput,
    ) -> async_graphql::Result<WebUIUpdatePayload> {
        Ok(WebUIUpdatePayload {
            client_mutation_id: input.client_mutation_id,
            update_status: WebUIUpdateStatus {
                info: WebUIUpdateInfo { channel: crate::settings::WebUIChannel::Stable, tag: String::new() },
                progress: 0,
                state: UpdateState::Idle,
            },
        })
    }

    #[graphql(name = "resetWebUIUpdateStatus")]
    async fn reset_web_ui_update_status(&self, _ctx: &Context<'_>) -> async_graphql::Result<WebUIUpdateStatus> {
        Ok(WebUIUpdateStatus {
            info: WebUIUpdateInfo { channel: crate::settings::WebUIChannel::Stable, tag: String::new() },
            progress: 0,
            state: UpdateState::Idle,
        })
    }
}

async fn fetch_chapter_row(state: &GraphQLState, id: i32) -> async_graphql::Result<suwayomi_core::schema::ChapterRow> {
    let sql = bind_placeholders("SELECT * FROM chapter WHERE id = ?");
    sqlx::query_as::<_, suwayomi_core::schema::ChapterRow>(&sql)
        .bind(id)
        .fetch_one(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)
}

async fn fetch_track_record_row(state: &GraphQLState, id: i32) -> async_graphql::Result<TrackRecordRow> {
    let sql = bind_placeholders("SELECT * FROM track_record WHERE id = ?");
    sqlx::query_as::<_, TrackRecordRow>(&sql)
        .bind(id)
        .fetch_one(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)
}
