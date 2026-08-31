//! Shared GraphQL state — services used by resolvers.

use std::sync::Arc;

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
    /// WebUI static dir — version check reads `<dir>/revision`, updates swap the dir.
    pub webui_dir: std::path::PathBuf,
    /// User data root (backups/downloads/local source live under it).
    pub data_dir: std::path::PathBuf,
    /// JVM sandbox base URL (e.g. `http://127.0.0.1:8091`) — aboutServer JVM info.
    pub sandbox_base: Option<String>,
}

impl GraphQLState {
    pub fn new(
        db: Db,
        config: ServerConfig,
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
        let download = DownloadManager::new(db.clone(), fetcher);
        let koreader = KoreaderSyncService::new(db.clone(), config.clone());
        let sync_yomi = SyncYomiService::new(db.clone(), config.clone());
        let extension_store = ExtensionStoreService::new(db.clone(), sandbox_base.clone());
        Self { db, config, manga, chapter, category, category_manga, library, manga_list, page, update, download, koreader, sync_yomi, extension_store, webui_dir, data_dir, sandbox_base }
    }
}
