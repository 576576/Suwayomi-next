//! Domain models — mirrors `suwayomi.tachidesk.manga.model.dataclass.*`

pub mod category;
pub mod chapter;
pub mod extension;
pub mod manga;
pub mod page;
pub mod pagination;
pub mod source;
pub mod track;

pub use category::{CategoryDataClass, IncludeOrExclude};
pub use chapter::ChapterDataClass;
pub use extension::{ContentWarning, ExtensionDataClass, ExtensionInfo, ExtensionSource, ExtensionStore};
pub use manga::{
    now_epoch_secs, to_genre_list, MangaChapterDataClass, MangaDataClass, MangaStatus, PagedMangaListDataClass,
    UpdateStrategy,
};
pub use page::PageDataClass;
pub use pagination::{paginated_from, PaginatedList, PAGINATION_FACTOR};
pub use source::SourceDataClass;
pub use track::{MangaTrackerDataClass, TrackRecordDataClass, TrackSearchDataClass};
