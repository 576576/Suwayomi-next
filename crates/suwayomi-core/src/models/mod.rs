//! Domain models — mirrors `suwayomi.manga.model.dataclass.*`

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
    MangaChapterDataClass, MangaDataClass, MangaStatus, PagedMangaListDataClass, UpdateStrategy, now_epoch_secs,
    to_genre_list,
};
pub use page::PageDataClass;
pub use pagination::{PAGINATION_FACTOR, PaginatedList, paginated_from};
pub use source::SourceDataClass;
pub use track::{MangaTrackerDataClass, TrackRecordDataClass, TrackSearchDataClass};
