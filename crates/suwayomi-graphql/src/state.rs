//! Shared GraphQL state — services used by resolvers.

use std::sync::Arc;

use suwayomi_core::auth::AuthContext;
use suwayomi_core::config::{RuntimeConfig, ServerConfig};
use suwayomi_core::db::Db;
use suwayomi_domain::category::category_manga::CategoryMangaService;
use suwayomi_domain::category::CategoryService;
use suwayomi_domain::chapter::ChapterService;
use suwayomi_domain::manga::library::LibraryService;
use suwayomi_domain::manga::manga_list::MangaListService;
use suwayomi_domain::manga::MangaService;
use suwayomi_domain::page::PageService;
use suwayomi_domain::download::DownloadManager;
use suwayomi_domain::extension_store::ExtensionStoreService;
use suwayomi_domain::koreader_sync::KoreaderSyncService;
use suwayomi_domain::sync_yomi::SyncYomiService;
use suwayomi_domain::source::SourceFetcher;
use suwayomi_domain::tracker::TrackerManager;
use suwayomi_domain::updater::UpdateManager;

#[derive(Clone)]
pub struct GraphQLState {
    pub db: Db,
    /// 运行时配置：env 基线 + `global_meta` 里持久化的 settings blob。
    /// `setSettings` 落库后由 `reload_runtime_config` 刷新；KOReader / SyncYomi
    /// 等服务读同一份，改设置不必重启。
    pub config: RuntimeConfig,
    /// env 基线。重算时以它为起点，避免上一次的覆盖被再叠一层。
    config_base: ServerConfig,
    /// 认证参数：`login` / `refreshToken` 用它签发 JWT（与 session cookie 同一把密钥）。
    pub auth: Arc<AuthContext>,
    pub manga: MangaService,
    pub chapter: ChapterService,
    pub category: CategoryService,
    pub category_manga: CategoryMangaService,
    pub library: LibraryService,
    pub manga_list: MangaListService,
    pub page: PageService,
    /// Library updater with a broadcast event bus.
    pub update: UpdateManager,
    /// 追踪器（登录态、搜索、绑定、推送），与 REST 侧共用同一个句柄。
    pub tracker: TrackerManager,
    /// Chapter download manager (queue + worker + event bus).
    pub download: DownloadManager,
    /// KOReader progress sync.
    pub koreader: KoreaderSyncService,
    /// SyncYomi library sync.
    pub sync_yomi: SyncYomiService,
    /// Extension store: repo refresh + online install.
    pub extension_store: ExtensionStoreService,
    /// WebUI static dir — version check reads `<dir>/version.txt`, updates swap the dir.
    pub webui_dir: std::path::PathBuf,
    /// User data root (backups/downloads/local source live under it).
    pub data_dir: std::path::PathBuf,
    /// JVM sandbox base URL (e.g. `http://127.0.0.1:8091`) — aboutServer JVM info.
    pub sandbox_base: Option<String>,
    /// In-memory results of finished backup restores (`restoreStatus(id:)`).
    backup_restores: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<String, crate::mutation_b4::BackupRestoreStatus>>>,
}

impl GraphQLState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Db,
        config: ServerConfig,
        auth: Arc<AuthContext>,
        fetcher: Arc<dyn SourceFetcher>,
        update: UpdateManager,
        tracker: TrackerManager,
        sandbox_base: Option<String>,
        webui_dir: std::path::PathBuf,
        data_dir: std::path::PathBuf,
    ) -> Self {
        let manga = MangaService::new(db.clone(), fetcher.clone());
        let chapter = ChapterService::new(db.clone(), fetcher.clone()).with_tracker(tracker.clone());
        let category = CategoryService::new(db.clone());
        let category_manga = CategoryMangaService::new(db.clone());
        let library = LibraryService::new(db.clone(), manga.clone());
        let manga_list = MangaListService::new(db.clone(), fetcher.clone());
        let page = PageService::new(db.clone());
        let download = DownloadManager::new(db.clone(), fetcher, data_dir.clone());
        let runtime = RuntimeConfig::new(config.clone());
        let koreader = KoreaderSyncService::new(db.clone(), runtime.clone());
        let sync_yomi = SyncYomiService::new(db.clone(), runtime.clone());
        let extension_store = ExtensionStoreService::new(db.clone(), sandbox_base.clone());
        Self { db, config: runtime, config_base: config, auth, manga, chapter, category, category_manga, library, manga_list, page, update, tracker, download, koreader, sync_yomi, extension_store, webui_dir, data_dir, sandbox_base, backup_restores: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())) }
    }

    pub async fn set_backup_restore_status(&self, id: &str, status: crate::mutation_b4::BackupRestoreStatus) {
        self.backup_restores.lock().await.insert(id.to_string(), status);
    }

    pub async fn get_backup_restore_status(&self, id: &str) -> Option<crate::mutation_b4::BackupRestoreStatus> {
        self.backup_restores.lock().await.get(id).cloned()
    }

    /// 当前有效配置的快照（env 基线 + 持久化 blob）。
    pub fn config_snapshot(&self) -> ServerConfig {
        self.config.snapshot()
    }

    /// `global_meta` 里 `setSettings` 写下的 settings blob。
    async fn settings_blob(&self) -> Option<serde_json::Value> {
        use suwayomi_domain::sql::bind_placeholders;
        let sql = bind_placeholders("SELECT value FROM global_meta WHERE meta_key = ?");
        let row = suwayomi_db::query(&sql).bind("settings").fetch_optional(self.db.pool()).await.ok()??;
        let value = row.try_get::<String, _>("value").ok()?;
        serde_json::from_str::<serde_json::Value>(&value).ok()
    }

    /// 重算运行时配置：env 基线上盖一层持久化 blob，写进 `self.config`。
    ///
    /// 启动时调用一次（让上次保存的设置生效），`setSettings` 之后再调用一次
    /// （让改动立刻生效）。不刷新的话 `self.config` 只是 env 基线，设置页改了
    /// KOReader 冲突策略 / SyncYomi 开关，运行时读到的还是 `ServerConfig` 默认值。
    pub async fn reload_runtime_config(&self) {
        let mut config = self.config_base.clone();
        if let Some(blob) = self.settings_blob().await {
            config.apply_settings_blob(&blob);
        }
        self.config.replace(config);
    }

    /// `settings` 查询与 `setSettings` 返回值共用的装配：`ServerConfig` 上盖一层
    /// `global_meta` 里持久化的 blob（`setSettings` 写的）覆盖。
    ///
    /// blob 里 `ServerConfig` 不持有的那批字段（下载 / OPDS / FlareSolverr 等）
    /// 只有 `apply_overrides` 认识，所以这里仍要读一次 blob。
    pub async fn effective_settings(&self) -> crate::settings::SettingsType {
        let mut settings = crate::settings::SettingsType::from_config(&self.config.snapshot());
        if let Some(blob) = self.settings_blob().await {
            settings.apply_overrides(&blob);
        }
        settings
    }
}
