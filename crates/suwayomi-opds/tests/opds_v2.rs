//! OPDS 2.0 feed and Divina manifest tests.
//!
//! The assertions here mirror the schema constraints in
//! `docs/agent/plans/opds-v2.md` §6 — they are the reason several fields are
//! omitted rather than emitted empty.

// 集成测试里 panic 就是断言失败的表达方式，不需要改成错误传播。
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::indexing_slicing
)]

use std::sync::{Arc, Mutex};

use serde_json::Value;
use suwayomi_core::db::Db;
use suwayomi_core::source::{MangasPage, SChapter, SManga, SourcePage};
use suwayomi_domain::error::DomainError;
use suwayomi_domain::source::{SourceBackend, SourceFetcher};
use suwayomi_opds::v2::feeds::{self, V2Ctx, V2Error};

/// Library with two series: one English source, one source declared `all`
/// (a Mihon pseudo-language that may not be written into `metadata.language`).
async fn seed() -> Db {
    let db = Db::sqlite_in_memory().await.expect("connect sqlite");
    db.migrate().await.expect("migrate");
    let pool = db.pool();

    suwayomi_db::query(
        "INSERT INTO extension (name, pkg_name, version_name, version_code, lang, content_warning) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind("Test Extension")
    .bind("eu.test.pkg")
    .bind("1.0")
    .bind(1_i64)
    .bind("en")
    .bind(0_i32)
    .execute(pool)
    .await
    .expect("insert extension");

    for (name, lang, base_url) in
        [("MangaDex", "en", "https://mangadex.org"), ("Everylang", "all", "https://everylang.example")]
    {
        suwayomi_db::query("INSERT INTO source (name, lang, extension, base_url) VALUES ($1, $2, $3, $4)")
            .bind(name)
            .bind(lang)
            .bind(1_i32)
            .bind(base_url)
            .execute(pool)
            .await
            .expect("insert source");
    }

    // id 1 — English source.
    suwayomi_db::query(
        "INSERT INTO manga (url, title, initialized, artist, author, description, genre, status, thumbnail_url, \
         in_library, source, last_fetched_at, last_modified_at) \
         VALUES ($1, $2, TRUE, $3, $4, $5, $6, $7, $8, TRUE, $9, $10, $11)",
    )
    .bind("/series/1")
    .bind("Test Manga")
    .bind("Artist A")
    .bind("Author B")
    .bind("A great test manga")
    .bind("Action, Comedy")
    .bind(1_i32)
    .bind("https://example.com/t.jpg")
    .bind(1_i64)
    .bind(1_700_000_000_000_i64)
    .bind(1_700_000_000_000_i64)
    .execute(pool)
    .await
    .expect("insert manga 1");

    // id 2 — `all` source; title sorts before "Test Manga".
    suwayomi_db::query(
        "INSERT INTO manga (url, title, initialized, status, in_library, source, last_fetched_at, last_modified_at) \
         VALUES ($1, $2, TRUE, $3, TRUE, $4, $5, $5)",
    )
    .bind("/series/2")
    .bind("Alpha Manga")
    .bind(1_i32)
    .bind(2_i64)
    .bind(1_700_000_000_000_i64)
    .execute(pool)
    .await
    .expect("insert manga 2");

    // Chapter 1: downloaded, unread.
    insert_chapter(&db, "/series/1/ch/1", "Chapter 1", 1.0, false, 0, 0, 1, true, 20).await;
    // Chapter 2: read to page 5.
    insert_chapter(&db, "/series/1/ch/2", "Chapter 2", 2.0, true, 5, 1_700_000_500_000, 2, false, 15).await;
    // Chapter 3: unnamed and of unknown length (-1) — exercises the title
    // fallback and the manifest's unknown-page-count path.
    insert_chapter(&db, "/series/1/ch/3", "", 3.0, false, 0, 0, 3, false, -1).await;

    db
}

#[allow(clippy::too_many_arguments)]
async fn insert_chapter(
    db: &Db,
    url: &str,
    name: &str,
    number: f32,
    read: bool,
    last_page_read: i32,
    last_read_at: i64,
    source_order: i32,
    downloaded: bool,
    page_count: i32,
) {
    suwayomi_db::query(
        "INSERT INTO chapter (url, name, date_upload, chapter_number, read, last_page_read, last_read_at, \
         source_order, is_downloaded, page_count, manga) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(url)
    .bind(name)
    .bind(1_700_000_100_000_i64 + i64::from(source_order))
    .bind(number)
    .bind(read)
    .bind(last_page_read)
    .bind(last_read_at)
    .bind(source_order)
    .bind(downloaded)
    .bind(page_count)
    .bind(1_i32)
    .execute(db.pool())
    .await
    .expect("insert chapter");
}

/// No extension sandbox in tests, so the backend is the stub — every fetch
/// fails, which is exactly the production shape when no sandbox is running.
static STUB: SourceBackend = SourceBackend::Stub;

fn ctx(db: &Db) -> V2Ctx<'_> {
    V2Ctx { db, base_url: "/api/opds/v2", lang: "en", fetcher: &STUB }
}

fn ctx_with<'a>(db: &'a Db, fetcher: &'a SourceBackend) -> V2Ctx<'a> {
    V2Ctx { db, base_url: "/api/opds/v2", lang: "en", fetcher }
}

/// A page list of a fixed length, or a source failure — the seam
/// `chapter_manifest` falls back to when the stored `page_count` is `-1`.
///
/// Records every `fetch_pages` call, so a test can tell "never asked" from
/// "asked and got the same answer".
struct PageListStub {
    pages: Option<usize>,
    calls: Mutex<Vec<(i64, String, String)>>,
}

impl PageListStub {
    /// The stub itself (to inspect) plus a backend wrapping it. Returning the
    /// backend keeps it alive for the caller — the context only borrows it.
    fn backend(pages: Option<usize>) -> (Arc<Self>, SourceBackend) {
        let stub = Arc::new(Self { pages, calls: Mutex::new(Vec::new()) });
        let shared: Arc<dyn SourceFetcher> = stub.clone();
        (stub, SourceBackend::Test(shared))
    }

    fn calls(&self) -> Vec<(i64, String, String)> {
        self.calls.lock().expect("stub mutex").clone()
    }
}

#[async_trait::async_trait]
impl SourceFetcher for PageListStub {
    async fn fetch_manga_update(
        &self,
        _source_id: i64,
        _manga: &SManga,
        _chapters: &[SChapter],
        _fetch_details: bool,
        _fetch_chapters: bool,
    ) -> suwayomi_domain::error::Result<(SManga, Vec<SChapter>)> {
        Err(DomainError::Source("unused in this stub".into()))
    }

    async fn get_popular_manga(&self, _source_id: i64, _page: u32) -> suwayomi_domain::error::Result<MangasPage> {
        Ok(MangasPage::default())
    }

    async fn get_latest_updates(&self, _source_id: i64, _page: u32) -> suwayomi_domain::error::Result<MangasPage> {
        Ok(MangasPage::default())
    }

    async fn search_manga(
        &self,
        _source_id: i64,
        _query: &str,
        _page: u32,
    ) -> suwayomi_domain::error::Result<MangasPage> {
        Ok(MangasPage::default())
    }

    async fn fetch_pages(
        &self,
        source_id: i64,
        manga_url: &str,
        chapter_url: &str,
    ) -> suwayomi_domain::error::Result<Vec<SourcePage>> {
        self.calls.lock().expect("stub mutex").push((source_id, manga_url.to_string(), chapter_url.to_string()));
        self.pages.map_or_else(
            || Err(DomainError::Source("source unavailable".into())),
            |count| {
                Ok((0..count as i32)
                    .map(|i| SourcePage::new(i, format!("https://example.com/{i}.jpg"), None))
                    .collect())
            },
        )
    }

    fn supports_latest(&self, _source_id: i64) -> bool {
        false
    }
}

fn value<T: serde::Serialize>(item: &T) -> Value {
    serde_json::to_value(item).expect("serialise")
}

fn array<'a>(value: &'a Value, key: &str) -> &'a Vec<Value> {
    value[key].as_array().unwrap_or_else(|| panic!("{key} should be an array"))
}

const ACQUISITION_RELS: [&str; 6] = [
    "http://opds-spec.org/acquisition",
    "http://opds-spec.org/acquisition/open-access",
    "http://opds-spec.org/acquisition/buy",
    "http://opds-spec.org/acquisition/borrow",
    "http://opds-spec.org/acquisition/sample",
    "http://opds-spec.org/acquisition/subscribe",
];

#[tokio::test]
async fn root_feed_is_a_navigation_collection() {
    let db = seed().await;
    let feed = value(&feeds::root_feed(&ctx(&db)));

    assert_eq!(feed["metadata"]["title"], "OPDS Catalog");
    assert!(feed.get("publications").is_none(), "navigation feed has no publications");
    assert!(!array(&feed, "navigation").is_empty());
    // Every navigation link must carry a title (feed schema `allOf`).
    assert!(array(&feed, "navigation").iter().all(|link| link["title"].is_string()));

    let links = array(&feed, "links");
    let self_link = links.iter().find(|l| l["rel"] == "self").expect("self link");
    assert_eq!(self_link["href"], "/api/opds/v2?lang=en", "self has no trailing slash");
    assert!(links.iter().any(|l| l["rel"] == "start"));
    let search = links.iter().find(|l| l["rel"] == "search").expect("search link");
    assert_eq!(search["templated"], true, "search is a URI template");
    assert_eq!(search["href"], "/api/opds/v2/library/series{?query,title,author}");

    // `feed.schema.json` routes unknown top-level keys to the subcollection
    // schema, so no JSON-LD context may appear here.
    assert!(feed.get("@context").is_none(), "OPDS feeds must not carry @context");
    assert_eq!(feed["metadata"]["numberOfItems"], 9);
    assert!(feed["metadata"].get("itemsPerPage").is_none(), "unpaged feed omits itemsPerPage");
}

#[tokio::test]
async fn empty_results_fall_back_to_navigation() {
    let db = Db::sqlite_in_memory().await.expect("connect sqlite");
    db.migrate().await.expect("migrate");

    let feed = value(&feeds::library_series_feed(&ctx(&db), None, None, None, None, None, 1, "title", "all").await);

    // `publications` is `minItems: 1` — an empty array would be invalid.
    assert!(feed.get("publications").is_none(), "empty result must not emit publications: []");
    assert_eq!(feed["metadata"]["numberOfItems"], 0);
    let navigation = array(&feed, "navigation");
    assert_eq!(navigation.len(), 1);
    assert_eq!(navigation[0]["rel"], "start");
    assert_eq!(navigation[0]["title"], "OPDS Catalog Root");
}

#[tokio::test]
async fn series_publications_acquire_indirectly() {
    let db = seed().await;
    let feed = value(&feeds::library_series_feed(&ctx(&db), None, None, None, None, None, 1, "title", "all").await);
    let publications = array(&feed, "publications");
    assert_eq!(publications.len(), 2);
    assert_eq!(publications[0]["metadata"]["title"], "Alpha Manga", "sorted by title");

    for publication in publications {
        let links = array(publication, "links");
        let acquisition = links
            .iter()
            .find(|l| ACQUISITION_RELS.contains(&l["rel"].as_str().unwrap_or_default()))
            .unwrap_or_else(|| panic!("publication {} has no acquisition link", publication["metadata"]["title"]));
        assert_eq!(acquisition["rel"], "http://opds-spec.org/acquisition");
        assert_eq!(acquisition["type"], "application/opds+json");
        let indirect = &acquisition["properties"]["indirectAcquisition"];
        assert_eq!(indirect[0]["type"], "application/divina+json", "chapters end in a Divina manifest");
        assert!(acquisition["href"].as_str().unwrap_or_default().contains("/chapters?lang=en"));
    }
}

#[tokio::test]
async fn pseudo_language_is_not_written() {
    let db = seed().await;
    let feed = value(&feeds::library_series_feed(&ctx(&db), None, None, None, None, None, 1, "title", "all").await);
    let publications = array(&feed, "publications");

    let alpha = publications.iter().find(|p| p["metadata"]["title"] == "Alpha Manga").expect("Alpha Manga");
    assert!(alpha["metadata"].get("language").is_none(), "`all` is not a language tag");

    let test_manga = publications.iter().find(|p| p["metadata"]["title"] == "Test Manga").expect("Test Manga");
    assert_eq!(test_manga["metadata"]["language"], "en");
    assert_eq!(test_manga["metadata"]["publisher"], "MangaDex");
    assert_eq!(array(test_manga, "images").len(), 1);
}

#[tokio::test]
async fn series_feed_links_are_unique() {
    let db = seed().await;
    let feed = value(&feeds::library_series_feed(&ctx(&db), None, None, None, None, None, 1, "title", "all").await);

    for collection in ["links", "navigation", "publications"] {
        if let Some(items) = feed[collection].as_array() {
            let rendered: Vec<String> = items.iter().map(ToString::to_string).collect();
            let mut sorted = rendered.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), rendered.len(), "{collection} must be uniqueItems");
        }
    }
}

#[tokio::test]
async fn chapter_publications_carry_state_and_series() {
    let db = seed().await;
    let feed = value(&feeds::series_chapters_feed(&ctx(&db), 1, 1, "number_asc", "all").await.expect("feed"));
    let publications = array(&feed, "publications");
    assert_eq!(publications.len(), 3);

    let first = publications.iter().find(|p| p["metadata"]["identifier"] == "urn:suwayomi:chapter:1").expect("ch1");
    // The 1.2 titles carry an "Unread"/"Downloaded" prefix; 2.0 keeps the
    // title clean and moves the state into the link properties.
    assert_eq!(first["metadata"]["title"], "Chapter 1");
    assert_eq!(first["metadata"]["numberOfPages"], 20);
    assert_eq!(first["metadata"]["belongsTo"]["series"]["name"], "Test Manga");
    assert_eq!(first["metadata"]["belongsTo"]["series"]["position"], 1.0);
    assert_eq!(array(first, "images")[0]["href"], "/api/v1/manga/1/chapter/1/page/0");

    let manifest =
        array(first, "links").iter().find(|l| l["type"] == "application/divina+json").expect("manifest link");
    assert_eq!(manifest["rel"], "http://opds-spec.org/acquisition/open-access");
    // The manifest path is keyed on the chapter id: `source_order` repeats
    // within a series, so it cannot address the later chapters.
    assert_eq!(manifest["href"], "/api/opds/v2/series/1/chapter/1/manifest");
    assert_eq!(manifest["properties"]["state"], "downloaded");
    assert_eq!(manifest["properties"]["readPages"], 20);
    assert!(manifest["properties"].get("readPage").is_none(), "unread chapters have no readPage");

    // A downloaded chapter also offers its CBZ.
    assert!(array(first, "links").iter().any(|l| l["type"] == "application/vnd.comicbook+zip"));

    let second = publications.iter().find(|p| p["metadata"]["identifier"] == "urn:suwayomi:chapter:2").expect("ch2");
    let manifest = array(second, "links").iter().find(|l| l["type"] == "application/divina+json").expect("manifest");
    assert_eq!(manifest["properties"]["state"], "in-progress");
    assert_eq!(manifest["properties"]["readPage"], 5);
    assert!(manifest["properties"]["readAt"].is_string());
}

#[tokio::test]
async fn unknown_page_count_omits_page_fields() {
    let db = seed().await;
    let feed = value(&feeds::series_chapters_feed(&ctx(&db), 1, 1, "number_asc", "all").await.expect("feed"));
    let third = array(&feed, "publications")
        .iter()
        .find(|p| p["metadata"]["identifier"] == "urn:suwayomi:chapter:3")
        .expect("ch3");
    // `numberOfPages` is `exclusiveMinimum: 0`, and an empty `images` array
    // violates its own `minItems: 1`.
    assert!(third["metadata"].get("numberOfPages").is_none());
    assert!(third.get("images").is_none());
    assert_eq!(third["metadata"]["title"], "Chapter 3", "unnamed chapter falls back to its number");
}

#[tokio::test]
async fn missing_series_is_not_found() {
    let db = seed().await;
    assert_eq!(feeds::series_chapters_feed(&ctx(&db), 99, 1, "number_asc", "all").await.err(), Some(V2Error::NotFound));
}

#[tokio::test]
async fn manifest_is_keyed_on_chapter_id_not_source_order() {
    let db = seed().await;
    // A second chapter sharing chapter 1's `source_order` — what the nhentai
    // family of extensions actually produces (one chapter per volume, every
    // row at `source_order = 0`). Keying the route on `source_order` makes
    // both chapters resolve to the first one.
    insert_chapter(&db, "/series/1/ch/4", "Chapter 1 (second volume)", 4.0, false, 0, 0, 1, false, 7).await;

    let first = value(&feeds::chapter_manifest(&ctx(&db), 1, 1).await.expect("chapter 1"));
    assert_eq!(first["metadata"]["numberOfPages"], 20);

    let second = value(&feeds::chapter_manifest(&ctx(&db), 1, 4).await.expect("chapter 4"));
    assert_eq!(second["metadata"]["identifier"], "urn:suwayomi:chapter:4");
    assert_eq!(second["metadata"]["numberOfPages"], 7, "the duplicate source_order must not shadow it");

    // The feed's manifest links therefore differ per chapter, and the
    // publications stay distinct objects (`uniqueItems`).
    let feed = value(&feeds::series_chapters_feed(&ctx(&db), 1, 1, "number_asc", "all").await.expect("feed"));
    let hrefs: Vec<&str> = array(&feed, "publications")
        .iter()
        .filter_map(|p| array(p, "links").iter().find(|l| l["type"] == "application/divina+json"))
        .filter_map(|link| link["href"].as_str())
        .collect();
    assert_eq!(hrefs.len(), 4);
    assert!(hrefs.contains(&"/api/opds/v2/series/1/chapter/1/manifest"));
    assert!(hrefs.contains(&"/api/opds/v2/series/1/chapter/4/manifest"));
}

#[tokio::test]
async fn manifest_lists_every_page() {
    let db = seed().await;
    let manifest = value(&feeds::chapter_manifest(&ctx(&db), 1, 1).await.expect("manifest"));

    assert_eq!(manifest["@context"], "http://readium.org/webpub-manifest/context.jsonld");
    assert_eq!(manifest["metadata"]["conformsTo"], "https://readium.org/webpub-manifest/profiles/divina");
    assert_eq!(manifest["metadata"]["numberOfPages"], 20);
    assert_eq!(manifest["metadata"]["belongsTo"]["series"]["name"], "Test Manga");

    let reading_order = array(&manifest, "readingOrder");
    assert_eq!(reading_order.len(), 20);
    assert!(reading_order.iter().all(|link| link["type"] == "image/jpeg"), "RWPM requires a type per entry");
    assert_eq!(reading_order[0]["href"], "/api/v1/manga/1/chapter/1/page/0?updateProgress=true&opds=true");
    assert_eq!(reading_order[19]["href"], "/api/v1/manga/1/chapter/1/page/19?updateProgress=true&opds=true");
    assert_eq!(array(&manifest, "links")[0]["rel"], "self");
    assert_eq!(array(&manifest, "links")[0]["type"], "application/divina+json");
}

#[tokio::test]
async fn unknown_page_count_has_no_manifest() {
    let db = seed().await;
    // Never answered with an empty readingOrder.
    assert_eq!(feeds::chapter_manifest(&ctx(&db), 1, 3).await.err(), Some(V2Error::PageCountUnknown));
    assert_eq!(feeds::chapter_manifest(&ctx(&db), 1, 99).await.err(), Some(V2Error::NotFound));
}

#[tokio::test]
async fn manifest_page_count_falls_back_to_the_source() {
    let db = seed().await;
    let (stub, backend) = PageListStub::backend(Some(9));
    let ctx = ctx_with(&db, &backend);

    // Chapter 3 is stored with `page_count = -1`, so the count has to come
    // from the source. Everything the count feeds (the reading order,
    // `numberOfPages`) has to agree with it.
    let manifest = value(&feeds::chapter_manifest(&ctx, 1, 3).await.expect("manifest"));
    assert_eq!(manifest["metadata"]["numberOfPages"], 9);
    assert_eq!(array(&manifest, "readingOrder").len(), 9);
    assert_eq!(
        array(&manifest, "readingOrder")[8]["href"],
        "/api/v1/manga/1/chapter/3/page/8?updateProgress=true&opds=true"
    );
    // The source address handed to the fetcher comes from the chapter's row.
    assert_eq!(stub.calls(), vec![(1, "/series/1".to_string(), "/series/1/ch/3".to_string())]);

    // A known count is never looked up: chapter 1 keeps its stored 20 instead
    // of the stub's 9, and the fetcher is not asked a second time.
    let known = value(&feeds::chapter_manifest(&ctx, 1, 1).await.expect("manifest"));
    assert_eq!(known["metadata"]["numberOfPages"], 20);
    assert_eq!(stub.calls().len(), 1, "a known page count must not be re-fetched");
}

#[tokio::test]
async fn empty_source_page_list_is_not_a_manifest() {
    let db = seed().await;
    let (_, backend) = PageListStub::backend(Some(0));
    let ctx = ctx_with(&db, &backend);

    // A source that answers with no pages is as unusable as one that fails.
    assert_eq!(feeds::chapter_manifest(&ctx, 1, 3).await.err(), Some(V2Error::PageCountUnknown));
}

#[tokio::test]
async fn source_failure_is_not_a_manifest() {
    let db = seed().await;
    let (_, backend) = PageListStub::backend(None);
    let ctx = ctx_with(&db, &backend);

    assert_eq!(feeds::chapter_manifest(&ctx, 1, 3).await.err(), Some(V2Error::PageCountUnknown));
}

/// A source listing for `/explore/source/{id}`: one remote manga, plus a flag
/// for whether the source offers a latest list.
struct BrowseStub {
    latest_supported: bool,
    calls: Mutex<Vec<&'static str>>,
}

impl BrowseStub {
    fn backend(latest_supported: bool) -> (Arc<Self>, SourceBackend) {
        let stub = Arc::new(Self { latest_supported, calls: Mutex::new(Vec::new()) });
        let shared: Arc<dyn SourceFetcher> = stub.clone();
        (stub, SourceBackend::Test(shared))
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().expect("stub mutex").clone()
    }

    fn listing(&self, kind: &'static str) -> MangasPage {
        self.calls.lock().expect("stub mutex").push(kind);
        MangasPage {
            mangas: vec![SManga {
                // Sources address their own pages by path, not by absolute URL.
                url: "/manga/1".into(),
                title: "Remote One".into(),
                author: Some("Author R".into()),
                description: Some("Remote blurb".into()),
                genre: Some("Action, Comedy".into()),
                thumbnail_url: Some("https://source.example/1.jpg".into()),
                ..Default::default()
            }],
            has_next_page: false,
        }
    }
}

#[async_trait::async_trait]
impl SourceFetcher for BrowseStub {
    async fn fetch_manga_update(
        &self,
        _source_id: i64,
        _manga: &SManga,
        _chapters: &[SChapter],
        _fetch_details: bool,
        _fetch_chapters: bool,
    ) -> suwayomi_domain::error::Result<(SManga, Vec<SChapter>)> {
        Err(DomainError::Source("unused in this stub".into()))
    }

    async fn get_popular_manga(&self, _source_id: i64, _page: u32) -> suwayomi_domain::error::Result<MangasPage> {
        Ok(self.listing("popular"))
    }

    async fn get_latest_updates(&self, _source_id: i64, _page: u32) -> suwayomi_domain::error::Result<MangasPage> {
        Ok(self.listing("latest"))
    }

    async fn search_manga(
        &self,
        _source_id: i64,
        _query: &str,
        _page: u32,
    ) -> suwayomi_domain::error::Result<MangasPage> {
        Ok(MangasPage::default())
    }

    fn supports_latest(&self, _source_id: i64) -> bool {
        self.latest_supported
    }
}

#[tokio::test]
async fn navigation_feeds_list_their_targets() {
    let db = seed().await;

    let feed = value(&feeds::explore_sources_feed(&ctx(&db)).await);
    assert_eq!(feed["metadata"]["title"], "Sources");
    assert!(feed.get("publications").is_none(), "navigation feed has no publications");
    let navigation = array(&feed, "navigation");
    assert_eq!(navigation.len(), 2);
    assert!(navigation.iter().all(|link| link["title"].is_string()));

    let mangadex = navigation.iter().find(|link| link["title"] == "MangaDex").expect("MangaDex");
    assert_eq!(mangadex["rel"], "subsection");
    assert_eq!(mangadex["type"], "application/opds+json");
    assert_eq!(mangadex["href"], "/api/opds/v2/explore/source/1?sort=popular&lang=en");

    // The per-source count rides on the link, not on the feed.
    let sources = value(&feeds::library_sources_feed(&ctx(&db)).await);
    let counted = array(&sources, "navigation").iter().find(|link| link["title"] == "MangaDex").expect("MangaDex");
    assert_eq!(counted["properties"]["numberOfItems"], 1);
    assert_eq!(counted["href"], "/api/opds/v2/source/1?lang=en");
}

#[tokio::test]
async fn empty_navigation_feed_falls_back_to_the_root() {
    let db = seed().await;
    // The seed has no categories: `navigation` is `minItems: 1` too.
    let feed = value(&feeds::categories_feed(&ctx(&db)).await);
    assert_eq!(feed["metadata"]["numberOfItems"], 0);
    let navigation = array(&feed, "navigation");
    assert_eq!(navigation.len(), 1);
    assert_eq!(navigation[0]["rel"], "start");
    assert_eq!(navigation[0]["title"], "OPDS Catalog Root");
}

#[tokio::test]
async fn remote_entries_acquire_the_source_page() {
    let db = seed().await;
    let (stub, backend) = BrowseStub::backend(false);
    let ctx = ctx_with(&db, &backend);

    let feed = value(&feeds::explore_source_feed(&ctx, 1, 1, "popular").await);
    assert_eq!(feed["metadata"]["title"], "Popular from MangaDex");
    assert_eq!(stub.calls(), vec!["popular"]);

    let publications = array(&feed, "publications");
    assert_eq!(publications.len(), 1);
    let publication = &publications[0];
    assert_eq!(publication["metadata"]["identifier"], "urn:suwayomi:remote:/manga/1");
    assert_eq!(publication["metadata"]["title"], "Remote One");
    assert_eq!(publication["metadata"]["publisher"], "MangaDex");
    assert_eq!(publication["metadata"]["subject"], serde_json::json!(["Action", "Comedy"]));
    assert_eq!(array(publication, "images")[0]["href"], "https://source.example/1.jpg");

    // Nothing on this server can serve a remote manga — no id, so no chapter
    // feed and no manifest. The source's own page is the only acquisition
    // there is; 1.2 emits this same entry with an empty href. The source's own
    // address is a path, so it has to be expanded against its base URL —
    // otherwise the link would resolve back to this server.
    let links = array(publication, "links");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["rel"], "http://opds-spec.org/acquisition");
    assert_eq!(links[0]["href"], "https://mangadex.org/manga/1");
    assert_eq!(links[0]["type"], "text/html");
    // Not a library series: there is no chapter feed to belong to.
    assert!(publication["metadata"].get("belongsTo").is_none());
}

#[tokio::test]
async fn source_addresses_are_expanded_against_the_base_url() {
    let db = seed().await;
    let feed = value(&feeds::library_series_feed(&ctx(&db), None, None, None, None, None, 1, "title", "all").await);
    let test_manga =
        array(&feed, "publications").iter().find(|p| p["metadata"]["title"] == "Test Manga").expect("Test Manga");
    let alternate = array(test_manga, "links").iter().find(|l| l["rel"] == "alternate").expect("alternate link");
    // The row holds `/series/1`; 1.2 writes that verbatim, so its "View on Web"
    // resolves against this server instead of the source.
    assert_eq!(alternate["href"], "https://mangadex.org/series/1");
}

#[tokio::test]
async fn latest_listing_is_used_only_when_the_source_has_one() {
    let db = seed().await;

    let (stub, backend) = BrowseStub::backend(true);
    let ctx = ctx_with(&db, &backend);
    let feed = value(&feeds::explore_source_feed(&ctx, 1, 1, "latest").await);
    assert_eq!(feed["metadata"]["title"], "Latest from MangaDex");
    assert_eq!(stub.calls(), vec!["latest"]);

    let (stub, backend) = BrowseStub::backend(false);
    let ctx = ctx_with(&db, &backend);
    let feed = value(&feeds::explore_source_feed(&ctx, 1, 1, "latest").await);
    // The title follows the request, but the listing falls back to popular.
    assert_eq!(feed["metadata"]["title"], "Latest from MangaDex");
    assert_eq!(stub.calls(), vec!["popular"]);
}

#[tokio::test]
async fn history_and_updates_feeds_list_chapters() {
    let db = seed().await;

    // Only chapter 2 has been read.
    let history = value(&feeds::history_feed(&ctx(&db), 1).await);
    let publications = array(&history, "publications");
    assert_eq!(publications.len(), 1);
    assert_eq!(history["metadata"]["itemsPerPage"], 50);
    assert_eq!(history["metadata"]["currentPage"], 1);

    let publication = &publications[0];
    // 1.2 writes "In Progress Test Manga: Chapter 2" — the series name lives
    // in `belongsTo` in 2.0, so the title stays clean.
    assert_eq!(publication["metadata"]["title"], "Chapter 2");
    assert_eq!(publication["metadata"]["belongsTo"]["series"]["name"], "Test Manga");
    let manifest =
        array(publication, "links").iter().find(|l| l["type"] == "application/divina+json").expect("manifest link");
    assert_eq!(manifest["properties"]["state"], "in-progress");

    let updates = value(&feeds::library_updates_feed(&ctx(&db), 1).await);
    assert_eq!(array(&updates, "publications").len(), 3);
    assert_eq!(updates["metadata"]["numberOfItems"], 3);
}

#[tokio::test]
async fn filtered_library_feed_names_its_filter() {
    let db = seed().await;
    let feed = value(&feeds::library_series_feed(&ctx(&db), Some(1), None, None, None, None, 1, "title", "all").await);

    assert_eq!(feed["metadata"]["title"], "Source: 1");
    assert_eq!(array(&feed, "publications").len(), 1);
    // Same redundant-but-harmless query param 1.2 emits: the filter is already
    // in the path, and the route ignores the duplicate.
    let self_link = array(&feed, "links").iter().find(|l| l["rel"] == "self").expect("self link");
    assert_eq!(self_link["href"], "/api/opds/v2/source/1?source_id=1&lang=en");
}
