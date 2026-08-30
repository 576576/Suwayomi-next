//! Source model — mirrors `manga/model/dataclass/SourceDataClass.kt`.

use serde::{Deserialize, Serialize};

/// Mirrors `data class SourceDataClass`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDataClass {
    pub id: String,
    pub name: String,
    pub lang: String,
    pub icon_url: String,
    /// The Source provides a latest listing
    pub supports_latest: bool,
    /// The Source implements ConfigurableSource
    pub is_configurable: bool,
    /// The Source class has a @Nsfw annotation
    pub is_nsfw: bool,
    /// A nicer version of name
    pub display_name: String,
    pub base_url: Option<String>,
}
