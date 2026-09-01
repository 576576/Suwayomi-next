//! Mutation batch B4 — Download/Update/Backup/Track/Extension/Sync/User/WebUI
//! mutations. DB-driven parts are fully implemented; manager-dependent parts
//! return Kotlin-compatible defaults until Phase 6 services land.

use async_graphql::{Context, Enum, InputObject, Object, SimpleObject};
use std::collections::HashMap;

use suwayomi_core::schema::TrackRecordRow;
use suwayomi_domain::meta::{MetaService, MetaTable};
use suwayomi_domain::sql::bind_placeholders;

use crate::query::SortOrder;
use crate::scalars::{DurationScalar, LongString};
use crate::settings::{
    AuthMode, CbzMediaType, GraphqlDatabaseType, KoreaderSyncChecksumMethod, KoreaderSyncConflictStrategy,
    WebUIChannel, WebUIFlavor, WebUIInterface,
};
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


/// Mirrors `KoSyncStatusPayload`.
#[derive(SimpleObject)]
pub struct KoSyncStatusPayloadType {
    pub is_logged_in: bool,
    pub server_address: Option<String>,
    pub username: Option<String>,
}

#[derive(SimpleObject)]
pub struct KoSyncConnectPayload {
    pub client_mutation_id: Option<String>,
    pub status: KoSyncStatusPayloadType,
    pub message: Option<String>,
}

#[derive(SimpleObject)]
pub struct LogoutKoSyncAccountPayload {
    pub client_mutation_id: Option<String>,
    pub status: KoSyncStatusPayloadType,
}

#[derive(InputObject)]
pub struct ConnectKoSyncAccountInput {
    pub client_mutation_id: Option<String>,
    pub server_address: String,
    pub username: String,
    pub password: String,
}

#[derive(InputObject)]
pub struct LogoutKoSyncAccountInput {
    pub client_mutation_id: Option<String>,
}

#[derive(InputObject)]
pub struct PushKoSyncProgressInput {
    pub client_mutation_id: Option<String>,
    pub chapter_id: i32,
}

#[derive(SimpleObject)]
pub struct PushKoSyncProgressPayload {
    pub client_mutation_id: Option<String>,
    pub success: bool,
    pub chapter: Option<crate::types::ChapterType>,
}

#[derive(InputObject)]
pub struct PullKoSyncProgressInput {
    pub client_mutation_id: Option<String>,
    pub chapter_id: i32,
}

#[derive(SimpleObject)]
pub struct PullKoSyncProgressPayload {
    pub client_mutation_id: Option<String>,
    pub chapter: Option<crate::types::ChapterType>,
    pub sync_conflict: Option<crate::mutation::SyncConflictInfoType>,
}

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

/// Mirrors `SettingsDownloadConversionHeaderTypeInput` (WebUI r3474).
#[derive(InputObject, Clone, Default)]
pub struct SettingsDownloadConversionHeaderTypeInput {
    pub name: String,
    pub value: String,
}

/// Mirrors `SettingsDownloadConversionTypeInput` (WebUI r3474).
#[derive(InputObject, Clone, Default)]
pub struct SettingsDownloadConversionTypeInput {
    pub call_timeout: Option<DurationScalar>,
    pub compression_level: Option<f64>,
    pub connect_timeout: Option<DurationScalar>,
    pub headers: Option<Vec<SettingsDownloadConversionHeaderTypeInput>>,
    pub mime_type: String,
    pub target: String,
}

/// Mirrors `PartialSettingsTypeInput` — the full mutable settings surface of
/// the upstream WebUI (77 fields), aligned with `graphql-base.types.ts`.
#[derive(InputObject, Default)]
pub struct PartialSettingsTypeInput {
    pub auth_mode: Option<AuthMode>,
    pub auth_password: Option<String>,
    pub auth_username: Option<String>,
    pub auto_backup_include_categories: Option<bool>,
    pub auto_backup_include_chapters: Option<bool>,
    pub auto_backup_include_client_data: Option<bool>,
    pub auto_backup_include_history: Option<bool>,
    pub auto_backup_include_manga: Option<bool>,
    pub auto_backup_include_server_settings: Option<bool>,
    pub auto_backup_include_tracking: Option<bool>,
    pub auto_download_ignore_re_uploads: Option<bool>,
    pub auto_download_new_chapters: Option<bool>,
    pub auto_download_new_chapters_limit: Option<i32>,
    #[graphql(name = "autoBackupFrequency")]
    pub auto_backup_frequency: Option<i32>,
    pub backup_interval: Option<i32>,
    pub backup_path: Option<String>,
    #[graphql(name = "backupTTL")]
    pub backup_ttl: Option<i32>,
    pub backup_time: Option<String>,
    pub database_password: Option<String>,
    pub database_type: Option<GraphqlDatabaseType>,
    pub database_url: Option<String>,
    pub database_username: Option<String>,
    pub debug_logs_enabled: Option<bool>,
    pub download_as_cbz: Option<bool>,
    pub download_conversions: Option<Vec<SettingsDownloadConversionTypeInput>>,
    pub downloads_path: Option<String>,
    pub electron_path: Option<String>,
    pub exclude_completed: Option<bool>,
    pub exclude_entry_with_unread_chapters: Option<bool>,
    pub exclude_not_started: Option<bool>,
    pub exclude_unread_chapters: Option<bool>,
    pub flare_solverr_as_response_fallback: Option<bool>,
    pub flare_solverr_enabled: Option<bool>,
    pub flare_solverr_session_name: Option<String>,
    pub flare_solverr_session_ttl: Option<i32>,
    pub flare_solverr_timeout: Option<i32>,
    pub flare_solverr_url: Option<String>,
    pub global_update_interval: Option<f64>,
    pub initial_open_in_browser_enabled: Option<bool>,
    pub ip: Option<String>,
    pub jwt_audience: Option<String>,
    pub jwt_refresh_expiry: Option<DurationScalar>,
    pub jwt_token_expiry: Option<DurationScalar>,
    pub kcef_enabled: Option<bool>,
    pub koreader_sync_checksum_method: Option<KoreaderSyncChecksumMethod>,
    pub koreader_sync_percentage_tolerance: Option<f64>,
    pub koreader_sync_strategy_backward: Option<KoreaderSyncConflictStrategy>,
    pub koreader_sync_strategy_forward: Option<KoreaderSyncConflictStrategy>,
    pub local_source_path: Option<String>,
    pub max_log_file_size: Option<String>,
    pub max_log_files: Option<i32>,
    pub max_log_folder_size: Option<String>,
    pub max_sources_in_parallel: Option<i32>,
    pub opds_cbz_mimetype: Option<CbzMediaType>,
    pub opds_chapter_sort_order: Option<SortOrder>,
    pub opds_enable_page_read_progress: Option<bool>,
    pub opds_items_per_page: Option<i32>,
    pub opds_mark_as_read_on_download: Option<bool>,
    pub opds_show_only_downloaded_chapters: Option<bool>,
    pub opds_show_only_unread_chapters: Option<bool>,
    pub opds_skip_chapter_metadata_feed: Option<bool>,
    pub opds_use_binary_file_sizes: Option<bool>,
    pub port: Option<i32>,
    pub serve_conversions: Option<Vec<SettingsDownloadConversionTypeInput>>,
    pub socks_proxy_enabled: Option<bool>,
    pub socks_proxy_host: Option<String>,
    pub socks_proxy_password: Option<String>,
    pub socks_proxy_port: Option<String>,
    pub socks_proxy_username: Option<String>,
    pub socks_proxy_version: Option<i32>,
    pub sync_data_categories: Option<bool>,
    pub sync_data_chapters: Option<bool>,
    pub sync_data_history: Option<bool>,
    pub sync_data_manga: Option<bool>,
    pub sync_data_tracking: Option<bool>,
    pub sync_interval: Option<DurationScalar>,
    pub sync_yomi_api_key: Option<String>,
    pub sync_yomi_enabled: Option<bool>,
    pub sync_yomi_host: Option<String>,
    pub system_tray_enabled: Option<bool>,
    pub update_mangas: Option<bool>,
    pub use_hikari_connection_pool: Option<bool>,
    #[graphql(name = "webUIChannel")]
    pub webui_channel: Option<WebUIChannel>,
    #[graphql(name = "webUIFlavor")]
    pub webui_flavor: Option<WebUIFlavor>,
    #[graphql(name = "webUIInterface")]
    pub webui_interface: Option<WebUIInterface>,
    #[graphql(name = "webUIUpdateCheckInterval")]
    pub webui_update_check_interval: Option<f64>,
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
        ctx: &Context<'_>,
        input: StartDownloaderInput,
    ) -> async_graphql::Result<StartDownloaderPayload> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        state.download.start().await;
        Ok(StartDownloaderPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: download_status(state).await?,
        })
    }

    async fn stop_downloader(
        &self,
        ctx: &Context<'_>,
        input: StopDownloaderInput,
    ) -> async_graphql::Result<StopDownloaderPayload> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        state.download.stop().await;
        Ok(StopDownloaderPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: download_status(state).await?,
        })
    }

    async fn clear_downloader(
        &self,
        ctx: &Context<'_>,
        input: ClearDownloaderInput,
    ) -> async_graphql::Result<ClearDownloaderPayload> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        state.download.clear().await;
        Ok(ClearDownloaderPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: download_status(state).await?,
        })
    }

    async fn enqueue_chapter_download(
        &self,
        ctx: &Context<'_>,
        input: EnqueueChapterDownloadInput,
    ) -> async_graphql::Result<EnqueueChapterDownloadPayload> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        state.download.enqueue_chapter(input.id).await.map_err(async_graphql::Error::from)?;
        Ok(EnqueueChapterDownloadPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: download_status(state).await?,
        })
    }

    async fn enqueue_chapter_downloads(
        &self,
        ctx: &Context<'_>,
        input: EnqueueChapterDownloadsInput,
    ) -> async_graphql::Result<EnqueueChapterDownloadsPayload> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        for id in &input.ids {
            let _ = state.download.enqueue_chapter(*id).await;
        }
        Ok(EnqueueChapterDownloadsPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: download_status(state).await?,
        })
    }

    async fn dequeue_chapter_download(
        &self,
        ctx: &Context<'_>,
        input: DequeueChapterDownloadInput,
    ) -> async_graphql::Result<DequeueChapterDownloadPayload> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        let _ = state.download.dequeue_chapter(input.id).await;
        Ok(DequeueChapterDownloadPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: download_status(state).await?,
        })
    }

    async fn dequeue_chapter_downloads(
        &self,
        ctx: &Context<'_>,
        input: DequeueChapterDownloadsInput,
    ) -> async_graphql::Result<DequeueChapterDownloadsPayload> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        for id in &input.ids {
            let _ = state.download.dequeue_chapter(*id).await;
        }
        Ok(DequeueChapterDownloadsPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: download_status(state).await?,
        })
    }

    async fn reorder_chapter_download(
        &self,
        ctx: &Context<'_>,
        input: ReorderChapterDownloadInput,
    ) -> async_graphql::Result<ReorderChapterDownloadPayload> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        state.download.reorder(input.chapter_id, input.to.max(0) as usize).await;
        Ok(ReorderChapterDownloadPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: download_status(state).await?,
        })
    }

    async fn reorder_chapter_downloads(
        &self,
        ctx: &Context<'_>,
        input: ReorderChapterDownloadsInput,
    ) -> async_graphql::Result<ReorderChapterDownloadsPayload> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        for r in &input.reorders {
            state.download.reorder(r.chapter_id, r.to.max(0) as usize).await;
        }
        Ok(ReorderChapterDownloadsPayload {
            client_mutation_id: input.client_mutation_id,
            download_status: download_status(state).await?,
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
        ctx: &Context<'_>,
        input: UpdateLibraryInput,
    ) -> async_graphql::Result<UpdateLibraryPayload> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        // Phase 6: run the real updater in the background; events stream to
        // the `libraryUpdateStatusChanged` subscription.
        state.update.start(input.categories).await;
        let running = state.update.is_running().await;
        Ok(UpdateLibraryPayload {
            client_mutation_id: input.client_mutation_id,
            update_status: LibraryUpdateStatus {
                category_updates: vec![],
                jobs_info: UpdaterJobsInfoType {
                    finished_jobs: 0,
                    is_running: running,
                    skipped_categories_count: 0,
                    skipped_mangas_count: 0,
                    total_jobs: 0,
                },
                manga_updates: vec![],
            },
        })
    }

    async fn update_stop(
        &self,
        ctx: &Context<'_>,
        input: UpdateStopInput,
    ) -> async_graphql::Result<UpdateStopPayload> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        state.update.stop().await;
        Ok(UpdateStopPayload { client_mutation_id: input.client_mutation_id })
    }

    // ---- Backup ----

    async fn create_backup(
        &self,
        ctx: &Context<'_>,
        input: CreateBackupInput,
    ) -> async_graphql::Result<CreateBackupPayload> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        // Phase 6: real export — gzipped Mihon protobuf backup; the client
        // downloads it from the REST export/file endpoint (import pending).
        let _ = input.flags;
        suwayomi_core::backup::create_backup(state.db.pool()).await.map_err(async_graphql::Error::from)?;
        Ok(CreateBackupPayload { client_mutation_id: input.client_mutation_id, url: "/api/v1/backup/export/file".to_string() })
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


    // ---- KOReader sync ----

    /// Mirrors `connectKoSyncAccount`.
    async fn connect_ko_sync_account(
        &self,
        ctx: &Context<'_>,
        input: ConnectKoSyncAccountInput,
    ) -> async_graphql::Result<KoSyncConnectPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let (message, status) = state.koreader.connect(&input.server_address, &input.username, &input.password).await?;
        Ok(KoSyncConnectPayload {
            client_mutation_id: input.client_mutation_id,
            status: KoSyncStatusPayloadType {
                is_logged_in: status.is_logged_in,
                server_address: status.server_address,
                username: status.username,
            },
            message: Some(message),
        })
    }

    /// Mirrors `logoutKoSyncAccount`.
    async fn logout_ko_sync_account(
        &self,
        ctx: &Context<'_>,
        input: LogoutKoSyncAccountInput,
    ) -> async_graphql::Result<LogoutKoSyncAccountPayload> {
        let state = ctx.data::<GraphQLState>()?;
        state.koreader.logout().await?;
        Ok(LogoutKoSyncAccountPayload {
            client_mutation_id: input.client_mutation_id,
            status: KoSyncStatusPayloadType { is_logged_in: false, server_address: None, username: None },
        })
    }

    /// Mirrors `pushKoSyncProgress`.
    async fn push_ko_sync_progress(
        &self,
        ctx: &Context<'_>,
        input: PushKoSyncProgressInput,
    ) -> async_graphql::Result<PushKoSyncProgressPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let _ = state.koreader.push_progress(input.chapter_id).await;
        let chapter = fetch_chapter_row(state, input.chapter_id).await.ok().map(|c| crate::types::ChapterType::from_row(&c));
        Ok(PushKoSyncProgressPayload {
            client_mutation_id: input.client_mutation_id,
            success: true,
            chapter,
        })
    }

    /// Mirrors `pullKoSyncProgress`.
    async fn pull_ko_sync_progress(
        &self,
        ctx: &Context<'_>,
        input: PullKoSyncProgressInput,
    ) -> async_graphql::Result<PullKoSyncProgressPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let result = state.koreader.pull_progress(input.chapter_id).await?;
        let mut sync_conflict = None;
        if let Some(r) = &result {
            if r.is_conflict {
                sync_conflict = Some(crate::mutation::SyncConflictInfoType { device_name: r.device.clone(), remote_page: r.page_read });
            }
            if r.should_update {
                sqlx::query("UPDATE suwayomi.chapter SET last_page_read = $1, last_read_at = $2 WHERE id = $3")
                    .bind(r.page_read)
                    .bind(r.timestamp)
                    .bind(input.chapter_id)
                    .execute(state.db.pool())
                    .await?;
            }
        }
        let chapter = fetch_chapter_row(state, input.chapter_id).await.ok().map(|c| crate::types::ChapterType::from_row(&c));
        Ok(PullKoSyncProgressPayload {
            client_mutation_id: input.client_mutation_id,
            chapter,
            sync_conflict,
        })
    }

    // ---- Extension ----

    /// Mirrors `fetchExtensions` — refreshes the repo indexes, syncs the
    /// sandbox's loaded sources, then lists extensions & stores from DB.
    async fn fetch_extensions(
        &self,
        ctx: &Context<'_>,
        input: FetchExtensionsInput,
    ) -> async_graphql::Result<FetchExtensionsPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let store = state.extension_store.clone();
        // refresh repo indexes (best-effort: a failing repo shouldn't block)
        let _ = store.refresh_stores().await;
        if store.sandbox_available() {
            let _ = store.sync_sources().await;
        }
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
        ctx: &Context<'_>,
        input: UpdateExtensionInput,
    ) -> async_graphql::Result<UpdateExtensionPayload> {
        let state = ctx.data::<GraphQLState>()?;
        apply_extension_patch(state, std::slice::from_ref(&input.id), &input.patch).await?;
        let ext = fetch_extension_by_pkg(state, &input.id).await?;
        Ok(UpdateExtensionPayload { client_mutation_id: input.client_mutation_id, extension: ext.map(|r| crate::types::ExtensionType { row: r }) })
    }

    async fn update_extensions(
        &self,
        ctx: &Context<'_>,
        input: UpdateExtensionsInput,
    ) -> async_graphql::Result<UpdateExtensionsPayload> {
        let state = ctx.data::<GraphQLState>()?;
        apply_extension_patch(state, &input.ids, &input.patch).await?;
        let exts = fetch_extensions_by_pkg(state, &input.ids).await?;
        Ok(UpdateExtensionsPayload {
            client_mutation_id: input.client_mutation_id,
            extensions: exts.into_iter().map(|r| crate::types::ExtensionType { row: r }).collect(),
        })
    }

    async fn install_external_extension(
        &self,
        ctx: &Context<'_>,
        input: InstallExternalExtensionInput,
    ) -> async_graphql::Result<InstallExternalExtensionPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let mut upload = input.extension_file.value(ctx)?;
        let mut bytes = Vec::new();
        use std::io::Read as _;
        upload
            .content
            .read_to_end(&mut bytes)
            .map_err(|e| async_graphql::Error::new(format!("read upload: {e}")))?;
        if bytes.is_empty() {
            return Err(async_graphql::Error::new("empty apk upload"));
        }
        state.extension_store.install_external(&bytes).await.map_err(async_graphql::Error::from)?;
        let meta = state
            .extension_store
            .sync_sources()
            .await
            .map_err(async_graphql::Error::from)?;
        let _ = meta;
        // resolve the freshly installed package (inspect told us the name;
        // simplest: the newest is_installed row without a store link)
        let ext = sqlx::query_as::<_, suwayomi_core::schema::ExtensionRow>(
            "SELECT * FROM suwayomi.extension WHERE is_installed AND apk_url IS NULL ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
        Ok(InstallExternalExtensionPayload {
            client_mutation_id: input.client_mutation_id,
            extension: crate::types::ExtensionType { row: ext.ok_or_else(|| async_graphql::Error::new("extension not registered"))? },
        })
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

    async fn start_sync(&self, ctx: &Context<'_>, input: StartSyncInput) -> async_graphql::Result<StartSyncPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let svc = state.sync_yomi.clone();
        if !svc.enabled() {
            return Ok(StartSyncPayload { client_mutation_id: input.client_mutation_id, result: StartSyncResult::SyncDisabled });
        }
        // Fire-and-forget: the sync cycle runs in the background (matches the
        // Kotlin GlobalScope.launch semantics). A later query can inspect the
        // persisted sync timestamp/ETag for status.
        tokio::spawn(async move {
            if let Err(e) = svc.sync_now().await {
                tracing::warn!("sync_yomi cycle failed: {e}");
            }
        });
        Ok(StartSyncPayload { client_mutation_id: input.client_mutation_id, result: StartSyncResult::Success })
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
        // Persist the submitted (non-None) settings as a JSON blob under the
        // `settings` global_meta key so saves survive restarts. The settings
        // query overlays this blob on top of the env-derived defaults.
        let json = partial_settings_to_json(&input.settings);
        let json_str = serde_json::to_string(&json).unwrap_or_else(|_| "{}".to_string());
        let mut m = HashMap::new();
        m.insert("settings".to_string(), json_str);
        let mut by_ref = HashMap::new();
        by_ref.insert(0i64, m);
        MetaService::new(state.db.clone())
            .modify(MetaTable::Global, &by_ref)
            .await
            .map_err(async_graphql::Error::from)?;
        // Local source path takes effect immediately (no restart needed).
        if let Some(p) = input.settings.local_source_path.clone() {
            suwayomi_domain::source::local::set_local_source_root(Some(std::path::PathBuf::from(p)));
        }
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

/// Builds the GraphQL `DownloadStatus` from the download manager snapshot.
pub(crate) async fn download_status(state: &GraphQLState) -> async_graphql::Result<DownloadStatus> {
    use suwayomi_domain::download::JobState;
    let jobs = state.download.snapshot().await;
    let mut queue = Vec::with_capacity(jobs.len());
    for (i, job) in jobs.iter().enumerate() {
        let chapter = fetch_chapter_row(state, job.chapter_id).await?;
        let manga: suwayomi_core::schema::MangaRow = sqlx::query_as("SELECT * FROM manga WHERE id = $1")
            .bind(job.manga_id)
            .fetch_one(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        queue.push(DownloadType {
            position: i as i32,
            progress: job.progress,
            state: match job.state {
                JobState::Queued => DownloadState::Queued,
                JobState::Downloading => DownloadState::Downloading,
                JobState::Finished => DownloadState::Finished,
                JobState::Error => DownloadState::Error,
            },
            tries: job.tries,
            chapter: crate::types::ChapterType::from_row(&chapter),
            manga: crate::types::MangaType::from_row(&manga),
        });
    }
    Ok(DownloadStatus {
        queue,
        state: if state.download.is_running() { DownloaderState::Started } else { DownloaderState::Stopped },
    })


}

/// Applies an extension patch (install / uninstall / update) for the given
/// pkg names — mirrors Kotlin `ExtensionMutation.updateExtensions`.
async fn apply_extension_patch(
    state: &GraphQLState,
    pkgs: &[String],
    patch: &UpdateExtensionPatchInput,
) -> async_graphql::Result<()> {
    let svc = state.extension_store.clone();
    for pkg in pkgs {
        let row: Option<suwayomi_core::schema::ExtensionRow> =
            sqlx::query_as("SELECT * FROM suwayomi.extension WHERE pkg_name = $1")
                .bind(pkg)
                .fetch_optional(state.db.pool())
                .await
                .map_err(async_graphql::Error::from)?;
        let Some(row) = row else { continue };
        if (patch.install == Some(true) && !row.is_installed)
            || (patch.update == Some(true) && row.has_update)
        {
            svc.install(pkg).await.map_err(async_graphql::Error::from)?;
        } else if patch.uninstall == Some(true) && row.is_installed {
            svc.uninstall(pkg).await.map_err(async_graphql::Error::from)?;
        }
    }
    Ok(())
}

async fn fetch_extension_by_pkg(state: &GraphQLState, pkg: &str) -> async_graphql::Result<Option<suwayomi_core::schema::ExtensionRow>> {
    sqlx::query_as("SELECT * FROM suwayomi.extension WHERE pkg_name = $1")
        .bind(pkg)
        .fetch_optional(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)
}

async fn fetch_extensions_by_pkg(
    state: &GraphQLState,
    pkgs: &[String],
) -> async_graphql::Result<Vec<suwayomi_core::schema::ExtensionRow>> {
    let mut out = Vec::new();
    for p in pkgs {
        if let Some(r) = fetch_extension_by_pkg(state, p).await? {
            out.push(r);
        }
    }
    Ok(out)
}

/// Serializes the submitted (non-None) settings into a JSON object keyed by
/// the upstream camelCase field names, for persistence under global_meta.
fn partial_settings_to_json(s: &PartialSettingsTypeInput) -> serde_json::Value {
    use crate::scalars::format_iso8601_duration;
    use serde_json::{json, Map, Value};

    let mut m = Map::new();
    macro_rules! put {
        ($k:expr, $v:expr) => {
            if let Some(v) = $v {
                m.insert($k.to_string(), json!(v));
            }
        };
    }

    put!("authMode", s.auth_mode.map(|v| match v {
        AuthMode::None => "NONE",
        AuthMode::BasicAuth => "BASIC_AUTH",
        AuthMode::SimpleLogin => "SIMPLE_LOGIN",
        AuthMode::UiLogin => "UI_LOGIN",
    }));
    put!("authPassword", s.auth_password.clone());
    put!("authUsername", s.auth_username.clone());
    put!("autoBackupIncludeCategories", s.auto_backup_include_categories);
    put!("autoBackupIncludeChapters", s.auto_backup_include_chapters);
    put!("autoBackupIncludeClientData", s.auto_backup_include_client_data);
    put!("autoBackupIncludeHistory", s.auto_backup_include_history);
    put!("autoBackupIncludeManga", s.auto_backup_include_manga);
    put!("autoBackupIncludeServerSettings", s.auto_backup_include_server_settings);
    put!("autoBackupIncludeTracking", s.auto_backup_include_tracking);
    put!("autoDownloadIgnoreReUploads", s.auto_download_ignore_re_uploads);
    put!("autoDownloadNewChapters", s.auto_download_new_chapters);
    put!("autoDownloadNewChaptersLimit", s.auto_download_new_chapters_limit);
    put!("autoBackupFrequency", s.auto_backup_frequency);
    put!("backupInterval", s.backup_interval);
    put!("backupPath", s.backup_path.clone());
    put!("backupTTL", s.backup_ttl);
    put!("backupTime", s.backup_time.clone());
    put!("databasePassword", s.database_password.clone());
    put!("databaseType", s.database_type.map(|v| match v {
        GraphqlDatabaseType::H2 => "H2",
        GraphqlDatabaseType::Postgresql => "POSTGRESQL",
    }));
    put!("databaseUrl", s.database_url.clone());
    put!("databaseUsername", s.database_username.clone());
    put!("debugLogsEnabled", s.debug_logs_enabled);
    put!("downloadAsCbz", s.download_as_cbz);
    put!(
        "downloadConversions",
        s.download_conversions.as_ref().map(|cs| cs.iter().map(conversion_to_json).collect::<Vec<_>>())
    );
    put!("downloadsPath", s.downloads_path.clone());
    put!("electronPath", s.electron_path.clone());
    put!("excludeCompleted", s.exclude_completed);
    put!("excludeEntryWithUnreadChapters", s.exclude_entry_with_unread_chapters);
    put!("excludeNotStarted", s.exclude_not_started);
    put!("excludeUnreadChapters", s.exclude_unread_chapters);
    put!("flareSolverrAsResponseFallback", s.flare_solverr_as_response_fallback);
    put!("flareSolverrEnabled", s.flare_solverr_enabled);
    put!("flareSolverrSessionName", s.flare_solverr_session_name.clone());
    put!("flareSolverrSessionTtl", s.flare_solverr_session_ttl);
    put!("flareSolverrTimeout", s.flare_solverr_timeout);
    put!("flareSolverrUrl", s.flare_solverr_url.clone());
    put!("globalUpdateInterval", s.global_update_interval);
    put!("initialOpenInBrowserEnabled", s.initial_open_in_browser_enabled);
    put!("ip", s.ip.clone());
    put!("jwtAudience", s.jwt_audience.clone());
    put!("jwtRefreshExpiry", s.jwt_refresh_expiry.map(|d| format_iso8601_duration(d.0)));
    put!("jwtTokenExpiry", s.jwt_token_expiry.map(|d| format_iso8601_duration(d.0)));
    put!("kcefEnabled", s.kcef_enabled);
    put!("koreaderSyncChecksumMethod", s.koreader_sync_checksum_method.map(|v| match v {
        KoreaderSyncChecksumMethod::Binary => "BINARY",
        KoreaderSyncChecksumMethod::Filename => "FILENAME",
    }));
    put!("koreaderSyncPercentageTolerance", s.koreader_sync_percentage_tolerance);
    put!("koreaderSyncStrategyBackward", s.koreader_sync_strategy_backward.map(|v| match v {
        KoreaderSyncConflictStrategy::Prompt => "PROMPT",
        KoreaderSyncConflictStrategy::KeepLocal => "KEEP_LOCAL",
        KoreaderSyncConflictStrategy::KeepRemote => "KEEP_REMOTE",
        KoreaderSyncConflictStrategy::Disabled => "DISABLED",
    }));
    put!("koreaderSyncStrategyForward", s.koreader_sync_strategy_forward.map(|v| match v {
        KoreaderSyncConflictStrategy::Prompt => "PROMPT",
        KoreaderSyncConflictStrategy::KeepLocal => "KEEP_LOCAL",
        KoreaderSyncConflictStrategy::KeepRemote => "KEEP_REMOTE",
        KoreaderSyncConflictStrategy::Disabled => "DISABLED",
    }));
    put!("localSourcePath", s.local_source_path.clone());
    put!("maxLogFileSize", s.max_log_file_size.clone());
    put!("maxLogFiles", s.max_log_files);
    put!("maxLogFolderSize", s.max_log_folder_size.clone());
    put!("maxSourcesInParallel", s.max_sources_in_parallel);
    put!("opdsCbzMimetype", s.opds_cbz_mimetype.map(|v| match v {
        CbzMediaType::Modern => "MODERN",
        CbzMediaType::Legacy => "LEGACY",
        CbzMediaType::Compatible => "COMPATIBLE",
    }));
    put!("opdsChapterSortOrder", s.opds_chapter_sort_order.map(|v| match v {
        SortOrder::Asc => "ASC",
        SortOrder::Desc => "DESC",
    }));
    put!("opdsEnablePageReadProgress", s.opds_enable_page_read_progress);
    put!("opdsItemsPerPage", s.opds_items_per_page);
    put!("opdsMarkAsReadOnDownload", s.opds_mark_as_read_on_download);
    put!("opdsShowOnlyDownloadedChapters", s.opds_show_only_downloaded_chapters);
    put!("opdsShowOnlyUnreadChapters", s.opds_show_only_unread_chapters);
    put!("opdsSkipChapterMetadataFeed", s.opds_skip_chapter_metadata_feed);
    put!("opdsUseBinaryFileSizes", s.opds_use_binary_file_sizes);
    put!("port", s.port);
    put!(
        "serveConversions",
        s.serve_conversions.as_ref().map(|cs| cs.iter().map(conversion_to_json).collect::<Vec<_>>())
    );
    put!("socksProxyEnabled", s.socks_proxy_enabled);
    put!("socksProxyHost", s.socks_proxy_host.clone());
    put!("socksProxyPassword", s.socks_proxy_password.clone());
    put!("socksProxyPort", s.socks_proxy_port.clone());
    put!("socksProxyUsername", s.socks_proxy_username.clone());
    put!("socksProxyVersion", s.socks_proxy_version);
    put!("syncDataCategories", s.sync_data_categories);
    put!("syncDataChapters", s.sync_data_chapters);
    put!("syncDataHistory", s.sync_data_history);
    put!("syncDataManga", s.sync_data_manga);
    put!("syncDataTracking", s.sync_data_tracking);
    put!("syncInterval", s.sync_interval.map(|d| format_iso8601_duration(d.0)));
    put!("syncYomiApiKey", s.sync_yomi_api_key.clone());
    put!("syncYomiEnabled", s.sync_yomi_enabled);
    put!("syncYomiHost", s.sync_yomi_host.clone());
    put!("systemTrayEnabled", s.system_tray_enabled);
    put!("updateMangas", s.update_mangas);
    put!("useHikariConnectionPool", s.use_hikari_connection_pool);
    put!("webUIChannel", s.webui_channel.map(|v| match v {
        WebUIChannel::Bundled => "BUNDLED",
        WebUIChannel::Stable => "STABLE",
        WebUIChannel::Preview => "PREVIEW",
    }));
    put!("webUIFlavor", s.webui_flavor.map(|v| match v {
        WebUIFlavor::Webui => "WEBUI",
        WebUIFlavor::Vui => "VUI",
        WebUIFlavor::Custom => "CUSTOM",
    }));
    put!("webUIInterface", s.webui_interface.map(|v| match v {
        WebUIInterface::Browser => "BROWSER",
        WebUIInterface::Electron => "ELECTRON",
    }));
    put!("webUIUpdateCheckInterval", s.webui_update_check_interval);

    Value::Object(m)
}

fn conversion_to_json(c: &SettingsDownloadConversionTypeInput) -> serde_json::Value {
    use crate::scalars::format_iso8601_duration;
    use serde_json::{json, Value};

    Value::Object(
        [
            Some(("callTimeout".to_string(), json!(c.call_timeout.map(|d| format_iso8601_duration(d.0))))),
            Some(("compressionLevel".to_string(), json!(c.compression_level))),
            Some(("connectTimeout".to_string(), json!(c.connect_timeout.map(|d| format_iso8601_duration(d.0))))),
            Some((
                "headers".to_string(),
                json!(c.headers.as_ref().map(|hs| hs
                    .iter()
                    .map(|h| json!({ "name": h.name, "value": h.value }))
                    .collect::<Vec<_>>())),
            )),
            Some(("mimeType".to_string(), json!(c.mime_type))),
            Some(("target".to_string(), json!(c.target))),
        ]
        .into_iter()
        .flatten()
        .collect(),
    )
}
