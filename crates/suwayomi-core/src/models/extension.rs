//! Extension models — mirror `manga/model/dataclass/ExtensionDataClass.kt`,
//! `ExtensionInfo.kt` and `ExtensionStore.kt`.

use serde::{Deserialize, Serialize};

/// Mirrors `enum class ContentWarning` (SAFE, MIXED, NSFW — ordinal indexed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentWarning {
    Safe,
    Mixed,
    Nsfw,
}

impl ContentWarning {
    pub fn from_i32(value: i32) -> Self {
        match value {
            1 => Self::Mixed,
            2 => Self::Nsfw,
            _ => Self::Safe,
        }
    }

    pub fn to_i32(&self) -> i32 {
        match self {
            Self::Safe => 0,
            Self::Mixed => 1,
            Self::Nsfw => 2,
        }
    }
}

/// Mirrors `data class ExtensionDataClass`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionDataClass {
    pub repo: Option<String>,
    pub apk_name: String,
    pub icon_url: String,
    pub name: String,
    pub pkg_name: String,
    pub version_name: String,
    pub version_code: i32,
    pub lang: String,
    pub is_nsfw: bool,
    pub installed: bool,
    pub has_update: bool,
    pub obsolete: bool,
}

/// Mirrors `data class ExtensionSource`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionSource {
    pub id: i64,
    pub name: String,
    pub lang: String,
    pub home_url: String,
    pub message: Option<String>,
    pub content_warning: ContentWarning,
}

/// Mirrors `data class ExtensionInfo`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionInfo {
    pub store_index_url: String,
    pub name: String,
    pub pkg_name: String,
    pub apk_url: String,
    pub jar_url: Option<String>,
    pub icon_url: String,
    pub extension_lib: String,
    pub version_code: i64,
    pub version_name: String,
    pub lang: String,
    pub content_warning: ContentWarning,
    pub sources: Vec<ExtensionSource>,
}

/// Mirrors `data class ExtensionStore.Contact`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionStoreContact {
    pub website: String,
    pub discord: Option<String>,
}

/// Mirrors `data class ExtensionStore`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionStore {
    pub index_url: String,
    pub name: String,
    pub badge_label: String,
    pub signing_key: String,
    pub contact: ExtensionStoreContact,
    pub is_legacy: bool,
    pub extension_list_url: Option<String>,
}
