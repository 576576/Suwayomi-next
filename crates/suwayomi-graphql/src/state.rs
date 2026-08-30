//! Shared GraphQL state — services used by resolvers.

use std::sync::Arc;

use suwayomi_core::db::Db;
use suwayomi_domain::category::category_manga::CategoryMangaService;
use suwayomi_domain::category::CategoryService;
use suwayomi_domain::chapter::ChapterService;
use suwayomi_domain::manga::library::LibraryService;
use suwayomi_domain::manga::manga_list::MangaListService;
use suwayomi_domain::manga::MangaService;
use suwayomi_domain::page::PageService;
use suwayomi_domain::source::SourceFetcher;

#[derive(Clone)]
pub struct GraphQLState {
    pub db: Db,
    pub manga: MangaService,
    pub chapter: ChapterService,
    pub category: CategoryService,
    pub category_manga: CategoryMangaService,
    pub library: LibraryService,
    pub manga_list: MangaListService,
    pub page: PageService,
}

impl GraphQLState {
    pub fn new(db: Db, fetcher: Arc<dyn SourceFetcher>) -> Self {
        let manga = MangaService::new(db.clone(), fetcher.clone());
        let chapter = ChapterService::new(db.clone(), fetcher.clone());
        let category = CategoryService::new(db.clone());
        let category_manga = CategoryMangaService::new(db.clone());
        let library = LibraryService::new(db.clone(), manga.clone());
        let manga_list = MangaListService::new(db.clone(), fetcher);
        let page = PageService::new(db.clone());
        Self {
            db,
            manga,
            chapter,
            category,
            category_manga,
            library,
            manga_list,
            page,
        }
    }
}
