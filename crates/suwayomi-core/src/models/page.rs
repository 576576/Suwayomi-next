//! Page model — mirrors `manga/model/dataclass/PageDataClass.kt`.

use serde::{Deserialize, Serialize};

/// Mirrors `data class PageDataClass`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageDataClass {
    pub index: i32,
    pub image_url: String,
}
