//! OPDS feed builders — mirrors `opds/impl/OpdsFeedBuilder.kt` +
//! `FeedBuilderInternal.kt`. Produces Atom XML strings for every feed type.

use std::pin::Pin;
use std::sync::Arc;

use chrono::{SecondsFormat, Utc};
use suwayomi_core::db::Db;
use suwayomi_domain::source::SourceFetcher;

use crate::constants::*;
use crate::model::{Author, Category, Content, Entry, Feed, Link, Summary};
use crate::repository::{
    ChapterListEntry, ChapterMetadataEntry, LibraryFilter, MangaAcqEntry, MangaDetails, NavEntry, OpdsRepository, SortKey,
};

/// Opaque feed context: database + upstream prefix + optional source fetcher.
pub struct FeedCtx<'a> {
    pub db: &'a Db,
    pub base_url: &'a str,
    pub lang: &'a str,
    pub fetcher: Option<Arc<dyn SourceFetcher>>,
}

fn now_opds() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn epoch_opds(epoch_millis: i64) -> String {
    match chrono::DateTime::from_timestamp_millis(epoch_millis) {
        Some(dt) => dt.to_rfc3339_opts(SecondsFormat::Secs, true),
        None => now_opds(),
    }
}

struct FeedBuilder<'a> {
    base_url: &'a str,
    lang: &'a str,
    id_path: String,
    title: String,
    feed_type: &'a str,
    page_num: Option<usize>,
    explicit_query_params: Option<String>,
    current_sort: Option<String>,
    current_filter: Option<String>,
    is_search_feed: bool,
    icon: Option<String>,
    extra_links: Vec<Link>,
    entries: Vec<Entry>,
    total_results: Option<u64>,
}

impl<'a> FeedBuilder<'a> {
    fn new(ctx: &FeedCtx<'a>, id_path: &str, title: String, feed_type: &'a str) -> Self {
        Self {
            base_url: ctx.base_url,
            lang: ctx.lang,
            id_path: id_path.to_string(),
            title,
            feed_type,
            page_num: None,
            explicit_query_params: None,
            current_sort: None,
            current_filter: None,
            is_search_feed: false,
            icon: None,
            extra_links: vec![],
            entries: vec![],
            total_results: None,
        }
    }

    fn with_page(mut self, page: Option<usize>) -> Self {
        self.page_num = page;
        self
    }

    fn with_query_params(mut self, params: Option<String>) -> Self {
        self.explicit_query_params = params;
        self
    }

    fn with_sort_filter(mut self, sort: Option<String>, filter: Option<String>) -> Self {
        self.current_sort = sort;
        self.current_filter = filter;
        self
    }

    fn url_with(&self, page: Option<usize>) -> String {
        let mut params: Vec<String> = Vec::new();
        if let Some(q) = &self.explicit_query_params {
            if !q.is_empty() {
                params.push(q.clone());
            }
        }
        if let Some(p) = page {
            params.push(format!("pageNumber={p}"));
        }
        if let Some(s) = &self.current_sort {
            params.push(format!("sort={s}"));
        }
        if let Some(f) = &self.current_filter {
            params.push(format!("filter={f}"));
        }
        params.push(format!("lang={}", self.lang));
        let q = params.join("&");
        let base = if self.id_path.is_empty() {
            self.base_url.to_string()
        } else {
            format!("{}/{}", self.base_url, self.id_path)
        };
        if q.is_empty() { base } else { format!("{base}/?{q}") }
    }

    fn build(self) -> Feed {
        let items_per_page = ITEMS_PER_PAGE;
        let urn_suffix_parts: Vec<String> = vec![
            Some(self.lang.to_string()),
            self.page_num.map(|p| format!("page{p}")),
            self.explicit_query_params.clone().map(|q| q.replace('&', ":").replace('=', "_")),
            self.current_sort.clone().map(|s| format!("sort_{s}")),
            self.current_filter.clone().map(|f| format!("filter_{f}")),
        ]
        .into_iter()
        .flatten()
        .collect();
        let urn_suffix = if urn_suffix_parts.is_empty() { String::new() } else { format!(":{}", urn_suffix_parts.join(":")) };
        let id = format!("urn:suwayomi:feed:{}:{}{urn_suffix}", self.id_path.replace('/', ":"), self.feed_type);

        let mut links = self.extra_links.clone();
        links.push(Link {
            rel: REL_SELF.into(),
            href: self.url_with(None),
            link_type: Some(self.feed_type.into()),
            title: Some("This feed".into()),
            ..Default::default()
        });
        links.push(Link {
            rel: REL_START.into(),
            href: format!("{}?lang={}", self.base_url, self.lang),
            link_type: Some(TYPE_ATOM_FEED_NAVIGATION.into()),
            title: Some("OPDS Catalog Root".into()),
            ..Default::default()
        });
        links.push(Link {
            rel: REL_SEARCH.into(),
            href: format!("{}/search?lang={}", self.base_url, self.lang),
            link_type: Some(TYPE_OPENSEARCH_DESCRIPTION.into()),
            title: Some("Search Catalog".into()),
            ..Default::default()
        });

        if let Some(page_num) = self.page_num {
            let total = self.total_results.unwrap_or(0);
            let total_pages = if total == 0 { 0 } else { ((total as f64) / (items_per_page as f64)).ceil() as usize };
            if total_pages > 1 {
                let current = page_num.max(1);
                links.push(Link {
                    rel: REL_FIRST.into(),
                    href: self.url_with(Some(1)),
                    link_type: Some(self.feed_type.into()),
                    title: Some("First Page".into()),
                    ..Default::default()
                });
                if current > 1 {
                    links.push(Link {
                        rel: REL_PREV.into(),
                        href: self.url_with(Some(current - 1)),
                        link_type: Some(self.feed_type.into()),
                        title: Some("Previous Page".into()),
                        ..Default::default()
                    });
                }
                if current < total_pages {
                    links.push(Link {
                        rel: REL_NEXT.into(),
                        href: self.url_with(Some(current + 1)),
                        link_type: Some(self.feed_type.into()),
                        title: Some("Next Page".into()),
                        ..Default::default()
                    });
                }
                links.push(Link {
                    rel: REL_LAST.into(),
                    href: self.url_with(Some(total_pages)),
                    link_type: Some(self.feed_type.into()),
                    title: Some("Last Page".into()),
                    ..Default::default()
                });
            }
        }

        let start_index = self.page_num.map(|p| (p.saturating_sub(1)) * items_per_page);
        Feed {
            id,
            title: self.title,
            updated: now_opds(),
            icon: self.icon,
            author: Author { name: "Suwayomi".into(), uri: Some("https://suwayomi.org/".into()) },
            links,
            entries: self.entries,
            total_results: self.total_results,
            items_per_page: Some(items_per_page),
            start_index,
        }
    }
}

fn nav_entry_to_entry(_ctx: &FeedCtx, entry: &NavEntry, href: &str) -> Entry {
    let mut link = Link::new(REL_SUBSECTION, href, TYPE_ATOM_FEED_ACQUISITION);
    link.title = Some(entry.title.clone());
    link.thr_count = entry.manga_count;
    Entry {
        id: format!("urn:suwayomi:navigation:{}", entry.id),
        title: entry.title.clone(),
        updated: now_opds(),
        links: vec![link],
        summary: entry.description.as_ref().map(|d| Summary { value: d.clone() }),
        ..Default::default()
    }
}

fn manga_entry_to_entry(ctx: &FeedCtx, entry: &MangaAcqEntry) -> Entry {
    let display_thumbnail = entry.thumbnail_url.as_deref().map(|_| suwayomi_domain::manga::proxy_thumbnail_url(entry.id));
    let category_scheme = if entry.in_library {
        format!("{}/library/genres", ctx.base_url)
    } else {
        format!("{}/genres", ctx.base_url)
    };

    let mut links = vec![Link {
        rel: REL_SUBSECTION.into(),
        href: format!("{}/series/{}/chapters?lang={}", ctx.base_url, entry.id, ctx.lang),
        link_type: Some(TYPE_ATOM_FEED_ACQUISITION.into()),
        title: Some(entry.title.clone()),
        ..Default::default()
    }];
    if let Some(url) = &entry.url {
        links.push(Link {
            rel: REL_ALTERNATE.into(),
            href: url.clone(),
            link_type: Some(TYPE_TEXT_HTML.into()),
            title: Some("View on Web".into()),
            ..Default::default()
        });
    }
    if let Some(t) = display_thumbnail {
        links.push(Link { rel: REL_IMAGE.into(), href: t.clone(), link_type: Some(TYPE_IMAGE_JPEG.into()), ..Default::default() });
        links.push(Link { rel: REL_IMAGE_THUMBNAIL.into(), href: t, link_type: Some(TYPE_IMAGE_JPEG.into()), ..Default::default() });
    }

    let summary_text = format!("Status: {} | Source: {} | Language: {}", status_name(entry.status), entry.source_name, entry.source_lang);
    Entry {
        id: format!("urn:suwayomi:manga:{}", entry.id),
        title: entry.title.clone(),
        updated: epoch_opds(entry.last_fetched_at * 1000),
        summary: Some(Summary { value: summary_text }),
        content: entry.description.as_ref().map(|d| Content { value: d.clone() }),
        links,
        authors: entry.author.as_ref().map(|a| vec![Author { name: a.clone(), uri: None }]).unwrap_or_default(),
        categories: entry
            .genres
            .iter()
            .map(|g| Category {
                scheme: Some(category_scheme.clone()),
                term: g.to_lowercase().replace(' ', "_"),
                label: g.clone(),
            })
            .collect(),
        publisher: Some(entry.source_name.clone()),
        language: Some(entry.source_lang.clone()),
        ..Default::default()
    }
}

fn chapter_title(chapter: &ChapterListEntry) -> String {
    let name = chapter.name.trim();
    if !name.is_empty() {
        return name.to_string();
    }
    if chapter.manga_total_chapters <= 1 {
        "Oneshot".to_string()
    } else if chapter.chapter_number >= 0.0 {
        if chapter.chapter_number.fract() == 0.0 {
            format!("Chapter {}", chapter.chapter_number as i64)
        } else {
            format!("Chapter {}", chapter.chapter_number)
        }
    } else {
        format!("Chapter {}", chapter.source_order)
    }
}

fn chapter_status(ch: &ChapterListEntry) -> &'static str {
    if ch.downloaded {
        "Downloaded"
    } else if ch.last_page_read > 0 {
        "In Progress"
    } else {
        "Unread"
    }
}

fn chapter_list_entry(ctx: &FeedCtx, chapter: &ChapterListEntry, add_manga_title: bool, skip_metadata: bool) -> Entry {
    let title_prefix = chapter_status(chapter);
    let chapter_name = chapter_title(chapter);
    let manga_part = if add_manga_title { format!(" {}:", chapter.manga_title) } else { String::new() };
    let entry_title = format!("{title_prefix}{manga_part} {chapter_name}");
    let mut details = format!("{} — {}", chapter.manga_title, chapter_name);
    if let Some(s) = &chapter.scanlator {
        if !s.is_empty() {
            details.push_str(&format!(" (Scanlator: {s})"));
        }
    }
    if chapter.page_count > 0 {
        details.push_str(&format!(" — {} of {} pages read", chapter.last_page_read, chapter.page_count));
    }

    let mut links = Vec::new();
    if skip_metadata {
        if chapter.downloaded {
            links.push(Link {
                rel: REL_ACQUISITION_OPEN_ACCESS.into(),
                href: format!("/api/v1/chapter/{}/download?markAsRead=true", chapter.id),
                link_type: Some(TYPE_CBZ.into()),
                title: Some("Download CBZ".into()),
                ..Default::default()
            });
        }
        if chapter.page_count > 0 {
            let base_page = format!(
                "/api/v1/manga/{}/chapter/{}/page/{{pageNumber}}?updateProgress=true&opds=true",
                chapter.manga_id, chapter.source_order
            );
            links.push(Link {
                rel: REL_PSE_STREAM.into(),
                href: base_page,
                link_type: Some(TYPE_IMAGE_JPEG.into()),
                title: Some(if chapter.last_page_read > 0 { "Continue Reading".into() } else { "Start Reading".into() }),
                pse_count: Some(chapter.page_count as usize),
                pse_last_read: (chapter.last_page_read > 0).then_some(chapter.last_page_read as usize),
                pse_last_read_date: (chapter.last_read_at > 0).then(|| epoch_opds(chapter.last_read_at * 1000)),
                ..Default::default()
            });
            links.push(Link {
                rel: REL_IMAGE.into(),
                href: format!("/api/v1/manga/{}/chapter/{}/page/0", chapter.manga_id, chapter.source_order),
                link_type: Some(TYPE_IMAGE_JPEG.into()),
                title: Some("Cover".into()),
                ..Default::default()
            });
        }
    } else {
        links.push(Link {
            rel: REL_SUBSECTION.into(),
            href: format!("{}/series/{}/chapter/{}/metadata?lang={}", ctx.base_url, chapter.manga_id, chapter.source_order, ctx.lang),
            link_type: Some(TYPE_ATOM_ENTRY_OPDS.into()),
            title: Some("Chapter Details".into()),
            ..Default::default()
        });
    }

    Entry {
        id: format!("urn:suwayomi:chapter:{}", chapter.id),
        title: entry_title,
        updated: epoch_opds(chapter.upload_date),
        summary: Some(Summary { value: details }),
        links,
        authors: chapter
            .manga_author
            .as_ref()
            .map(|a| vec![Author { name: a.clone(), uri: None }])
            .unwrap_or_default(),
        ..Default::default()
    }
}

fn status_name(status: i32) -> &'static str {
    match status {
        0 => "Unknown",
        1 => "Ongoing",
        2 => "Completed",
        3 => "Licensed",
        4 => "Publishing Finished",
        5 => "Cancelled",
        6 => "On Hiatus",
        _ => "Unknown",
    }
}

/// Root navigation feed.
pub async fn root_feed(ctx: &FeedCtx<'_>) -> String {
    let mut builder = FeedBuilder::new(ctx, "", "OPDS Catalog".into(), TYPE_ATOM_FEED_NAVIGATION);
    let items: Vec<NavEntry> = vec![
        NavEntry { id: "library/series".into(), title: "Library".into(), manga_count: None, description: Some("All series in your library".into()) },
        NavEntry { id: "library/sources".into(), title: "Library Sources".into(), manga_count: None, description: Some("Sources of series in your library".into()) },
        NavEntry { id: "library/categories".into(), title: "Categories".into(), manga_count: None, description: Some("Browse library by category".into()) },
        NavEntry { id: "library/genres".into(), title: "Genres".into(), manga_count: None, description: Some("Browse library by genre".into()) },
        NavEntry { id: "library/statuses".into(), title: "Statuses".into(), manga_count: None, description: Some("Browse library by publication status".into()) },
        NavEntry { id: "library/languages".into(), title: "Languages".into(), manga_count: None, description: Some("Browse library by content language".into()) },
        NavEntry { id: "explore".into(), title: "Explore Sources".into(), manga_count: None, description: Some("Browse online sources".into()) },
        NavEntry { id: "history".into(), title: "Reading History".into(), manga_count: None, description: Some("Recently read chapters".into()) },
        NavEntry { id: "library-updates".into(), title: "Library Updates".into(), manga_count: None, description: Some("Recent chapter updates".into()) },
    ];
    builder.total_results = Some(items.len() as u64);
    builder.entries = items
        .into_iter()
        .map(|item| {
            let mut link = Link::new(REL_SUBSECTION, format!("{}/{}?lang={}", ctx.base_url, item.id, ctx.lang), TYPE_ATOM_FEED_NAVIGATION);
            link.title = Some(item.title.clone());
            Entry {
                id: format!("urn:suwayomi:navigation:root:{}", item.id.replace('/', ":")),
                title: item.title,
                updated: now_opds(),
                links: vec![link],
                summary: item.description.as_ref().map(|d| Summary { value: d.clone() }),
                ..Default::default()
            }
        })
        .collect();
    builder.build().render()
}

/// OpenSearch description document.
pub fn search_description(lang: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <OpenSearchDescription xmlns=\"http://a9.com/-/spec/opensearch/1.1/\" \
         xmlns:atom=\"http://www.w3.org/2005/Atom\">\n\
         <ShortName>Suwayomi</ShortName>\n\
         <Description>Search the Suwayomi library</Description>\n\
         <InputEncoding>UTF-8</InputEncoding>\n\
         <OutputEncoding>UTF-8</OutputEncoding>\n\
         <Url type=\"{TYPE_ATOM_FEED_ACQUISITION}\" rel=\"results\" \
         template=\"/api/opds/v1.2/library/series?query={{searchTerms}}&amp;lang={lang}\"/>\n\
         </OpenSearchDescription>"
    )
}

/// Recently read chapters feed.
pub async fn history_feed(ctx: &FeedCtx<'_>, page_num: usize) -> String {
    let repo = OpdsRepository::new(ctx.db.pool());
    let result = repo.history(page_num).await.unwrap_or(Page::empty());
    let mut builder = FeedBuilder::new(ctx, "history", "Reading History".into(), TYPE_ATOM_FEED_ACQUISITION).with_page(Some(page_num));
    builder.total_results = Some(result.total as u64);
    builder.entries = result.items.iter().map(|c| chapter_list_entry(ctx, c, true, true)).collect();
    builder.build().render()
}

/// Search results feed.
pub async fn search_feed(ctx: &FeedCtx<'_>, query: Option<&str>, author: Option<&str>, title: Option<&str>, page_num: usize) -> String {
    let repo = OpdsRepository::new(ctx.db.pool());
    let result = repo.search_manga(query, author, title, page_num).await.unwrap_or(Page::empty());
    let query_params = query.filter(|q| !q.is_empty()).map(|q| format!("query={}", urlencode(q)));
    let mut builder = FeedBuilder::new(ctx, "library/series", "Search Results".into(), TYPE_ATOM_FEED_ACQUISITION)
        .with_page(Some(page_num))
        .with_query_params(query_params)
        .with_sort_filter(Some("title".into()), None);
    builder.is_search_feed = true;
    builder.total_results = Some(result.total as u64);
    builder.entries = result.items.iter().map(|m| manga_entry_to_entry(ctx, m)).collect();
    builder.build().render()
}

/// Library series feed (with cross-filters / sort / filter).
#[allow(clippy::too_many_arguments)]
pub async fn library_series_feed(
    ctx: &FeedCtx<'_>,
    source_id: Option<i64>,
    category_id: Option<i32>,
    status_id: Option<i32>,
    lang_code: Option<&str>,
    genre: Option<&str>,
    page_num: usize,
    sort: &str,
    filter: &str,
) -> String {
    let repo = OpdsRepository::new(ctx.db.pool());
    let result = repo
        .library_manga(source_id, category_id, status_id, lang_code, genre, page_num, SortKey::parse(sort), LibraryFilter::parse(filter))
        .await
        .unwrap_or(Page::empty());

    let title = match (source_id, category_id, genre, status_id, lang_code) {
        (Some(id), _, _, _, _) => format!("Source: {id}"),
        (_, Some(id), _, _, _) => format!("Category: {id}"),
        (_, _, Some(g), _, _) => format!("Genre: {g}"),
        (_, _, _, Some(id), _) => format!("Status: {id}"),
        (_, _, _, _, Some(l)) => format!("Language: {}", crate::repository::display_language(l)),
        _ => "All Series in Library".to_string(),
    };

    let cross_params = build_cross_params(source_id, category_id, status_id, lang_code, genre);
    let feed_path = match (source_id, category_id, genre, status_id, lang_code) {
        (Some(id), _, _, _, _) => format!("source/{id}"),
        (_, Some(id), _, _, _) => format!("category/{id}"),
        (_, _, Some(g), _, _) => format!("genre/{}", urlencode(g)),
        (_, _, _, Some(id), _) => format!("status/{id}"),
        (_, _, _, _, Some(l)) => format!("language/{l}"),
        _ => "library/series".to_string(),
    };

    let mut builder = FeedBuilder::new(ctx, &feed_path, title, TYPE_ATOM_FEED_ACQUISITION)
        .with_page(Some(page_num))
        .with_query_params(cross_params)
        .with_sort_filter(Some(sort.to_string()), Some(filter.to_string()));
    builder.total_results = Some(result.total as u64);
    add_sort_facets(&mut builder, ctx, &feed_path, sort);
    builder.entries = result.items.iter().map(|m| manga_entry_to_entry(ctx, m)).collect();
    builder.build().render()
}

fn build_cross_params(
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

fn add_sort_facets(builder: &mut FeedBuilder, ctx: &FeedCtx, feed_path: &str, active_sort: &str) {
    let sorts: &[(&str, &str)] = &[
        ("title", "Title"),
        ("date_added", "Date Added"),
        ("last_read_at", "Last Read"),
        ("last_modified_at", "Last Modified"),
        ("latest_upload", "Latest Upload"),
        ("total_chapters", "Total Chapters"),
    ];
    for (key, label) in sorts {
        let href = format!("{}/{feed_path}?lang={}&sort={key}", ctx.base_url, ctx.lang);
        builder.extra_links.push(Link {
            rel: REL_FACET.into(),
            href,
            link_type: Some(TYPE_ATOM_FEED_ACQUISITION.into()),
            title: Some(label.to_string()),
            facet_group: Some("sort".into()),
            active_facet: Some(active_sort == *key),
            ..Default::default()
        });
    }
}

/// Explore sources navigation feed.
pub async fn explore_sources_feed(ctx: &FeedCtx<'_>) -> String {
    let repo = OpdsRepository::new(ctx.db.pool());
    let sources = repo.explore_sources().await.unwrap_or_default();
    let mut builder = FeedBuilder::new(ctx, "explore", "Sources".into(), TYPE_ATOM_FEED_NAVIGATION);
    builder.total_results = Some(sources.len() as u64);
    builder.entries = sources
        .iter()
        .map(|s| {
            let href = format!("{}/explore/source/{}?sort=popular&lang={}", ctx.base_url, s.id, ctx.lang);
            nav_entry_to_entry(ctx, s, &href)
        })
        .collect();
    builder.build().render()
}

/// Library sources navigation feed.
pub async fn library_sources_feed(ctx: &FeedCtx<'_>) -> String {
    let repo = OpdsRepository::new(ctx.db.pool());
    let sources = repo.library_sources().await.unwrap_or_default();
    let mut builder = FeedBuilder::new(ctx, "library/sources", "Library Sources".into(), TYPE_ATOM_FEED_NAVIGATION);
    builder.total_results = Some(sources.len() as u64);
    builder.entries = sources
        .iter()
        .map(|s| {
            let href = format!("{}/source/{}?lang={}", ctx.base_url, s.id, ctx.lang);
            nav_entry_to_entry(ctx, s, &href)
        })
        .collect();
    builder.build().render()
}

/// Categories navigation feed.
pub async fn categories_feed(ctx: &FeedCtx<'_>) -> String {
    let repo = OpdsRepository::new(ctx.db.pool());
    let categories = repo.categories().await.unwrap_or_default();
    let mut builder = FeedBuilder::new(ctx, "library/categories", "Categories".into(), TYPE_ATOM_FEED_NAVIGATION);
    builder.total_results = Some(categories.len() as u64);
    builder.entries = categories
        .iter()
        .map(|c| {
            let href = format!("{}/category/{}?lang={}", ctx.base_url, c.id, ctx.lang);
            nav_entry_to_entry(ctx, c, &href)
        })
        .collect();
    builder.build().render()
}

/// Genres navigation feed.
pub async fn genres_feed(ctx: &FeedCtx<'_>) -> String {
    let repo = OpdsRepository::new(ctx.db.pool());
    let genres = repo.genres().await.unwrap_or_default();
    let mut builder = FeedBuilder::new(ctx, "library/genres", "Genres".into(), TYPE_ATOM_FEED_NAVIGATION);
    builder.total_results = Some(genres.len() as u64);
    builder.entries = genres
        .iter()
        .map(|g| {
            let href = format!("{}/genre/{}?lang={}", ctx.base_url, urlencode(&g.id), ctx.lang);
            nav_entry_to_entry(ctx, g, &href)
        })
        .collect();
    builder.build().render()
}

/// Statuses navigation feed.
pub async fn statuses_feed(ctx: &FeedCtx<'_>) -> String {
    let repo = OpdsRepository::new(ctx.db.pool());
    let statuses = repo.statuses().await.unwrap_or_default();
    let mut builder = FeedBuilder::new(ctx, "library/statuses", "Statuses".into(), TYPE_ATOM_FEED_NAVIGATION);
    builder.total_results = Some(statuses.len() as u64);
    builder.entries = statuses
        .iter()
        .map(|s| {
            let href = format!("{}/status/{}?lang={}", ctx.base_url, s.id, ctx.lang);
            nav_entry_to_entry(ctx, s, &href)
        })
        .collect();
    builder.build().render()
}

/// Languages navigation feed.
pub async fn languages_feed(ctx: &FeedCtx<'_>) -> String {
    let repo = OpdsRepository::new(ctx.db.pool());
    let languages = repo.languages().await.unwrap_or_default();
    let mut builder = FeedBuilder::new(ctx, "library/languages", "Languages".into(), TYPE_ATOM_FEED_NAVIGATION);
    builder.total_results = Some(languages.len() as u64);
    builder.entries = languages
        .iter()
        .map(|l| {
            let href = format!("{}/language/{}?lang={}", ctx.base_url, l.id, ctx.lang);
            nav_entry_to_entry(ctx, l, &href)
        })
        .collect();
    builder.build().render()
}

/// Library updates feed (recent chapter additions).
pub async fn library_updates_feed(ctx: &FeedCtx<'_>, page_num: usize) -> String {
    let repo = OpdsRepository::new(ctx.db.pool());
    let result = repo.library_updates(page_num).await.unwrap_or(Page::empty());
    let mut builder = FeedBuilder::new(ctx, "library-updates", "Library Updates".into(), TYPE_ATOM_FEED_ACQUISITION).with_page(Some(page_num));
    builder.total_results = Some(result.total as u64);
    builder.entries = result.items.iter().map(|c| chapter_list_entry(ctx, c, true, true)).collect();
    builder.build().render()
}

/// Explore source feed (popular/latest from the source fetcher).
pub async fn explore_source_feed(ctx: &FeedCtx<'_>, source_id: i64, page_num: usize, sort: &str) -> String {
    let repo = OpdsRepository::new(ctx.db.pool());
    let source_name = repo.source_name(source_id).await.ok().flatten().unwrap_or_else(|| source_id.to_string());
    let title = if sort == "latest" { format!("Latest from {source_name}") } else { format!("Popular from {source_name}") };
    let mut builder = FeedBuilder::new(ctx, &format!("explore/source/{source_id}"), title, TYPE_ATOM_FEED_ACQUISITION)
        .with_page(Some(page_num))
        .with_sort_filter(Some(sort.to_string()), None);

    let mangas = fetch_popular(ctx, source_id, page_num, sort).await;
    let total = if mangas.has_next_page {
        (page_num * ITEMS_PER_PAGE + 1) as u64
    } else {
        ((page_num.saturating_sub(1)) * ITEMS_PER_PAGE + mangas.mangas.len()) as u64
    };
    builder.total_results = Some(total);
    builder.entries = mangas.mangas.iter().map(|m| smanga_to_entry(ctx, m, &source_name)).collect();
    builder.build().render()
}

async fn fetch_popular(ctx: &FeedCtx<'_>, source_id: i64, page_num: usize, sort: &str) -> suwayomi_core::source::MangasPage {
    match &ctx.fetcher {
        Some(f) if sort == "latest" && f.supports_latest(source_id) => f.get_latest_updates(source_id, page_num as u32).await.unwrap_or_default(),
        Some(f) => f.get_popular_manga(source_id, page_num as u32).await.unwrap_or_default(),
        None => suwayomi_core::source::MangasPage::default(),
    }
}

fn smanga_to_entry(_ctx: &FeedCtx, m: &suwayomi_core::source::SManga, source_name: &str) -> Entry {
    let genres: Vec<String> = m
        .genre
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty())
        .collect();
    let mut links = vec![Link {
        rel: REL_SUBSECTION.into(),
        href: String::new(), // remote manga: no library id; readers rely on source browse only
        link_type: Some(TYPE_ATOM_FEED_ACQUISITION.into()),
        title: Some(m.title.clone()),
        ..Default::default()
    }];
    if let Some(t) = &m.thumbnail_url {
        links.push(Link { rel: REL_IMAGE.into(), href: t.clone(), link_type: Some(TYPE_IMAGE_JPEG.into()), ..Default::default() });
        links.push(Link { rel: REL_IMAGE_THUMBNAIL.into(), href: t.clone(), link_type: Some(TYPE_IMAGE_JPEG.into()), ..Default::default() });
    }
    Entry {
        id: format!("urn:suwayomi:remote:{}", m.url),
        title: m.title.clone(),
        updated: now_opds(),
        summary: m.description.as_ref().map(|d| Summary { value: d.clone() }),
        links,
        authors: m.author.as_ref().map(|a| vec![Author { name: a.clone(), uri: None }]).unwrap_or_default(),
        categories: genres.iter().map(|g| Category { scheme: None, term: g.to_lowercase().replace(' ', "_"), label: g.clone() }).collect(),
        publisher: Some(source_name.to_string()),
        ..Default::default()
    }
}

/// Series chapters feed.
pub async fn series_chapters_feed(ctx: &FeedCtx<'_>, manga_id: i32, page_num: usize, sort: &str, filter: &str) -> Result<String, String> {
    let repo = OpdsRepository::new(ctx.db.pool());
    let details = repo.manga_details(manga_id).await.map_err(|e| e.to_string())?.ok_or("manga not found")?;
    let result = repo.chapters_for_manga(manga_id, sort, filter, page_num).await.map_err(|e| e.to_string())?;
    let mut builder = FeedBuilder::new(
        ctx,
        &format!("series/{manga_id}/chapters"),
        format!("{} — Chapters", details.title),
        TYPE_ATOM_FEED_ACQUISITION,
    )
    .with_page(Some(page_num))
    .with_sort_filter(Some(sort.to_string()), Some(filter.to_string()));
    builder.total_results = Some(result.total as u64);
    if details.thumbnail_url.is_some() {
        let proxied = suwayomi_domain::manga::proxy_thumbnail_url(details.id);
        builder.icon = Some(proxied.clone());
        builder.extra_links.push(Link { rel: REL_IMAGE.into(), href: proxied.clone(), link_type: Some(TYPE_IMAGE_JPEG.into()), ..Default::default() });
        builder.extra_links.push(Link { rel: REL_IMAGE_THUMBNAIL.into(), href: proxied, link_type: Some(TYPE_IMAGE_JPEG.into()), ..Default::default() });
    }
    add_chapter_facets(&mut builder, ctx, manga_id, sort, filter);
    builder.entries = result.items.iter().map(|c| chapter_list_entry(ctx, c, false, false)).collect();
    Ok(builder.build().render())
}

fn add_chapter_facets(builder: &mut FeedBuilder, ctx: &FeedCtx, manga_id: i32, active_sort: &str, active_filter: &str) {
    let base = format!("{}/series/{manga_id}/chapters", ctx.base_url);
    for (key, label) in [("number_asc", "Number ↑"), ("number_desc", "Number ↓"), ("date_asc", "Date ↑"), ("date_desc", "Date ↓")] {
        builder.extra_links.push(Link {
            rel: REL_FACET.into(),
            href: format!("{base}?lang={}&sort={key}&filter={active_filter}", ctx.lang),
            link_type: Some(TYPE_ATOM_FEED_ACQUISITION.into()),
            title: Some(label.into()),
            facet_group: Some("sort".into()),
            active_facet: Some(active_sort == key),
            ..Default::default()
        });
    }
    for (key, label) in [("all", "All"), ("unread", "Unread")] {
        builder.extra_links.push(Link {
            rel: REL_FACET.into(),
            href: format!("{base}?lang={}&sort={active_sort}&filter={key}", ctx.lang),
            link_type: Some(TYPE_ATOM_FEED_ACQUISITION.into()),
            title: Some(label.into()),
            facet_group: Some("filter".into()),
            active_facet: Some(active_filter == key),
            ..Default::default()
        });
    }
}

/// Chapter metadata feed.
pub async fn chapter_metadata_feed(ctx: &FeedCtx<'_>, manga_id: i32, source_order: i32) -> Result<String, String> {
    let repo = OpdsRepository::new(ctx.db.pool());
    let details = repo.manga_details(manga_id).await.map_err(|e| e.to_string())?.ok_or("manga not found")?;
    let chapter = repo.chapter_metadata(manga_id, source_order).await.map_err(|e| e.to_string())?.ok_or("chapter not found")?;

    let mut builder = FeedBuilder::new(
        ctx,
        &format!("series/{manga_id}/chapter/{source_order}/metadata"),
        format!("{} — {}", details.title, chapter.name),
        TYPE_ATOM_FEED_ACQUISITION,
    );
    if details.thumbnail_url.is_some() {
        let proxied = suwayomi_domain::manga::proxy_thumbnail_url(details.id);
        builder.icon = Some(proxied.clone());
        builder.extra_links.push(Link { rel: REL_IMAGE.into(), href: proxied.clone(), link_type: Some(TYPE_IMAGE_JPEG.into()), ..Default::default() });
        builder.extra_links.push(Link { rel: REL_IMAGE_THUMBNAIL.into(), href: proxied, link_type: Some(TYPE_IMAGE_JPEG.into()), ..Default::default() });
    }
    builder.total_results = Some(1);
    builder.entries = vec![chapter_metadata_entry(&details, &chapter)];
    Ok(builder.build().render())
}

fn chapter_metadata_entry(manga: &MangaDetails, chapter: &ChapterMetadataEntry) -> Entry {
    let status = if chapter.downloaded {
        "Downloaded"
    } else if chapter.read {
        "Read"
    } else if chapter.last_page_read > 0 {
        "In Progress"
    } else {
        "Unread"
    };
    let mut links = Vec::new();
    if chapter.page_count > 0 {
        links.push(Link {
            rel: REL_PSE_STREAM.into(),
            href: format!("/api/v1/manga/{}/chapter/{}/page/{{pageNumber}}?updateProgress=true&opds=true", manga.id, chapter.source_order),
            link_type: Some(TYPE_IMAGE_JPEG.into()),
            title: Some(if chapter.last_page_read > 0 { "Continue Reading".into() } else { "Start Reading".into() }),
            pse_count: Some(chapter.page_count as usize),
            pse_last_read: (chapter.last_page_read > 0).then_some(chapter.last_page_read as usize),
            pse_last_read_date: (chapter.last_read_at > 0).then(|| epoch_opds(chapter.last_read_at * 1000)),
            ..Default::default()
        });
        links.push(Link {
            rel: REL_IMAGE.into(),
            href: format!("/api/v1/manga/{}/chapter/{}/page/0", manga.id, chapter.source_order),
            link_type: Some(TYPE_IMAGE_JPEG.into()),
            title: Some("Cover".into()),
            ..Default::default()
        });
    }
    if chapter.downloaded {
        links.push(Link {
            rel: REL_ACQUISITION_OPEN_ACCESS.into(),
            href: format!("/api/v1/chapter/{}/download?markAsRead=true", chapter.id),
            link_type: Some(TYPE_CBZ.into()),
            title: Some("Download CBZ".into()),
            ..Default::default()
        });
    }
    Entry {
        id: format!("urn:suwayomi:chapter:{}", chapter.id),
        title: format!("{status} {}", chapter.name),
        updated: epoch_opds(chapter.upload_date),
        summary: Some(Summary { value: format!("{} pages, {} of {} read", chapter.page_count, chapter.last_page_read, chapter.page_count) }),
        links,
        ..Default::default()
    }
}

/// Not-found feed (HTTP 404 body).
pub fn not_found_feed(ctx: &FeedCtx<'_>, id_path: &str, message: &str) -> String {
    FeedBuilder::new(ctx, id_path, message.to_string(), TYPE_ATOM_FEED_ACQUISITION).build().render()
}

fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => {
                let mut b = [0u8; 4];
                let bytes = c.encode_utf8(&mut b).as_bytes();
                bytes.iter().map(|x| format!("%{x:02X}")).collect()
            }
        })
        .collect()
}

// --- small helpers ----------------------------------------------------------

pub(crate) use crate::repository::Page;

trait PageEmpty<T> {
    fn empty() -> Self;
}

impl<T> PageEmpty<Page<T>> for Page<T> {
    fn empty() -> Self {
        Page { items: vec![], total: 0 }
    }
}

pub type PinFuture<T> = Pin<Box<dyn std::future::Future<Output = T> + Send>>;
