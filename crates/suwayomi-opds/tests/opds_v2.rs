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

use serde_json::Value;
use suwayomi_core::db::Db;
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

    for (name, lang) in [("MangaDex", "en"), ("Everylang", "all")] {
        suwayomi_db::query("INSERT INTO source (name, lang, extension) VALUES ($1, $2, $3)")
            .bind(name)
            .bind(lang)
            .bind(1_i32)
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

fn ctx(db: &Db) -> V2Ctx<'_> {
    V2Ctx { db, base_url: "/api/opds/v2", lang: "en" }
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
