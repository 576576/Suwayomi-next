//! JSON model for OPDS 2.0 feeds and Readium Web Publication Manifests.
//!
//! Every optional field is omitted rather than serialised as `null`: the
//! published OPDS 2.0 schemas (`https://specs.opds.io/schema/`) type most
//! fields as `string` / `integer`, and `null` fails those. Empty collections
//! are omitted too — `publications`, `navigation`, `facets` and `images` are
//! all `minItems: 1`, so emitting `[]` is invalid (see the plan's §6.3).

use serde::Serialize;
use serde_json::{Map, Value};

/// A link's `rel` is either one value or a list — `first` and `previous`
/// collapse onto the same link when the reader is on page 2.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Rel {
    One(String),
    Many(Vec<String>),
}

impl Rel {
    pub fn one(rel: &str) -> Self {
        Self::One(rel.to_string())
    }
}

/// Link Object — shared by feeds, publications and manifests.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Link {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rel: Option<Rel>,
    pub href: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Set only for URI templates; the link schema validates `href` against
    /// `uri-template` when this is true and `uri-reference` otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub templated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Map<String, Value>>,
}

impl Link {
    pub fn new(rel: &str, href: impl Into<String>) -> Self {
        Self { rel: Some(Rel::one(rel)), href: href.into(), ..Default::default() }
    }

    /// A link with no `rel` — used where the collection role already carries
    /// the meaning (`readingOrder` entries, `images`).
    pub fn bare(href: impl Into<String>) -> Self {
        Self { href: href.into(), ..Default::default() }
    }

    pub fn with_type(mut self, media_type: &str) -> Self {
        self.media_type = Some(media_type.to_string());
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_properties(mut self, properties: Map<String, Value>) -> Self {
        self.properties = Some(properties);
        self
    }
}

/// The `series` object of `metadata.belongsTo`.
#[derive(Debug, Clone, Serialize)]
pub struct SeriesRef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<f32>,
}

/// `metadata.belongsTo` — ties a chapter publication back to its series.
#[derive(Debug, Clone, Serialize)]
pub struct BelongsTo {
    pub series: SeriesRef,
}

/// Publication metadata (the Readium vocabulary shared by OPDS 2.0 and RWPM).
#[derive(Debug, Clone, Default, Serialize)]
pub struct PublicationMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    pub title: String,
    /// Divina compliance statement; only manifests carry this.
    #[serde(rename = "conformsTo", skip_serializing_if = "Option::is_none")]
    pub conforms_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
    /// Must already be a valid BCP-47 tag — see [`crate::v2::json::language_tag`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub subject: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "numberOfPages", skip_serializing_if = "Option::is_none")]
    pub number_of_pages: Option<i32>,
    #[serde(rename = "belongsTo", skip_serializing_if = "Option::is_none")]
    pub belongs_to: Option<BelongsTo>,
}

/// Publication Object.
#[derive(Debug, Clone, Serialize)]
pub struct Publication {
    pub metadata: PublicationMetadata,
    /// `publication.schema.json` requires at least one acquisition link, so
    /// this is never empty.
    pub links: Vec<Link>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<Link>,
}

/// Feed-level metadata.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FeedMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    #[serde(rename = "numberOfItems", skip_serializing_if = "Option::is_none")]
    pub number_of_items: Option<u64>,
    #[serde(rename = "itemsPerPage", skip_serializing_if = "Option::is_none")]
    pub items_per_page: Option<usize>,
    /// 1-based — the 1.2 feed's `opensearch:startIndex` is a 0-based offset.
    #[serde(rename = "currentPage", skip_serializing_if = "Option::is_none")]
    pub current_page: Option<usize>,
}

/// One facet group (`metadata.title` names the group, e.g. "Sort").
#[derive(Debug, Clone, Serialize)]
pub struct Facet {
    pub metadata: FeedMetadata,
    pub links: Vec<Link>,
}

/// OPDS 2.0 feed. At least one of `navigation` / `publications` must be
/// present — the schema's `anyOf` demands it.
#[derive(Debug, Clone, Serialize)]
pub struct Feed {
    pub metadata: FeedMetadata,
    pub links: Vec<Link>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub navigation: Vec<Link>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub publications: Vec<Publication>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub facets: Vec<Facet>,
}

/// Readium Web Publication Manifest (Divina profile) for a single chapter.
#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    #[serde(rename = "@context")]
    pub context: String,
    pub metadata: PublicationMetadata,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
    /// Every entry must carry a `type` — RWPM says so explicitly.
    #[serde(rename = "readingOrder")]
    pub reading_order: Vec<Link>,
}
