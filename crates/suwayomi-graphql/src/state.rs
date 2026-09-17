//! Shared GraphQL state — services used by resolvers.

use std::sync::Arc;

use suwayomi_core::auth::AuthContext;
use suwayomi_core::config::ServerConfig;
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

use crate::updater::UpdateManager;

#[derive(Clone)]
pub struct GraphQLState {
    pub db: Db,
    pub config: ServerConfig,
    /// 认证参数：`login` / `refreshToken` 用它签发 JWT（与 session cookie 同一把密钥）。
    pub auth: Arc<AuthContext>,
    pub manga: MangaService,
    pub chapter: ChapterService,
    pub category: CategoryService,
    pub category_manga: CategoryMangaService,
    pub library: LibraryService,
    pub manga_list: MangaListService,
    pub page: PageService,
    /// Library updater with a broadcast event bus (Phase 6).
    pub update: UpdateManager,
    /// Chapter download manager (queue + worker + event bus).
    pub download: DownloadManager,
    /// KOReader progress sync (Phase 6).
    pub koreader: KoreaderSyncService,
    /// SyncYomi library sync (Phase 6).
    pub sync_yomi: SyncYomiService,
    /// Extension store: repo refresh + online install (Phase 6).
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
    pub fn new(
        db: Db,
        config: ServerConfig,
        auth: Arc<AuthContext>,
        fetcher: Arc<dyn SourceFetcher>,
        sandbox_base: Option<String>,
        webui_dir: std::path::PathBuf,
        data_dir: std::path::PathBuf,
    ) -> Self {
        let manga = MangaService::new(db.clone(), fetcher.clone());
        let chapter = ChapterService::new(db.clone(), fetcher.clone());
        let category = CategoryService::new(db.clone());
        let category_manga = CategoryMangaService::new(db.clone());
        let library = LibraryService::new(db.clone(), manga.clone());
        let manga_list = MangaListService::new(db.clone(), fetcher.clone());
        let page = PageService::new(db.clone());
        let update = UpdateManager::new(db.clone(), fetcher.clone());
        let download = DownloadManager::new(db.clone(), fetcher, data_dir.clone());
        let koreader = KoreaderSyncService::new(db.clone(), config.clone());
        let sync_yomi = SyncYomiService::new(db.clone(), config.clone());
        let extension_store = ExtensionStoreService::new(db.clone(), sandbox_base.clone());
        Self { db, config, auth, manga, chapter, category, category_manga, library, manga_list, page, update, download, koreader, sync_yomi, extension_store, webui_dir, data_dir, sandbox_base, backup_restores: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())) }
    }

    pub async fn set_backup_restore_status(&self, id: &str, status: crate::mutation_b4::BackupRestoreStatus) {
        self.backup_restores.lock().await.insert(id.to_string(), status);
    }

    pub async fn get_backup_restore_status(&self, id: &str) -> Option<crate::mutation_b4::BackupRestoreStatus> {
        self.backup_restores.lock().await.get(id).cloned()
    }

    /// `settings` 查询与 `setSettings` 返回值共用的装配：`ServerConfig` 上盖一层
    /// `global_meta` 里持久化的 blob（`setSettings` 写的）覆盖。
    ///
    /// `self.config` 是**启动时**算出来的，进程内不会随写入变化；不回读 blob 的话
    /// `setSettings` 的返回值永远是改动前的旧值，前端把它写进 Apollo 缓存后
    /// 保存过的项会显示成没保存。
    pub async fn effective_settings(&self) -> crate::settings::SettingsType {
        use suwayomi_domain::sql::bind_placeholders;
        let mut settings = crate::settings::SettingsType::from_config(&self.config);
        let sql = bind_placeholders("SELECT value FROM global_meta WHERE meta_key = ?");
        if let Ok(row) = suwayomi_db::query(&sql).bind("settings").fetch_optional(self.db.pool()).await
            && let Some(row) = row
            && let Ok(value) = row.try_get::<String, _>("value")
            && let Ok(blob) = serde_json::from_str::<serde_json::Value>(&value)
        {
            settings.apply_overrides(&blob);
        }
        settings
    }
}
