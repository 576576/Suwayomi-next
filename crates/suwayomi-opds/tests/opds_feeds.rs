//! OPDS feed integration tests — run against an embedded PGlite (no
//! external PostgreSQL needed). Inserts seed rows, builds each feed, and
//! asserts the resulting Atom/OPDS XML.

use suwayomi_core::db::Db;
use suwayomi_domain::source::StubFetcher;
use suwayomi_opds::feeds::{self, FeedCtx};
use std::sync::Arc;

async fn seed() -> Db {
    let db = Db::connect_embedded(None).await.expect("connect embedded");
    db.migrate().await.expect("migrate");
    let pool = db.pool();

    // extension (FK target of source.extension — a violation would kill the
    // embedded session, since the PGlite proxy terminates on any SQL error)
    sqlx::query("INSERT INTO extension (name, pkg_name, version_name, version_code, lang, content_warning) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind("Test Extension")
        .bind("eu.test.pkg")
        .bind("1.0")
        .bind(1_i64)
        .bind("en")
        .bind(0_i32)
        .execute(pool)
        .await
        .expect("insert extension");

    // source
    sqlx::query("INSERT INTO source (name, lang, extension) VALUES ($1, $2, $3)")
        .bind("MangaDex")
        .bind("en")
        .bind(1_i32)
        .execute(pool)
        .await
        .expect("insert source");

    // manga (in library)
    sqlx::query(
        "INSERT INTO manga (url, title, initialized, artist, author, description, genre, status, thumbnail_url, in_library, source, last_fetched_at, last_modified_at) \
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
    .expect("insert manga");

    // chapters
    sqlx::query(
        "INSERT INTO chapter (url, name, date_upload, chapter_number, read, last_page_read, last_read_at, source_order, is_downloaded, page_count, manga) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind("/series/1/ch/1")
    .bind("Chapter 1")
    .bind(1_700_000_100_000_i64)
    .bind(1.0_f32)
    .bind(false)
    .bind(0_i32)
    .bind(0_i64)
    .bind(1_i32)
    .bind(true)
    .bind(20_i32)
    .bind(1_i32)
    .execute(pool)
    .await
    .expect("insert chapter 1");

    sqlx::query(
        "INSERT INTO chapter (url, name, date_upload, chapter_number, read, last_page_read, last_read_at, source_order, is_downloaded, page_count, manga) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind("/series/1/ch/2")
    .bind("Chapter 2")
    .bind(1_700_000_200_000_i64)
    .bind(2.0_f32)
    .bind(true)
    .bind(5_i32)
    .bind(1_700_000_500_000_i64)
    .bind(2_i32)
    .bind(false)
    .bind(15_i32)
    .bind(1_i32)
    .execute(pool)
    .await
    .expect("insert chapter 2");

    // category + membership
    sqlx::query("INSERT INTO category (name, sort_order) VALUES ($1, $2)")
        .bind("My Category")
        .bind(0_i32)
        .execute(pool)
        .await
        .expect("insert category");
    sqlx::query("INSERT INTO category_manga (category, manga) VALUES ($1, $2)")
        .bind(1_i32)
        .bind(1_i32)
        .execute(pool)
        .await
        .expect("insert category_manga");

    db
}

fn ctx(db: &Db) -> FeedCtx<'_> {
    FeedCtx {
        db,
        base_url: "/api/opds/v1.2",
        lang: "en",
        fetcher: Some(Arc::new(StubFetcher)),
    }
}

#[tokio::test]
async fn root_feed_is_navigation_xml() {
    let db = seed().await;
    let xml = feeds::root_feed(&ctx(&db)).await;
    assert!(xml.contains("<?xml"), "declaration");
    assert!(xml.contains("<feed"), "feed root");
    assert!(xml.contains("xmlns=\"http://www.w3.org/2005/Atom\""), "atom ns");
    assert!(xml.contains("xmlns:opds=\"http://opds-spec.org/2010/catalog\""), "opds ns");
    assert!(xml.contains("urn:suwayomi:navigation:root:"), "root entries");
    assert!(xml.contains("library/series"), "library link");
    assert!(xml.contains("kind=navigation"), "navigation type");
}

#[tokio::test]
async fn library_series_feed_contains_manga_entry() {
    let db = seed().await;
    let xml = feeds::library_series_feed(&ctx(&db), None, None, None, None, None, 1, "title", "all").await;
    assert!(xml.contains("urn:suwayomi:manga:1"), "manga entry id");
    assert!(xml.contains("Test Manga"), "manga title");
    assert!(xml.contains("urn:suwayomi:feed:library:series"), "feed id");
    assert!(xml.contains("opensearch:totalResults"), "totalResults");
    assert!(xml.contains("rel=\"self\""), "self link");
    assert!(xml.contains("rel=\"start\""), "start link");
    assert!(xml.contains("rel=\"search\""), "search link");
    assert!(xml.contains("Action"), "genre category");
}

#[tokio::test]
async fn series_chapters_feed_contains_chapters() {
    let db = seed().await;
    let xml = feeds::series_chapters_feed(&ctx(&db), 1, 1, "number_asc", "all").await.expect("feed");
    assert!(xml.contains("urn:suwayomi:chapter:1"), "chapter 1");
    assert!(xml.contains("urn:suwayomi:chapter:2"), "chapter 2");
    assert!(xml.contains("Chapter 1"), "chapter title");
    // default (skipMetadata=false): chapter entries link to the metadata feed
    assert!(xml.contains("series/1/chapter/1/metadata"), "metadata feed link");
    assert!(xml.contains("rel=\"http://opds-spec.org/image\""), "cover image");
}

#[tokio::test]
async fn history_feed_contains_read_chapters_only() {
    let db = seed().await;
    let xml = feeds::history_feed(&ctx(&db), 1).await;
    // chapter 2 was read (last_read_at > 0); chapter 1 was not
    assert!(xml.contains("urn:suwayomi:chapter:2"), "read chapter present");
    assert!(!xml.contains("urn:suwayomi:chapter:1"), "unread chapter absent");
}

#[tokio::test]
async fn navigation_feeds_render() {
    let db = seed().await;
    for xml in [
        feeds::categories_feed(&ctx(&db)).await,
        feeds::genres_feed(&ctx(&db)).await,
        feeds::statuses_feed(&ctx(&db)).await,
        feeds::languages_feed(&ctx(&db)).await,
        feeds::library_sources_feed(&ctx(&db)).await,
        feeds::explore_sources_feed(&ctx(&db)).await,
    ] {
        assert!(xml.contains("<feed"), "feed root");
        assert!(xml.contains("urn:suwayomi:navigation:"), "nav entry");
    }
    // library-updates is an ACQUISITION feed (chapter entries, not nav)
    let updates = feeds::library_updates_feed(&ctx(&db), 1).await;
    assert!(updates.contains("<feed"), "feed root");
    assert!(updates.contains("urn:suwayomi:chapter:"), "chapter entry");
    let genres = feeds::genres_feed(&ctx(&db)).await;
    assert!(genres.contains("Action"), "genre Action");
    let cats = feeds::categories_feed(&ctx(&db)).await;
    assert!(cats.contains("My Category"), "category name");
}

#[tokio::test]
async fn chapter_metadata_feed_renders() {
    let db = seed().await;
    let xml = feeds::chapter_metadata_feed(&ctx(&db), 1, 2).await.expect("feed");
    assert!(xml.contains("urn:suwayomi:chapter:2"), "chapter entry");
    assert!(xml.contains("Read"), "read status");
    let not_found = feeds::chapter_metadata_feed(&ctx(&db), 1, 99).await;
    assert!(not_found.is_err(), "missing chapter -> Err");
}

#[tokio::test]
async fn search_feed_renders() {
    let db = seed().await;
    let xml = feeds::search_feed(&ctx(&db), Some("test"), None, None, 1).await;
    assert!(xml.contains("Test Manga"), "search hit");
    assert!(xml.contains("Search Results"), "search title");
}
