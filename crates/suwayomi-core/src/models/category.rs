//! Category model — mirrors `manga/model/dataclass/CategoryDataClass.kt`.

use serde::{Deserialize, Serialize};

/// Mirrors `enum class IncludeOrExclude` (EXCLUDE=0, INCLUDE=1, UNSET=-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncludeOrExclude {
    Exclude,
    Include,
    Unset,
}

impl IncludeOrExclude {
    pub fn from_i32(value: i32) -> Self {
        match value {
            0 => Self::Exclude,
            1 => Self::Include,
            _ => Self::Unset,
        }
    }

    pub fn to_i32(&self) -> i32 {
        match self {
            Self::Exclude => 0,
            Self::Include => 1,
            Self::Unset => -1,
        }
    }
}

/// Mirrors `data class CategoryDataClass`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryDataClass {
    pub id: i32,
    pub order: i32,
    pub name: String,
    pub default: bool,
    pub include_in_update: IncludeOrExclude,
    pub include_in_download: IncludeOrExclude,
    pub version: i64,
    pub uid: i64,
    pub last_modified_at: i64,
}
