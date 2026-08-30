//! Pagination helpers — mirror `manga/model/dataclass/PaginatedList.kt`.

use serde::{Deserialize, Serialize};

pub const PAGINATION_FACTOR: usize = 50;

/// Mirrors `open class PaginatedList<T>(val page: List<T>, val hasNextPage: Boolean)`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedList<T> {
    pub page: Vec<T>,
    pub has_next_page: bool,
}

/// Mirrors `fun <T> paginatedFrom(pageNum, paginationFactor, lister)`.
/// Kotlin's `slice` shares element references; `T: Clone` mirrors that
/// for owned values.
pub fn paginated_from<T: Clone, F>(page_num: usize, pagination_factor: usize, lister: F) -> PaginatedList<T>
where
    F: FnOnce() -> Vec<T>,
{
    let list = lister();
    let last_index = list.len() as isize - 1;
    let lower_index = page_num * pagination_factor;
    let higher_index = (page_num + 1) * pagination_factor - 1;

    if lower_index as isize > last_index {
        return PaginatedList { page: Vec::new(), has_next_page: false };
    }

    let upper = (higher_index as isize).min(last_index) as usize;
    let sliced = list[lower_index..=upper].to_vec();
    let has_next = (higher_index as isize) < last_index;

    PaginatedList { page: sliced, has_next_page: has_next }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginated_from_slices_correctly() {
        let items: Vec<i32> = (0..120).collect();

        let page0 = paginated_from(0, PAGINATION_FACTOR, || items.clone());
        assert_eq!(page0.page.len(), 50);
        assert!(page0.has_next_page);

        let page2 = paginated_from(2, PAGINATION_FACTOR, || items.clone());
        assert_eq!(page2.page.len(), 20);
        assert!(!page2.has_next_page);

        let page3 = paginated_from(3, PAGINATION_FACTOR, || items.clone());
        assert!(page3.page.is_empty());
        assert!(!page3.has_next_page);
    }

    #[test]
    fn paginated_from_empty() {
        let page = paginated_from::<i32, _>(0, PAGINATION_FACTOR, std::vec::Vec::new);
        assert!(page.page.is_empty());
        assert!(!page.has_next_page);
    }
}
