//! Mutation batch B4 — Download/Update/Backup/Track/Extension/Sync/User/WebUI
//! mutations.

use async_graphql::{Context, Enum, InputObject, Object, SimpleObject};
use std::collections::HashMap;

use suwayomi_core::schema::TrackRecordRow;
use suwayomi_domain::meta::{MetaService, MetaTable};
use suwayomi_domain::sql::bind_placeholders;
use suwayomi_domain::tracker::TrackUpdate;

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

impl From<suwayomi_domain::updater::MangaJobStatus> for MangaJobStatus {
    fn from(s: suwayomi_domain::updater::MangaJobStatus) -> Self {
        use suwayomi_domain::updater::MangaJobStatus as D;
        match s {
            D::Pending => Self::Pending,
            D::Running => Self::Running,
            D::Complete => Self::Complete,
            D::Failed => Self::Failed,
            D::Skipped => Self::Skipped,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum CategoryJobStatus {
    Updating,
    Skipped,
}

impl From<suwayomi_domain::updater::CategoryJobStatus> for CategoryJobStatus {
    fn from(s: suwayomi_domain::updater::CategoryJobStatus) -> Self {
        use suwayomi_domain::updater::CategoryJobStatus as D;
        match s {
            D::Updating => Self::Updating,
            D::Skipped => Self::Skipped,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct MangaUpdateType {
    pub status: MangaJobStatus,
    pub manga: MangaType,
}

impl From<suwayomi_domain::updater::MangaUpdate> for MangaUpdateType {
    fn from(u: suwayomi_domain::updater::MangaUpdate) -> Self {
        Self { status: u.status.into(), manga: MangaType::from_row(&u.manga) }
    }
}

#[derive(SimpleObject, Clone)]
pub struct CategoryUpdateType {
    pub category: CategoryType,
    pub status: CategoryJobStatus,
}

impl From<suwayomi_domain::updater::CategoryUpdate> for CategoryUpdateType {
    fn from(u: suwayomi_domain::updater::CategoryUpdate) -> Self {
        Self { category: CategoryType::from(&u.category), status: u.status.into() }
    }
}

#[derive(SimpleObject, Clone)]
pub struct UpdaterJobsInfoType {
    pub finished_jobs: i32,
    pub is_running: bool,
    pub skipped_categories_count: i32,
    pub skipped_mangas_count: i32,
    pub total_jobs: i32,
}

impl From<suwayomi_domain::updater::UpdaterJobsInfo> for UpdaterJobsInfoType {
    fn from(j: suwayomi_domain::updater::UpdaterJobsInfo) -> Self {
        Self {
            finished_jobs: j.finished_jobs,
            is_running: j.is_running,
            skipped_categories_count: j.skipped_categories_count,
            skipped_mangas_count: j.skipped_mangas_count,
            total_jobs: j.total_jobs,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct LibraryUpdateStatus {
    pub category_updates: Vec<CategoryUpdateType>,
    pub jobs_info: UpdaterJobsInfoType,
    pub manga_updates: Vec<MangaUpdateType>,
}

impl From<suwayomi_domain::updater::LibraryUpdateStatus> for LibraryUpdateStatus {
    fn from(s: suwayomi_domain::updater::LibraryUpdateStatus) -> Self {
        Self {
            category_updates: s.category_updates.into_iter().map(Into::into).collect(),
            jobs_info: s.jobs_info.into(),
            manga_updates: s.manga_updates.into_iter().map(Into::into).collect(),
        }
    }
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
// 上游类型名就是 `KoSyncStatusPayload`（同文件里的 KoSyncConnectPayload /
// LogoutKoSyncAccountPayload / ConnectKoSyncAccountInput 都照抄了上游名），
// 这里多出来的 `Type` 后缀会让 WebUI 的 `fragment … on KoSyncStatusPayload`
// 直接校验失败（Unknown type）—— 用显式重命名对齐，不动 Rust 标识符。
#[graphql(name = "KoSyncStatusPayload")]
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
    #[graphql(deprecation = "Replaced with \"unbindTrack\" mutation")]
    pub unbind: Option<bool>,
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
    pub callback_url: String,
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

#[derive(InputObject)]
pub struct RefreshTrackerUserInput {
    pub client_mutation_id: Option<String>,
    pub tracker_id: i32,
}

#[derive(SimpleObject, Clone)]
pub struct RefreshTrackerUserPayload {
    pub client_mutation_id: Option<String>,
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

/// 「重建下载索引」：强制用磁盘上的 `<数据目录>/downloads/**` 重新对账数据库。
///
/// 与 `reconcile_downloads` 同一套逻辑（启动时也会跑一次），差别只是这里由用户
/// 手动触发 —— 手工往下载目录里丢了 CBZ、或换了存储位置之后，不用重启就能重新
/// 扫出来。没有开关参数：它只读磁盘、只补/修下载标记，不删用户文件。
#[derive(InputObject)]
pub struct RebuildDownloadIndexInput {
    pub client_mutation_id: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct RebuildDownloadIndexPayload {
    pub client_mutation_id: Option<String>,
    /// 本次扫描到的章节归档数（含此前已经索引过的）。
    pub chapters: i32,
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
    pub data_dir: Option<String>,
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

/// 把 WebUI 的变更输入归一成沙盒要的**一个**字符串值。
///
/// 具体怎么解释它由沙盒按该项自己的默认值类型决定（`Boolean` 走 `toBoolean`、
/// `Set<String>` 是 JSON 数组），这里只负责挑出前端填的那一个字段。
fn preference_value(change: &SourcePreferenceChangeInput) -> async_graphql::Result<String> {
    if let Some(v) = change.check_box_state {
        return Ok(v.to_string());
    }
    if let Some(v) = change.switch_state {
        return Ok(v.to_string());
    }
    if let Some(v) = &change.edit_text_state {
        return Ok(v.clone());
    }
    if let Some(v) = &change.list_state {
        return Ok(v.clone());
    }
    if let Some(v) = &change.multi_select_state {
        return serde_json::to_string(v).map_err(|e| async_graphql::Error::new(e.to_string()));
    }
    Err(async_graphql::Error::new("no preference value in change input"))
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
        suwayomi_db::query(bind_placeholders("UPDATE chapter SET is_downloaded = FALSE WHERE id = ?").as_str())
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
            suwayomi_db::query(bind_placeholders("UPDATE chapter SET is_downloaded = FALSE WHERE id = ?").as_str())
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
        // Run the updater in the background; events stream to the
        // `libraryUpdateStatusChanged` subscription.
        state.update.start(input.categories).await;
        let running = state.update.is_running().await;
        let mut update_status = LibraryUpdateStatus::idle();
        update_status.jobs_info.is_running = running;
        Ok(UpdateLibraryPayload { client_mutation_id: input.client_mutation_id, update_status })
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
        // Gzipped Mihon protobuf backup; the client downloads it from the
        // REST export/file endpoint.
        let flags = suwayomi_core::backup::BackupFlags::from_partial(&backup_flags(input.flags.as_ref()));
        suwayomi_core::backup::create_backup(state.db.pool(), flags).await.map_err(async_graphql::Error::from)?;
        Ok(CreateBackupPayload { client_mutation_id: input.client_mutation_id, url: "/api/v1/backup/export/file".to_string() })
    }

    async fn restore_backup(
        &self,
        ctx: &Context<'_>,
        input: RestoreBackupInput,
    ) -> async_graphql::Result<RestoreBackupPayload> {
        let state = ctx.data::<crate::state::GraphQLState>()?;
        // Read the uploaded .tachibk payload and restore it (upserts manga/
        // chapters/categories idempotently; sources are matched by name).
        let mut upload = input.backup.value(ctx)?;
        let mut bytes = Vec::new();
        use std::io::Read as _;
        upload
            .content
            .read_to_end(&mut bytes)
            .map_err(|e| async_graphql::Error::new(format!("read upload: {e}")))?;

        let id = format!("restore-{}", chrono::Utc::now().timestamp_millis());
        let flags = suwayomi_core::backup::BackupFlags::from_partial(&backup_flags(input.flags.as_ref()));
        let final_status = match suwayomi_core::backup::restore_backup(state.db.pool(), &bytes, flags).await {
            Ok(summary) => {
                if !summary.errors.is_empty() {
                    tracing::warn!(errors = ?summary.errors, "backup restore completed with errors");
                }
                let manga_total = summary.restored_manga as i32;
                BackupRestoreStatus {
                    manga_progress: manga_total,
                    state: BackupRestoreState::Success,
                    total_manga: manga_total,
                }
            }
            Err(e) => {
                tracing::error!(%e, "backup restore failed");
                BackupRestoreStatus {
                    manga_progress: 0,
                    state: BackupRestoreState::Failure,
                    total_manga: 0,
                }
            }
        };
        state.set_backup_restore_status(&id, final_status.clone()).await;
        Ok(RestoreBackupPayload {
            client_mutation_id: input.client_mutation_id,
            id,
            status: Some(final_status),
        })
    }

    // ---- Track ----

    /// Mirrors `bindTrack`.
    async fn bind_track(&self, ctx: &Context<'_>, input: BindTrackInput) -> async_graphql::Result<BindTrackPayload> {
        let state = ctx.data::<GraphQLState>()?;
        state
            .tracker
            .bind(input.manga_id, input.tracker_id, input.remote_id.0, input.private.unwrap_or(false))
            .await?;
        let row = fetch_track_record_row_for(state, input.manga_id, input.tracker_id).await?;
        Ok(BindTrackPayload {
            client_mutation_id: input.client_mutation_id,
            track_record: TrackRecordType::from_row(&row),
        })
    }

    /// Mirrors `bindTrackRecord` — 返回并进后的那一行（目标漫画原本已有记录时
    /// 是目标行，不是入参的那一行）。
    async fn bind_track_record(
        &self,
        ctx: &Context<'_>,
        input: BindTrackRecordInput,
    ) -> async_graphql::Result<BindTrackRecordPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let id = state.tracker.bind_track_record(input.manga_id, input.track_record_id).await?;
        let row = fetch_track_record_row(state, id).await?;
        Ok(BindTrackRecordPayload {
            client_mutation_id: input.client_mutation_id,
            track_record: TrackRecordType::from_row(&row),
        })
    }

    /// Mirrors `unbindTrack` — 本地行总是删；`deleteRemoteTrack` 只在站点支持删除
    /// 时才会连带删掉站点上的记录。删除后回读，所以 `trackRecord` 恒为 null。
    async fn unbind_track(
        &self,
        ctx: &Context<'_>,
        input: UnbindTrackInput,
    ) -> async_graphql::Result<UnbindTrackPayload> {
        let state = ctx.data::<GraphQLState>()?;
        state.tracker.unbind(input.record_id, input.delete_remote_track.unwrap_or(false)).await?;
        let sql = bind_placeholders("SELECT * FROM track_record WHERE id = ?");
        let row = suwayomi_db::query_as::<TrackRecordRow>(&sql)
            .bind(input.record_id)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(UnbindTrackPayload {
            client_mutation_id: input.client_mutation_id,
            track_record: row.map(|r| TrackRecordType::from_row(&r)),
        })
    }

    /// Mirrors `trackProgress` — 先把当前阅读进度推给站点，再返回该漫画的全部记录。
    async fn track_progress(
        &self,
        ctx: &Context<'_>,
        input: TrackProgressInput,
    ) -> async_graphql::Result<TrackProgressPayload> {
        let state = ctx.data::<GraphQLState>()?;
        state.tracker.track_chapter(input.manga_id).await?;
        let sql = bind_placeholders("SELECT * FROM track_record WHERE manga_id = ?");
        let rows = suwayomi_db::query_as::<TrackRecordRow>(&sql)
            .bind(input.manga_id)
            .fetch_all(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let track_records = rows.iter().map(TrackRecordType::from_row).collect();
        Ok(TrackProgressPayload { client_mutation_id: input.client_mutation_id, track_records })
    }

    /// Mirrors `updateTrack` — 由 `domain::tracker` 负责状态/进度的连带推导，再推给站点。
    async fn update_track(
        &self,
        ctx: &Context<'_>,
        input: UpdateTrackInput,
    ) -> async_graphql::Result<UpdateTrackPayload> {
        let state = ctx.data::<GraphQLState>()?;
        state
            .tracker
            .update(TrackUpdate {
                record_id: input.record_id,
                status: input.status,
                last_chapter_read: input.last_chapter_read,
                score_string: input.score_string,
                start_date: input.start_date.map(|v| v.0),
                finish_date: input.finish_date.map(|v| v.0),
                unbind: input.unbind,
                private: input.private,
            })
            .await?;
        let sql = bind_placeholders("SELECT * FROM track_record WHERE id = ?");
        let row = suwayomi_db::query_as::<TrackRecordRow>(&sql)
            .bind(input.record_id)
            .fetch_optional(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        Ok(UpdateTrackPayload {
            client_mutation_id: input.client_mutation_id,
            track_record: row.map(|r| TrackRecordType::from_row(&r)),
        })
    }

    /// Mirrors `fetchTrack` — 先拉站点上的最新状态，再回读本地行。
    async fn fetch_track(&self, ctx: &Context<'_>, input: FetchTrackInput) -> async_graphql::Result<FetchTrackPayload> {
        let state = ctx.data::<GraphQLState>()?;
        state.tracker.refresh(input.record_id).await?;
        let row = fetch_track_record_row(state, input.record_id).await?;
        Ok(FetchTrackPayload {
            client_mutation_id: input.client_mutation_id,
            track_record: TrackRecordType::from_row(&row),
        })
    }

    /// Mirrors `loginTrackerCredentials`.
    async fn login_tracker_credentials(
        &self,
        ctx: &Context<'_>,
        input: LoginTrackerCredentialsInput,
    ) -> async_graphql::Result<LoginTrackerCredentialsPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let service =
            state.tracker.find(input.tracker_id).ok_or_else(|| async_graphql::Error::new("Could not find tracker"))?;
        service.login_impl(&input.username, &input.password).await?;
        let is_logged_in = service.is_logged_in().await?;
        Ok(LoginTrackerCredentialsPayload {
            client_mutation_id: input.client_mutation_id,
            is_logged_in,
            tracker: TrackerType::from_service(service),
        })
    }

    /// Mirrors `loginTrackerOAuth` — `callbackUrl` 即浏览器回调地址，用户名密码不参与。
    async fn login_tracker_o_auth(
        &self,
        ctx: &Context<'_>,
        input: LoginTrackerOAuthInput,
    ) -> async_graphql::Result<LoginTrackerOAuthPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let service =
            state.tracker.find(input.tracker_id).ok_or_else(|| async_graphql::Error::new("Could not find tracker"))?;
        service.auth_callback(&input.callback_url).await?;
        let is_logged_in = service.is_logged_in().await?;
        Ok(LoginTrackerOAuthPayload {
            client_mutation_id: input.client_mutation_id,
            is_logged_in,
            tracker: TrackerType::from_service(service),
        })
    }

    /// Mirrors `logoutTracker` — 未登录时报错，不做静默成功。
    async fn logout_tracker(
        &self,
        ctx: &Context<'_>,
        input: LogoutTrackerInput,
    ) -> async_graphql::Result<LogoutTrackerPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let service =
            state.tracker.find(input.tracker_id).ok_or_else(|| async_graphql::Error::new("Could not find tracker"))?;
        if !service.is_logged_in().await? {
            return Err(async_graphql::Error::new("Cannot logout of a tracker that is not logged-in"));
        }
        service.logout().await?;
        let is_logged_in = service.is_logged_in().await?;
        Ok(LogoutTrackerPayload {
            client_mutation_id: input.client_mutation_id,
            is_logged_in,
            tracker: TrackerType::from_service(service),
        })
    }

    /// Mirrors Mihon `BaseTracker.refreshUser()` —— 重新拉站点上的用户级设置（评分制）
    /// 并落库，`tracker.scores` 随之更新。上游 Suwayomi 没有对应 mutation。
    async fn refresh_tracker_user(
        &self,
        ctx: &Context<'_>,
        input: RefreshTrackerUserInput,
    ) -> async_graphql::Result<RefreshTrackerUserPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let service =
            state.tracker.find(input.tracker_id).ok_or_else(|| async_graphql::Error::new("Could not find tracker"))?;
        service.refresh_user().await?;
        Ok(RefreshTrackerUserPayload {
            client_mutation_id: input.client_mutation_id,
            tracker: TrackerType::from_service(service),
        })
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
                suwayomi_db::query("UPDATE suwayomi.chapter SET last_page_read = $1, last_read_at = $2 WHERE id = $3")
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
        if store.sandbox_available()
            && let Err(e) = store.sync_sources().await
        {
            tracing::warn!("source sync after refresh failed: {e}");
        }
        let exts = suwayomi_db::query_as::<suwayomi_core::schema::ExtensionRow>("SELECT * FROM extension")
            .fetch_all(state.db.pool())
            .await
            .map_err(async_graphql::Error::from)?;
        let stores = suwayomi_db::query_as::<suwayomi_core::schema::ExtensionStoreRow>("SELECT * FROM extension_store")
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
        let ext = suwayomi_db::query_as::<suwayomi_core::schema::ExtensionRow>(
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
        let id: i32 = suwayomi_db::query_scalar(
            bind_placeholders(
                "INSERT INTO extension_store (index_url, name, is_legacy, badge_label, contact_website, signing_key) VALUES (?, '', FALSE, '', '', '') ON CONFLICT (index_url) DO UPDATE SET index_url = EXCLUDED.index_url RETURNING id",
            )
            .as_str(),
        )
        .bind(&input.index_url)
        .fetch_one(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
        let row = suwayomi_db::query_as::<suwayomi_core::schema::ExtensionStoreRow>(
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
        let row = suwayomi_db::query_as::<suwayomi_core::schema::ExtensionStoreRow>(
            bind_placeholders("SELECT * FROM extension_store WHERE index_url = ?").as_str(),
        )
        .bind(&input.index_url)
        .fetch_optional(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)?;
        suwayomi_db::query(bind_placeholders("DELETE FROM extension_store WHERE index_url = ?").as_str())
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
        ctx: &Context<'_>,
        input: UpdateSourcePreferenceInput,
    ) -> async_graphql::Result<UpdateSourcePreferencePayload> {
        let state = ctx.data::<GraphQLState>()?;
        let source_id = input.source.0;
        let position = input
            .change
            .position
            .ok_or_else(|| async_graphql::Error::new("missing preference position"))?;
        let value = preference_value(&input.change)?;
        let base = state
            .sandbox_base
            .clone()
            .ok_or_else(|| async_graphql::Error::new("extension sandbox is not running"))?;
        let fetcher = suwayomi_domain::source::sandbox::HttpSandboxFetcher::new(base);
        let updated = fetcher
            .set_source_preference(source_id, position, &value)
            .await
            .map_err(async_graphql::Error::from)?;
        if updated.is_none() {
            return Err(async_graphql::Error::new(format!("source {source_id} has no preferences")));
        }
        // 回读而不是把入参原样返回：扩展的监听器可能改别的项（比如填了服务器地址后
        // 把"已连接"开关置上），前端拿返回值直接覆盖本地状态，回读才能看到真实结果。
        let json = fetcher.source_preferences(source_id).await.map_err(async_graphql::Error::from)?;
        Ok(UpdateSourcePreferencePayload {
            client_mutation_id: input.client_mutation_id,
            preferences: json.as_deref().map(crate::types::parse_preferences).unwrap_or_default(),
            source: crate::mutation::fetch_source_type(state, source_id).await?,
        })
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

    /// 「存储管理 → 重建下载索引」：用磁盘重新对账下载，返回扫到的章节数。
    async fn rebuild_download_index(
        &self,
        ctx: &Context<'_>,
        input: RebuildDownloadIndexInput,
    ) -> async_graphql::Result<RebuildDownloadIndexPayload> {
        let state = ctx.data::<GraphQLState>()?;
        let chapters = suwayomi_domain::download::reconcile_downloads(&state.db, &state.data_dir)
            .await
            .map_err(async_graphql::Error::from)?;
        tracing::info!("download index rebuilt: {chapters} chapter(s)");
        Ok(RebuildDownloadIndexPayload {
            client_mutation_id: input.client_mutation_id,
            chapters: chapters as i32,
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
            settings: state.effective_settings().await,
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
        //
        // Merge with whatever is already persisted: the blob row is a single
        // JSON value, and callers submit single keys (e.g. only
        // autoBackupFrequency). Without merging, each save would drop every
        // previously stored setting and they'd silently fall back to
        // defaults after a restart.
        let json = partial_settings_to_json(&input.settings);
        let mut merged: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
        let existing_sql = bind_placeholders("SELECT value FROM global_meta WHERE meta_key = 'settings'");
        if let Ok(Some(existing)) = suwayomi_db::query_scalar::<String>(&existing_sql)
            .fetch_optional(state.db.pool())
            .await
            && let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(&existing)
        {
            merged = map;
        }
        if let serde_json::Value::Object(input_map) = json {
            for (k, v) in input_map {
                merged.insert(k, v);
            }
        }
        let json_str = serde_json::to_string(&serde_json::Value::Object(merged)).unwrap_or_else(|_| "{}".to_string());
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
        // 重算运行时配置：KOReader 冲突策略 / SyncYomi 开关由服务从 `state.config`
        // 读取，不刷新的话保存完只有设置页显示变了。
        state.reload_runtime_config().await;
        Ok(SetSettingsPayload {
            client_mutation_id: input.client_mutation_id,
            // 回读刚写下去的值：直接回 `from_config` 会把改动前的旧值当成保存结果，
            // 前端写进 Apollo 缓存后表现为"保存了但显示没变"。
            settings: state.effective_settings().await,
        })
    }

    /// Mirrors `login` — UI_LOGIN 模式下 WebUI 的登录入口。
    ///
    /// 用户名密码比对成功即签发一对 JWT；失败返回与其它未认证请求同样的
    /// `UnauthorizedException` 文案（WebUI 靠它识别认证失败）。
    async fn login(&self, ctx: &Context<'_>, input: LoginInput) -> async_graphql::Result<LoginPayload> {
        let state = ctx.data::<GraphQLState>()?;
        if !state.auth.verify_credentials(&input.username, &input.password) {
            return Err(async_graphql::Error::new(unauthorized_message()));
        }
        let (access_token, refresh_token) = state.auth.issue_tokens(suwayomi_core::auth::now());
        Ok(LoginPayload { access_token, client_mutation_id: input.client_mutation_id, refresh_token })
    }

    /// Mirrors `refreshToken` — 用 refresh token 换新的 access token。
    async fn refresh_token(
        &self,
        ctx: &Context<'_>,
        input: RefreshTokenInput,
    ) -> async_graphql::Result<RefreshTokenPayload> {
        let state = ctx.data::<GraphQLState>()?;
        if !state.auth.verify_token(&input.refresh_token, "refresh", suwayomi_core::auth::now()) {
            return Err(async_graphql::Error::new(unauthorized_message()));
        }
        let access_token = state.auth.issue_access_token(suwayomi_core::auth::now());
        Ok(RefreshTokenPayload { access_token, client_mutation_id: input.client_mutation_id })
    }
}

/// WebUI 的 `GraphQLClient.isAuthError` 就是按这个名字识别「凭据无效」的，
/// 与上游 `UnauthorizedException` 的类名一致——改它之前先改前端。
fn unauthorized_message() -> String {
    "suwayomi.tachidesk.server.user.UnauthorizedException: Unauthorized".to_string()
}

async fn fetch_chapter_row(state: &GraphQLState, id: i32) -> async_graphql::Result<suwayomi_core::schema::ChapterRow> {
    let sql = bind_placeholders("SELECT * FROM chapter WHERE id = ?");
    suwayomi_db::query_as::<suwayomi_core::schema::ChapterRow>(&sql)
        .bind(id)
        .fetch_one(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)
}

/// GraphQL 的部分开关 → 备份模块的部分开关（字段一一对应）。
pub(crate) fn backup_flags(input: Option<&PartialBackupFlagsInput>) -> suwayomi_core::backup::PartialBackupFlags {
    let Some(f) = input else {
        return suwayomi_core::backup::PartialBackupFlags::default();
    };
    suwayomi_core::backup::PartialBackupFlags {
        include_manga: f.include_manga,
        include_categories: f.include_categories,
        include_chapters: f.include_chapters,
        include_tracking: f.include_tracking,
        include_history: f.include_history,
        include_client_data: f.include_client_data,
        include_server_settings: f.include_server_settings,
    }
}

async fn fetch_track_record_row(state: &GraphQLState, id: i32) -> async_graphql::Result<TrackRecordRow> {
    let sql = bind_placeholders("SELECT * FROM track_record WHERE id = ?");
    suwayomi_db::query_as::<TrackRecordRow>(&sql)
        .bind(id)
        .fetch_one(state.db.pool())
        .await
        .map_err(async_graphql::Error::from)
}

/// 取某漫画在某追踪器上的记录 —— `bindTrack` 之后回读用（上游也是按这两个键找回来）。
async fn fetch_track_record_row_for(
    state: &GraphQLState,
    manga_id: i32,
    tracker_id: i32,
) -> async_graphql::Result<TrackRecordRow> {
    let sql = bind_placeholders("SELECT * FROM track_record WHERE manga_id = ? AND sync_id = ?");
    suwayomi_db::query_as::<TrackRecordRow>(&sql)
        .bind(manga_id)
        .bind(tracker_id)
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
        let manga: suwayomi_core::schema::MangaRow = suwayomi_db::query_as("SELECT * FROM manga WHERE id = $1")
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
            suwayomi_db::query_as("SELECT * FROM suwayomi.extension WHERE pkg_name = $1")
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
    suwayomi_db::query_as("SELECT * FROM suwayomi.extension WHERE pkg_name = $1")
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
    put!("dataDir", s.data_dir.clone());
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
