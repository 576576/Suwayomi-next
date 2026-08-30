//! Shared application state for REST handlers.

use std::sync::Arc;

use suwayomi_core::config::ServerConfig;
use suwayomi_core::db::Db;
use suwayomi_domain::category::category_manga::CategoryMangaService;
use suwayomi_domain::category::CategoryService;
use suwayomi_domain::chapter::ChapterService;
use suwayomi_domain::download::DownloadManager;
use suwayomi_domain::extension_store::ExtensionStoreService;
use suwayomi_domain::manga::library::LibraryService;
use suwayomi_domain::manga::manga_list::MangaListService;
use suwayomi_domain::manga::MangaService;
use suwayomi_domain::page::PageService;
use suwayomi_domain::source::SourceFetcher;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub config: ServerConfig,
    /// Extension source fetcher (stub until the JVM sandbox loads real extensions).
    pub fetcher: Arc<dyn SourceFetcher>,
    pub manga: MangaService,
    pub chapter: ChapterService,
    pub category: CategoryService,
    pub category_manga: CategoryMangaService,
    pub library: LibraryService,
    pub manga_list: MangaListService,
    pub page: PageService,
    /// Chapter download manager (queue + worker + event bus).
    pub download: DownloadManager,
    /// Extension store: repo refresh + online install.
    pub extension_store: ExtensionStoreService,
}

impl AppState {
    pub fn new(db: Db, config: ServerConfig, fetcher: Arc<dyn SourceFetcher>, sandbox_base: Option<String>) -> Self {
        let manga = MangaService::new(db.clone(), fetcher.clone());
        let chapter = ChapterService::new(db.clone(), fetcher.clone());
        let category = CategoryService::new(db.clone());
        let category_manga = CategoryMangaService::new(db.clone());
        let library = LibraryService::new(db.clone(), manga.clone());
        let manga_list = MangaListService::new(db.clone(), fetcher.clone());
        let page = PageService::new(db.clone());
        let download = DownloadManager::new(db.clone(), fetcher.clone());
        let extension_store = ExtensionStoreService::new(db.clone(), sandbox_base);
        Self { db, config, fetcher, manga, chapter, category, category_manga, library, manga_list, page, download, extension_store }
    }
}
