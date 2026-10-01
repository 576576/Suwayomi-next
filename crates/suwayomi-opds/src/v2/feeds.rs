//! OPDS 2.0 feed builders and the Divina manifest builder.
//!
//! Same routes and same rows as the 1.2 module — only the encoding differs.
//! Two shape rules from the schemas drive most of the code here: every
//! publication needs an acquisition link (§6.1), and an empty result set must
//! fall back to `navigation` because `publications` is `minItems: 1` (§6.3).

use std::fmt::Write as _;

use chrono::{SecondsFormat, Utc};
use serde_json::{Value, json};
use suwayomi_core::db::Db;
use suwayomi_core::source::{MangasPage, SManga};
use suwayomi_core::text::urlencode;
use suwayomi_domain::source::{SourceBackend, SourceFetcher};

use crate::constants::TYPE_CBZ;
use crate::repository::{
    ChapterListEntry, LibraryFilter, MangaAcqEntry, NavEntry, OpdsRepository, Page, SortKey, SourceIdentity,
    split_genres,
};
use crate::v2::json::{
    CONFORMS_TO_DIVINA, CONTEXT_WEBPUB, ITEMS_PER_PAGE, MIME_DIVINA_JSON, MIME_OPDS_JSON, REL_ACQUISITION,
    REL_ACQUISITION_OPEN_ACCESS, REL_ALTERNATE, REL_FIRST, REL_LAST, REL_NEXT, REL_PREVIOUS, REL_SEARCH, REL_SELF,
    REL_START, REL_SUBSECTION, language_tag, non_empty, props,
};
use crate::v2::model::{
    BelongsTo, Facet, Feed, FeedMetadata, Link, Manifest, Publication, PublicationMetadata, Rel, SeriesRef,
};

const TYPE_IMAGE_JPEG: &str = "image/jpeg";
const TYPE_TEXT_HTML: &str = "text/html";

/// Feed context: database + route prefix + desired language + source backend.
///
/// The backend is only consulted for chapters whose stored page count is
/// unknown — see `chapter_manifest`.
pub struct V2Ctx<'a> {
    pub db: &'a Db,
    pub base_url: &'a str,
    pub lang: &'a str,
    pub fetcher: &'a SourceBackend,
}

/// Why a 2.0 feed or manifest could not be produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V2Error {
    /// No such series, or no such chapter in it.
    NotFound,
    /// The chapter's page count is unknown (`-1`) and the source could not
    /// supply it either, so no reading order can be built. Never answered with
    /// an empty `readingOrder`.
    PageCountUnknown,
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn epoch(epoch_millis: i64) -> String {
    chrono::DateTime::from_timestamp_millis(epoch_millis)
        .map_or_else(now, |dt| dt.to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn empty_page<T>() -> Page<T> {
    Page { items: Vec::new(), total: 0 }
}

// --- feed assembly ----------------------------------------------------------

struct FeedBuilder<'a> {
    ctx: &'a V2Ctx<'a>,
    id_path: String,
    title: String,
    page_num: Option<usize>,
    query_params: Vec<String>,
    sort: Option<String>,
    filter: Option<String>,
    facets: Vec<Facet>,
    navigation: Vec<Link>,
    publications: Vec<Publication>,
    total: Option<u64>,
}

impl<'a> FeedBuilder<'a> {
    fn new(ctx: &'a V2Ctx<'a>, id_path: &str, title: impl Into<String>) -> Self {
        Self {
            ctx,
            id_path: id_path.to_string(),
            title: title.into(),
            page_num: None,
            query_params: Vec::new(),
            sort: None,
            filter: None,
            facets: Vec::new(),
            navigation: Vec::new(),
            publications: Vec::new(),
            total: None,
        }
    }

    fn with_page(mut self, page: usize) -> Self {
        self.page_num = Some(page);
        self
    }

    fn with_query_params(mut self, params: Option<String>) -> Self {
        self.query_params = params.map(|p| vec![p]).unwrap_or_default();
        self
    }

    fn with_sort_filter(mut self, sort: Option<&str>, filter: Option<&str>) -> Self {
        self.sort = sort.map(str::to_string);
        self.filter = filter.map(str::to_string);
        self
    }

    /// Absolute in-site path, **without** a trailing slash before `?`.
    ///
    /// The 1.2 builder writes `{base}/?{query}`; axum's `nest` matches the
    /// path without the slash, so every 1.2 `rel="self"` currently resolves to
    /// the WebUI's SPA fallback instead of the feed (plan §1). Feed paths here
    /// are spelled the way the router actually serves them.
    fn url_with(&self, page: Option<usize>) -> String {
        let mut params = self.query_params.clone();
        if let Some(p) = page {
            params.push(format!("pageNumber={p}"));
        }
        params.push(format!("lang={}", self.ctx.lang));
        let path = if self.id_path.is_empty() {
            self.ctx.base_url.to_string()
        } else {
            format!("{}/{}", self.ctx.base_url, self.id_path)
        };
        format!("{path}?{}", params.join("&"))
    }

    fn root_href(&self) -> String {
        format!("{}?lang={}", self.ctx.base_url, self.lang())
    }

    fn lang(&self) -> &str {
        self.ctx.lang
    }

    fn identifier(&self) -> String {
        let mut parts = vec![self.ctx.lang.to_string()];
        if let Some(page) = self.page_num {
            parts.push(format!("page{page}"));
        }
        if !self.query_params.is_empty() {
            parts.push(self.query_params.join(":").replace('&', ":").replace('=', "_"));
        }
        if let Some(sort) = &self.sort {
            parts.push(format!("sort_{sort}"));
        }
        if let Some(filter) = &self.filter {
            parts.push(format!("filter_{filter}"));
        }
        let id_key = if self.id_path.is_empty() { "root".to_string() } else { self.id_path.replace('/', ":") };
        format!("urn:suwayomi:feed:v2:{id_key}:{}", parts.join(":"))
    }

    fn build(mut self) -> Feed {
        let mut links = vec![
            Link::new(REL_SELF, self.url_with(None)).with_type(MIME_OPDS_JSON).with_title("This feed"),
            Link::new(REL_START, self.root_href()).with_type(MIME_OPDS_JSON).with_title("OPDS Catalog Root"),
            Link {
                rel: Some(Rel::one(REL_SEARCH)),
                href: format!("{}/library/series{{?query,title,author}}", self.ctx.base_url),
                media_type: Some(MIME_OPDS_JSON.to_string()),
                title: Some("Search Catalog".to_string()),
                templated: Some(true),
                properties: None,
            },
        ];
        links.extend(self.pagination_links());

        // An empty page of publications cannot be serialised (`minItems: 1`),
        // so fall back to a navigation collection pointing at the root.
        if self.publications.is_empty() && self.navigation.is_empty() {
            self.navigation
                .push(Link::new(REL_START, self.root_href()).with_type(MIME_OPDS_JSON).with_title("OPDS Catalog Root"));
        }

        let metadata = FeedMetadata {
            identifier: Some(self.identifier()),
            title: self.title,
            modified: Some(now()),
            number_of_items: self.total,
            items_per_page: self.page_num.is_some().then_some(ITEMS_PER_PAGE),
            current_page: self.page_num,
        };
        Feed { metadata, links, navigation: self.navigation, publications: self.publications, facets: self.facets }
    }

    fn pagination_links(&self) -> Vec<Link> {
        let Some(page_num) = self.page_num else {
            return Vec::new();
        };
        let total = self.total.unwrap_or(0);
        let total_pages = if total == 0 { 0 } else { total.div_ceil(ITEMS_PER_PAGE as u64) as usize };
        if total_pages <= 1 {
            return Vec::new();
        }
        let current = page_num.max(1);
        let mut links = Vec::new();

        let first_href = self.url_with(Some(1));
        let previous_href = (current > 1).then(|| self.url_with(Some(current - 1)));
        // On page 2 `first` and `previous` address the same page; the spec's
        // own pagination example collapses them onto one link.
        let first_rel = match &previous_href {
            Some(previous) if *previous == first_href => {
                Rel::Many(vec![REL_FIRST.to_string(), REL_PREVIOUS.to_string()])
            }
            _ => Rel::one(REL_FIRST),
        };
        links.push(Link {
            rel: Some(first_rel),
            href: first_href.clone(),
            media_type: Some(MIME_OPDS_JSON.to_string()),
            title: Some("First Page".to_string()),
            templated: None,
            properties: None,
        });
        if let Some(previous) = previous_href.filter(|p| *p != first_href) {
            links.push(Link::new(REL_PREVIOUS, previous).with_type(MIME_OPDS_JSON).with_title("Previous Page"));
        }
        if current < total_pages {
            links.push(
                Link::new(REL_NEXT, self.url_with(Some(current + 1))).with_type(MIME_OPDS_JSON).with_title("Next Page"),
            );
        }
        links.push(
            Link::new(REL_LAST, self.url_with(Some(total_pages))).with_type(MIME_OPDS_JSON).with_title("Last Page"),
        );
        links
    }
}

/// A facet group whose active entry is marked with `rel: "self"` (the spec's
/// rule — there is no `activeFacet` boolean in 2.0). Groups with a single
/// entry carry no information and are dropped.
fn facet_group(title: &str, links: Vec<Link>) -> Option<Facet> {
    if links.len() < 2 {
        return None;
    }
    Some(Facet { metadata: FeedMetadata { title: title.to_string(), ..Default::default() }, links })
}

fn facet_link(label: &str, href: String, active: bool) -> Link {
    let mut link = Link::new(REL_SELF, href).with_type(MIME_OPDS_JSON).with_title(label);
    if !active {
        link.rel = None;
    }
    link
}

// --- publications -----------------------------------------------------------

/// A series publication. It has no downloadable artefact, so its acquisition
/// link is an *indirect* one: take the chapter feed, then a Divina manifest.
fn series_publication(ctx: &V2Ctx<'_>, manga: &MangaAcqEntry) -> Publication {
    let mut links = vec![
        Link::new(REL_ACQUISITION, format!("{}/series/{}/chapters?lang={}", ctx.base_url, manga.id, ctx.lang))
            .with_type(MIME_OPDS_JSON)
            .with_title("Chapters")
            .with_properties(props([
                ("numberOfItems", json!(manga.total_chapters)),
                ("indirectAcquisition", json!([{ "type": MIME_DIVINA_JSON }])),
            ])),
    ];
    if let Some(url) = &manga.url {
        links.push(
            Link::new(REL_ALTERNATE, source_page_url(manga.source_base_url.as_deref(), url))
                .with_type(TYPE_TEXT_HTML)
                .with_title("View on Web"),
        );
    }

    let mut images = Vec::new();
    if manga.thumbnail_url.is_some() {
        images.push(Link::bare(suwayomi_domain::manga::proxy_thumbnail_url(manga.id)).with_type(TYPE_IMAGE_JPEG));
    }

    Publication {
        metadata: PublicationMetadata {
            identifier: Some(format!("urn:suwayomi:manga:{}", manga.id)),
            title: manga.title.clone(),
            modified: Some(epoch(manga.last_fetched_at * 1000)),
            language: language_tag(&manga.source_lang).map(str::to_string),
            author: non_empty(manga.author.as_deref().unwrap_or_default()),
            publisher: non_empty(&manga.source_name),
            subject: manga.genres.clone(),
            description: non_empty(manga.description.as_deref().unwrap_or_default()),
            ..Default::default()
        },
        links,
        images,
    }
}

/// The chapter title rule shared with 1.2 (`feeds::chapter_title`): keep the
/// source's name, otherwise derive one from the chapter number.
fn chapter_title(name: &str, chapter_number: f32, source_order: i32, total_chapters: i64) -> String {
    let name = name.trim();
    if !name.is_empty() {
        return name.to_string();
    }
    if total_chapters <= 1 {
        "Oneshot".to_string()
    } else if chapter_number >= 0.0 {
        if chapter_number.fract() == 0.0 {
            format!("Chapter {}", chapter_number as i64)
        } else {
            format!("Chapter {chapter_number}")
        }
    } else {
        format!("Chapter {source_order}")
    }
}

/// Reading state of a chapter, lower-cased and hyphenated from 1.2's
/// `chapter_status` labels. Written into the manifest link's `properties` —
/// it has no slot in `metadata` and the link `properties` schema is open.
fn chapter_state(chapter: &ChapterListEntry) -> &'static str {
    if chapter.downloaded {
        "downloaded"
    } else if chapter.last_page_read > 0 {
        "in-progress"
    } else {
        "unread"
    }
}

/// A chapter publication. Always acquires a Divina manifest; a downloaded
/// chapter additionally offers its CBZ.
fn chapter_publication(ctx: &V2Ctx<'_>, chapter: &ChapterListEntry) -> Publication {
    let title =
        chapter_title(&chapter.name, chapter.chapter_number, chapter.source_order, chapter.manga_total_chapters);

    let mut state = vec![("state", json!(chapter_state(chapter)))];
    if chapter.last_page_read > 0 {
        state.push(("readPage", json!(chapter.last_page_read)));
    }
    if chapter.page_count > 0 {
        state.push(("readPages", json!(chapter.page_count)));
    }
    if chapter.last_read_at > 0 {
        state.push(("readAt", json!(epoch(chapter.last_read_at * 1000))));
    }

    let mut links = vec![
        Link::new(
            REL_ACQUISITION_OPEN_ACCESS,
            // Keyed on the chapter id, not `source_order`: the latter repeats
            // within a series (see `OpdsRepository::chapter_metadata_by_id`).
            format!("{}/series/{}/chapter/{}/manifest", ctx.base_url, chapter.manga_id, chapter.id),
        )
        .with_type(MIME_DIVINA_JSON)
        .with_title("Read")
        .with_properties(props(state)),
    ];
    if chapter.downloaded {
        links.push(
            Link::new(REL_ACQUISITION_OPEN_ACCESS, format!("/api/v1/chapter/{}/download?markAsRead=true", chapter.id))
                .with_type(TYPE_CBZ)
                .with_title("Download CBZ"),
        );
    }

    let mut images = Vec::new();
    if chapter.page_count > 0 {
        images.push(
            Link::bare(format!("/api/v1/manga/{}/chapter/{}/page/0", chapter.manga_id, chapter.source_order))
                .with_type(TYPE_IMAGE_JPEG),
        );
    }

    let scanlator = chapter.scanlator.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let description = scanlator.map(|s| {
        let mut text = title.clone();
        let _ = write!(text, " (Scanlator: {s})");
        text
    });

    Publication {
        metadata: PublicationMetadata {
            identifier: Some(format!("urn:suwayomi:chapter:{}", chapter.id)),
            title,
            modified: Some(epoch(chapter.upload_date)),
            author: non_empty(chapter.manga_author.as_deref().unwrap_or_default()),
            description,
            // `numberOfPages` is `exclusiveMinimum: 0` — an unknown count is
            // omitted rather than written as 0.
            number_of_pages: (chapter.page_count > 0).then_some(chapter.page_count),
            belongs_to: Some(BelongsTo {
                series: SeriesRef {
                    name: chapter.manga_title.clone(),
                    position: (chapter.chapter_number >= 0.0).then_some(chapter.chapter_number),
                },
            }),
            ..Default::default()
        },
        links,
        images,
    }
}

/// The absolute URL of a manga on its source.
///
/// `MangaAcqEntry::url` and `SManga::url` are the source's **own** address for
/// the manga — a path like `/g/450767/`, not a URL. Written into a link as-is
/// it resolves against *this* server and lands on the WebUI's SPA fallback;
/// 1.2 has exactly that in every `rel="alternate"`. Expanding it against the
/// source's `base_url` is what makes the link mean "the source's page".
///
/// A source with no `base_url` leaves the value as it came — such a source
/// (an unconfigured Komga entry) has no page to point at, and cannot produce a
/// listing to begin with.
fn source_page_url(base_url: Option<&str>, manga_url: &str) -> String {
    let base = base_url.unwrap_or_default().trim_end_matches('/');
    if base.is_empty() || manga_url.starts_with("http://") || manga_url.starts_with("https://") {
        return manga_url.to_string();
    }
    format!("{base}{manga_url}")
}

/// A manga that lives on a source but is not in the library.
///
/// Nothing on this server can serve it: there is no manga id, so no chapter
/// feed and no manifest. The only thing a client can do with it is open it on
/// the source, so the acquisition link points at the source's own page as
/// `text/html` — 1.2 emits the same entry with an **empty** href, a dead link.
fn remote_publication(manga: &SManga, source: &SourceIdentity) -> Publication {
    let mut images = Vec::new();
    if let Some(thumbnail) = &manga.thumbnail_url {
        images.push(Link::bare(thumbnail.clone()).with_type(TYPE_IMAGE_JPEG));
    }

    Publication {
        metadata: PublicationMetadata {
            identifier: Some(format!("urn:suwayomi:remote:{}", manga.url)),
            title: manga.title.clone(),
            modified: Some(now()),
            author: non_empty(manga.author.as_deref().unwrap_or_default()),
            publisher: non_empty(&source.name),
            subject: split_genres(manga.genre.as_deref()),
            description: non_empty(manga.description.as_deref().unwrap_or_default()),
            ..Default::default()
        },
        links: vec![
            Link::new(REL_ACQUISITION, source_page_url(source.base_url.as_deref(), &manga.url))
                .with_type(TYPE_TEXT_HTML)
                .with_title("Open on Source"),
        ],
        images,
    }
}

// --- feeds ------------------------------------------------------------------

/// Root navigation feed.
pub fn root_feed(ctx: &V2Ctx<'_>) -> Feed {
    const ITEMS: [(&str, &str, &str); 9] = [
        ("library/series", "Library", "All series in your library"),
        ("library/sources", "Library Sources", "Sources of series in your library"),
        ("library/categories", "Categories", "Browse library by category"),
        ("library/genres", "Genres", "Browse library by genre"),
        ("library/statuses", "Statuses", "Browse library by publication status"),
        ("library/languages", "Languages", "Browse library by content language"),
        ("explore", "Explore Sources", "Browse online sources"),
        ("history", "Reading History", "Recently read chapters"),
        ("library-updates", "Library Updates", "Recent chapter updates"),
    ];

    let mut builder = FeedBuilder::new(ctx, "", "OPDS Catalog");
    builder.total = Some(ITEMS.len() as u64);
    builder.navigation = ITEMS
        .iter()
        .map(|(path, title, description)| {
            Link::new(REL_SUBSECTION, format!("{}/{}?lang={}", ctx.base_url, path, ctx.lang))
                .with_type(MIME_OPDS_JSON)
                .with_title(*title)
                // Navigation links have no description slot; `properties` does.
                .with_properties(props([("description", json!(description))]))
        })
        .collect();
    builder.build()
}

/// Library series feed (cross-filters + sort + filter).
#[allow(clippy::too_many_arguments)]
pub async fn library_series_feed(
    ctx: &V2Ctx<'_>,
    source_id: Option<i64>,
    category_id: Option<i32>,
    status_id: Option<i32>,
    lang_code: Option<&str>,
    genre: Option<&str>,
    page_num: usize,
    sort: &str,
    filter: &str,
) -> Feed {
    let repo = OpdsRepository::new(ctx.db.pool());
    let result = repo
        .library_manga(
            source_id,
            category_id,
            status_id,
            lang_code,
            genre,
            page_num,
            SortKey::parse(sort),
            LibraryFilter::parse(filter),
        )
        .await
        .unwrap_or_else(|_| empty_page());

    let title = match (source_id, category_id, genre, status_id, lang_code) {
        (Some(id), ..) => format!("Source: {id}"),
        (_, Some(id), ..) => format!("Category: {id}"),
        (_, _, Some(g), ..) => format!("Genre: {g}"),
        (_, _, _, Some(id), _) => format!("Status: {id}"),
        (_, _, _, _, Some(l)) => format!("Language: {}", crate::repository::display_language(l)),
        _ => "All Series in Library".to_string(),
    };
    let feed_path = match (source_id, category_id, genre, status_id, lang_code) {
        (Some(id), ..) => format!("source/{id}"),
        (_, Some(id), ..) => format!("category/{id}"),
        (_, _, Some(g), ..) => format!("genre/{}", urlencode(g)),
        (_, _, _, Some(id), _) => format!("status/{id}"),
        (_, _, _, _, Some(l)) => format!("language/{l}"),
        _ => "library/series".to_string(),
    };

    let mut builder = FeedBuilder::new(ctx, &feed_path, title)
        .with_page(page_num)
        .with_query_params(cross_params(source_id, category_id, status_id, lang_code, genre))
        .with_sort_filter(Some(sort), Some(filter));
    builder.total = Some(result.total as u64);
    builder.facets = sort_facets(ctx, &feed_path, sort).into_iter().collect();
    builder.publications = result.items.iter().map(|m| series_publication(ctx, m)).collect();
    builder.build()
}

/// Search results feed (shares `/library/series`, as in 1.2).
pub async fn search_feed(
    ctx: &V2Ctx<'_>,
    query: Option<&str>,
    author: Option<&str>,
    title: Option<&str>,
    page_num: usize,
) -> Feed {
    let repo = OpdsRepository::new(ctx.db.pool());
    let result = repo.search_manga(query, author, title, page_num).await.unwrap_or_else(|_| empty_page());

    let query_params = {
        let mut parts = Vec::new();
        if let Some(q) = query.filter(|q| !q.is_empty()) {
            parts.push(format!("query={}", urlencode(q)));
        }
        if let Some(a) = author.filter(|a| !a.is_empty()) {
            parts.push(format!("author={}", urlencode(a)));
        }
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            parts.push(format!("title={}", urlencode(t)));
        }
        (!parts.is_empty()).then(|| parts.join("&"))
    };

    let mut builder = FeedBuilder::new(ctx, "library/series", "Search Results")
        .with_page(page_num)
        .with_query_params(query_params)
        .with_sort_filter(Some("title"), None);
    builder.total = Some(result.total as u64);
    builder.publications = result.items.iter().map(|m| series_publication(ctx, m)).collect();
    builder.build()
}

/// A `navigation` feed over repository nav entries.
///
/// `href_of` carries each feed's own link shape — 1.2 spells these per feed
/// (`explore_sources_feed`, `library_sources_feed`, …) and 2.0 must point at
/// the same routes.
fn navigation_feed(
    ctx: &V2Ctx<'_>,
    id_path: &str,
    title: &str,
    entries: &[NavEntry],
    href_of: impl Fn(&NavEntry) -> String,
) -> Feed {
    let mut builder = FeedBuilder::new(ctx, id_path, title);
    builder.total = Some(entries.len() as u64);
    builder.navigation = entries
        .iter()
        .map(|entry| {
            let mut properties: Vec<(&'static str, Value)> = Vec::new();
            if let Some(count) = entry.manga_count {
                properties.push(("numberOfItems", json!(count)));
            }
            if let Some(description) = &entry.description {
                properties.push(("description", json!(description)));
            }
            let link =
                Link::new(REL_SUBSECTION, href_of(entry)).with_type(MIME_OPDS_JSON).with_title(entry.title.clone());
            if properties.is_empty() { link } else { link.with_properties(props(properties)) }
        })
        .collect();
    builder.build()
}

/// Explore sources navigation feed (every installed source).
pub async fn explore_sources_feed(ctx: &V2Ctx<'_>) -> Feed {
    let repo = OpdsRepository::new(ctx.db.pool());
    let sources = repo.explore_sources().await.unwrap_or_default();
    navigation_feed(ctx, "explore", "Sources", &sources, |source| {
        format!("{}/explore/source/{}?sort=popular&lang={}", ctx.base_url, source.id, ctx.lang)
    })
}

/// Library sources navigation feed (sources that have series in the library).
pub async fn library_sources_feed(ctx: &V2Ctx<'_>) -> Feed {
    let repo = OpdsRepository::new(ctx.db.pool());
    let sources = repo.library_sources().await.unwrap_or_default();
    navigation_feed(ctx, "library/sources", "Library Sources", &sources, |source| {
        format!("{}/source/{}?lang={}", ctx.base_url, source.id, ctx.lang)
    })
}

/// Categories navigation feed.
pub async fn categories_feed(ctx: &V2Ctx<'_>) -> Feed {
    let repo = OpdsRepository::new(ctx.db.pool());
    let categories = repo.categories().await.unwrap_or_default();
    navigation_feed(ctx, "library/categories", "Categories", &categories, |category| {
        format!("{}/category/{}?lang={}", ctx.base_url, category.id, ctx.lang)
    })
}

/// Genres navigation feed.
pub async fn genres_feed(ctx: &V2Ctx<'_>) -> Feed {
    let repo = OpdsRepository::new(ctx.db.pool());
    let genres = repo.genres().await.unwrap_or_default();
    navigation_feed(ctx, "library/genres", "Genres", &genres, |genre| {
        format!("{}/genre/{}?lang={}", ctx.base_url, urlencode(&genre.id), ctx.lang)
    })
}

/// Statuses navigation feed.
pub async fn statuses_feed(ctx: &V2Ctx<'_>) -> Feed {
    let repo = OpdsRepository::new(ctx.db.pool());
    let statuses = repo.statuses().await.unwrap_or_default();
    navigation_feed(ctx, "library/statuses", "Statuses", &statuses, |status| {
        format!("{}/status/{}?lang={}", ctx.base_url, status.id, ctx.lang)
    })
}

/// Languages navigation feed.
pub async fn languages_feed(ctx: &V2Ctx<'_>) -> Feed {
    let repo = OpdsRepository::new(ctx.db.pool());
    let languages = repo.languages().await.unwrap_or_default();
    navigation_feed(ctx, "library/languages", "Languages", &languages, |language| {
        format!("{}/language/{}?lang={}", ctx.base_url, language.id, ctx.lang)
    })
}

/// A paged feed of chapter publications (`/history`, `/library-updates`).
///
/// The series name is **not** prefixed onto the title (1.2 prefixes it when
/// `add_manga_title` is set): it is already carried by `belongsTo.series.name`,
/// and 2.0 keeps each fact in one place.
fn chapter_list_feed(
    ctx: &V2Ctx<'_>,
    id_path: &str,
    title: &str,
    page_num: usize,
    result: Page<ChapterListEntry>,
) -> Feed {
    let mut builder = FeedBuilder::new(ctx, id_path, title).with_page(page_num);
    builder.total = Some(result.total as u64);
    builder.publications = result.items.iter().map(|chapter| chapter_publication(ctx, chapter)).collect();
    builder.build()
}

/// Recently read chapters.
pub async fn history_feed(ctx: &V2Ctx<'_>, page_num: usize) -> Feed {
    let repo = OpdsRepository::new(ctx.db.pool());
    let result = repo.history(page_num).await.unwrap_or_else(|_| empty_page());
    chapter_list_feed(ctx, "history", "Reading History", page_num, result)
}

/// Recent chapter additions for library manga.
pub async fn library_updates_feed(ctx: &V2Ctx<'_>, page_num: usize) -> Feed {
    let repo = OpdsRepository::new(ctx.db.pool());
    let result = repo.library_updates(page_num).await.unwrap_or_else(|_| empty_page());
    chapter_list_feed(ctx, "library-updates", "Library Updates", page_num, result)
}

/// Popular (or latest) manga of one source — remote entries (see
/// [`remote_publication`]).
pub async fn explore_source_feed(ctx: &V2Ctx<'_>, source_id: i64, page_num: usize, sort: &str) -> Feed {
    let repo = OpdsRepository::new(ctx.db.pool());
    let source = repo
        .source_identity(source_id)
        .await
        .ok()
        .flatten()
        .unwrap_or(SourceIdentity { name: source_id.to_string(), base_url: None });
    let title =
        if sort == "latest" { format!("Latest from {}", source.name) } else { format!("Popular from {}", source.name) };

    let mangas = fetch_popular(ctx, source_id, page_num, sort).await;
    // A source that says it has a next page gives a lower bound only — 1.2
    // counts it the same way.
    let total = if mangas.has_next_page {
        (page_num * ITEMS_PER_PAGE + 1) as u64
    } else {
        ((page_num.saturating_sub(1)) * ITEMS_PER_PAGE + mangas.mangas.len()) as u64
    };

    let mut builder = FeedBuilder::new(ctx, &format!("explore/source/{source_id}"), title)
        .with_page(page_num)
        .with_sort_filter(Some(sort), None);
    builder.total = Some(total);
    builder.publications = mangas.mangas.iter().map(|manga| remote_publication(manga, &source)).collect();
    builder.build()
}

/// Popular/latest listing for a source. `latest` only when the source says it
/// supports it; otherwise the popular listing stands in.
async fn fetch_popular(ctx: &V2Ctx<'_>, source_id: i64, page_num: usize, sort: &str) -> MangasPage {
    let fetcher = ctx.fetcher;
    if sort == "latest" && fetcher.supports_latest(source_id) {
        fetcher.get_latest_updates(source_id, page_num as u32).await.unwrap_or_default()
    } else {
        fetcher.get_popular_manga(source_id, page_num as u32).await.unwrap_or_default()
    }
}

fn cross_params(
    source_id: Option<i64>,
    category_id: Option<i32>,
    status_id: Option<i32>,
    lang_code: Option<&str>,
    genre: Option<&str>,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(id) = source_id {
        parts.push(format!("source_id={id}"));
    }
    if let Some(id) = category_id {
        parts.push(format!("category_id={id}"));
    }
    if let Some(id) = status_id {
        parts.push(format!("status_id={id}"));
    }
    if let Some(l) = lang_code {
        parts.push(format!("lang_code={l}"));
    }
    if let Some(g) = genre {
        parts.push(format!("genre={}", urlencode(g)));
    }
    if parts.is_empty() { None } else { Some(parts.join("&")) }
}

fn sort_facets(ctx: &V2Ctx<'_>, feed_path: &str, active: &str) -> Option<Facet> {
    const SORTS: [(&str, &str); 6] = [
        ("title", "Title"),
        ("date_added", "Date Added"),
        ("last_read_at", "Last Read"),
        ("last_modified_at", "Last Modified"),
        ("latest_upload", "Latest Upload"),
        ("total_chapters", "Total Chapters"),
    ];
    let links = SORTS
        .iter()
        .map(|(key, label)| {
            facet_link(label, format!("{}/{feed_path}?lang={}&sort={key}", ctx.base_url, ctx.lang), active == *key)
        })
        .collect();
    facet_group("Sort", links)
}

fn chapter_facets(ctx: &V2Ctx<'_>, manga_id: i32, active_sort: &str, active_filter: &str) -> Vec<Facet> {
    let base = format!("{}/series/{manga_id}/chapters", ctx.base_url);
    let sorts =
        [("number_asc", "Number ↑"), ("number_desc", "Number ↓"), ("date_asc", "Date ↑"), ("date_desc", "Date ↓")]
            .iter()
            .map(|(key, label)| {
                facet_link(
                    label,
                    format!("{base}?lang={}&sort={key}&filter={active_filter}", ctx.lang),
                    active_sort == *key,
                )
            })
            .collect();
    let filters = [("all", "All"), ("unread", "Unread")]
        .iter()
        .map(|(key, label)| {
            facet_link(
                label,
                format!("{base}?lang={}&sort={active_sort}&filter={key}", ctx.lang),
                active_filter == *key,
            )
        })
        .collect();
    [facet_group("Sort", sorts), facet_group("Filter", filters)].into_iter().flatten().collect()
}

/// Chapters of one series.
pub async fn series_chapters_feed(
    ctx: &V2Ctx<'_>,
    manga_id: i32,
    page_num: usize,
    sort: &str,
    filter: &str,
) -> Result<Feed, V2Error> {
    let repo = OpdsRepository::new(ctx.db.pool());
    let details = repo.manga_details(manga_id).await.map_err(|_| V2Error::NotFound)?.ok_or(V2Error::NotFound)?;
    let result = repo.chapters_for_manga(manga_id, sort, filter, page_num).await.map_err(|_| V2Error::NotFound)?;

    let mut builder =
        FeedBuilder::new(ctx, &format!("series/{manga_id}/chapters"), format!("{} — Chapters", details.title))
            .with_page(page_num)
            .with_sort_filter(Some(sort), Some(filter));
    builder.total = Some(result.total as u64);
    builder.facets = chapter_facets(ctx, manga_id, sort, filter);
    builder.publications = result.items.iter().map(|c| chapter_publication(ctx, c)).collect();
    Ok(builder.build())
}

// --- Divina manifest --------------------------------------------------------

/// Readium Divina manifest for a single chapter: one `readingOrder` entry per
/// page.
///
/// A chapter whose page count is unknown (`-1`) gets it from the source
/// (`SourceFetcher::fetch_pages`). When that fails too, the answer is
/// `PageCountUnknown` rather than an empty manifest — `readingOrder` is
/// required and an empty one is meaningless.
pub async fn chapter_manifest(ctx: &V2Ctx<'_>, manga_id: i32, chapter_id: i32) -> Result<Manifest, V2Error> {
    let repo = OpdsRepository::new(ctx.db.pool());
    let details = repo.manga_details(manga_id).await.map_err(|_| V2Error::NotFound)?.ok_or(V2Error::NotFound)?;
    let chapter = repo
        .chapter_metadata_by_id(manga_id, chapter_id)
        .await
        .map_err(|_| V2Error::NotFound)?
        .ok_or(V2Error::NotFound)?;
    let page_count = if chapter.page_count > 0 {
        chapter.page_count
    } else {
        source_page_count(ctx, chapter_id).await.ok_or(V2Error::PageCountUnknown)?
    };

    // Page URLs keep `source_order` — that is the REST layer's own key
    // (`/api/v1/manga/{id}/chapter/{source_order}/page/{n}`), not ours to change.
    let page_order = chapter.source_order;
    let href = format!("{}/series/{manga_id}/chapter/{chapter_id}/manifest", ctx.base_url);
    let reading_order = (0..page_count)
        .map(|page| {
            Link::bare(format!(
                "/api/v1/manga/{manga_id}/chapter/{page_order}/page/{page}?updateProgress=true&opds=true"
            ))
            .with_type(TYPE_IMAGE_JPEG)
        })
        .collect();

    Ok(Manifest {
        context: CONTEXT_WEBPUB.to_string(),
        metadata: PublicationMetadata {
            identifier: Some(format!("urn:suwayomi:chapter:{}", chapter.id)),
            title: chapter_title(&chapter.name, chapter.chapter_number, chapter.source_order, details.total_chapters),
            conforms_to: Some(CONFORMS_TO_DIVINA.to_string()),
            modified: Some(epoch(chapter.upload_date)),
            author: non_empty(details.author.as_deref().unwrap_or_default()),
            number_of_pages: Some(page_count),
            belongs_to: Some(BelongsTo {
                series: SeriesRef {
                    name: details.title.clone(),
                    position: (chapter.chapter_number >= 0.0).then_some(chapter.chapter_number),
                },
            }),
            ..Default::default()
        },
        links: vec![Link::new(REL_SELF, href).with_type(MIME_DIVINA_JSON)],
        reading_order,
    })
}

/// How many pages the source says a chapter has. `None` when the chapter has
/// no source address, the source cannot list its pages, or it lists none.
///
/// Deliberately read-only: a GET that writes the count back would make the
/// response depend on how many times it has been requested.
async fn source_page_count(ctx: &V2Ctx<'_>, chapter_id: i32) -> Option<i32> {
    let repo = OpdsRepository::new(ctx.db.pool());
    let addr = match repo.chapter_source_ref(chapter_id).await {
        Ok(addr) => addr?,
        Err(e) => {
            tracing::warn!(chapter = chapter_id, %e, "OPDS 2.0: cannot read the chapter's source address");
            return None;
        }
    };
    match ctx.fetcher.fetch_pages(addr.source_id, &addr.manga_url, &addr.chapter_url).await {
        Ok(pages) => i32::try_from(pages.len()).ok().filter(|n| *n > 0),
        Err(e) => {
            tracing::warn!(chapter = chapter_id, source = addr.source_id, %e, "OPDS 2.0: source page list unavailable");
            None
        }
    }
}

/// A `navigation` feed used as a 404 body — e-reader clients handle a minimal
/// catalog far better than an opaque JSON error.
pub fn not_found_feed(ctx: &V2Ctx<'_>, title: &str) -> Feed {
    let mut builder = FeedBuilder::new(ctx, "not-found", title);
    builder.total = Some(0);
    builder.navigation = vec![
        Link::new(REL_START, format!("{}?lang={}", ctx.base_url, ctx.lang))
            .with_type(MIME_OPDS_JSON)
            .with_title("OPDS Catalog Root"),
    ];
    builder.build()
}
