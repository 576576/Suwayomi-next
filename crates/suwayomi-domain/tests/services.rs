//! Phase 2 integration tests — service-level port of the Kotlin tests
//! (MangaTest / CategoryMangaTest / library & category behavior) against a
//! PostgreSQL database with the migration baseline applied.
//!
//! Requires `DATABASE_URL` (e.g. postgres://postgres:postgres@localhost:5432/postgres);
//! skipped when absent.

use std::collections::HashMap;
use std::sync::Arc;

use sqlx::postgres::PgPool;
use suwayomi_core::db::Db;
use suwayomi_core::models::{IncludeOrExclude, MangaStatus, PaginatedList, UpdateStrategy};
use suwayomi_core::source::SManga;
use suwayomi_domain::category::CategoryService;
use suwayomi_domain::category::category_manga::CategoryMangaService;
use suwayomi_domain::chapter::ChapterService;
use suwayomi_domain::manga::MangaService;
use suwayomi_domain::manga::library::LibraryService;
use suwayomi_domain::manga::manga_list::MangaListService;
use suwayomi_domain::source::StubFetcher;

const BUSINESS_TABLES: &[&str] = &[
    "track_search",
    "track_record",
    "extension_store",
    "global_meta",
    "source_meta",
    "manga_meta",
    "chapter_meta",
    "category_meta",
    "category_manga",
    "page",
    "chapter",
    "manga",
    "category",
    "source",
    "extension",
];

type Services = (Db, MangaService, ChapterService, CategoryService, CategoryMangaService, LibraryService);

async fn setup() -> Option<Services> {
    let url = std::env::var("DATABASE_URL").or_else(|_| std::env::var("SUWAYOMI_TEST_DB")).ok()?;
    let db = Db::connect(&url).await.expect("connect postgres");
    db.migrate().await.expect("migrate");
    let pool = db.pool();
    for t in BUSINESS_TABLES {
        let _ = sqlx::query(&format!("TRUNCATE TABLE suwayomi.{t} RESTART IDENTITY CASCADE")).execute(pool).await;
    }

    let fetcher: Arc<dyn suwayomi_domain::source::SourceFetcher> = Arc::new(StubFetcher);
    let manga = MangaService::new(db.clone(), fetcher.clone());
    let chapter = ChapterService::new(db.clone(), fetcher.clone());
    let category = CategoryService::new(db.clone());
    let cm = CategoryMangaService::new(db.clone());
    let library = LibraryService::new(db.clone(), manga.clone());
    Some((db, manga, chapter, category, cm, library))
}

fn pool(db: &Db) -> &PgPool {
    db.pool()
}

/// createLibraryManga equivalent
async fn create_library_manga(db: &Db, title: &str, source_id: i64) -> i32 {
    let (id,): (i32,) = sqlx::query_as(
        "INSERT INTO suwayomi.manga (url, title, source, initialized, in_library, in_library_at) VALUES ($1, $2, $3, TRUE, TRUE, 1) RETURNING id",
    )
    .bind(format!("/manga/{title}"))
    .bind(title)
    .bind(source_id)
    .fetch_one(pool(db))
    .await
    .expect("insert manga");
    id
}

/// createChapters equivalent: n chapters with `read` flag; optional start index.
async fn create_chapters(db: &Db, manga_id: i32, count: i32, read: bool, start: i32) {
    for i in start..start + count {
        sqlx::query(
            "INSERT INTO suwayomi.chapter (url, name, date_upload, chapter_number, read, bookmark, last_page_read, last_read_at, fetched_at, source_order, is_downloaded, page_count, manga) VALUES ($1, $2, 0, $3, $4, FALSE, 0, 0, 0, $5, FALSE, -1, $6)",
        )
        .bind(format!("/chapter/{i}"))
        .bind(format!("Chapter {i}"))
        .bind(i as f32)
        .bind(read)
        .bind(i)
        .bind(manga_id)
        .execute(pool(db))
        .await
        .expect("insert chapter");
    }
}

#[tokio::test]
async fn manga_meta_upsert_matches_kotlin() {
    let Some((db, manga, _, _, _, _)) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };
    let id = create_library_manga(&db, "Meta", 1).await;

    let empty = manga.get_meta_map(id).await.unwrap();
    assert_eq!(empty.len(), 0, "Default Manga meta should be empty at start");

    let mut m = HashMap::new();
    m.insert(id, HashMap::from([("test".to_string(), "value".to_string())]));
    manga.modify_metas(&m).await.unwrap();

    let map = manga.get_meta_map(id).await.unwrap();
    assert_eq!(map.len(), 1, "Manga meta should have one member");
    assert_eq!(map.get("test").map(|s| s.as_str()), Some("value"));

    // update existing key
    let mut m = HashMap::new();
    m.insert(id, HashMap::from([("test".to_string(), "v2".to_string())]));
    manga.modify_metas(&m).await.unwrap();
    let map = manga.get_meta_map(id).await.unwrap();
    assert_eq!(map.len(), 1);
    assert_eq!(map.get("test").map(|s| s.as_str()), Some("v2"));
}

#[tokio::test]
async fn manga_full_counts_and_library_flow() {
    let Some((db, manga, _, category, cm, library)) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };
    let id = create_library_manga(&db, "Psyren", 1).await;
    create_chapters(&db, id, 10, true, 1).await;

    let full = manga.get_manga_full(id, false).await.unwrap();
    assert_eq!(full.unread_count, Some(0));
    assert_eq!(full.chapter_count, Some(10));
    assert_eq!(full.download_count, Some(0));

    create_chapters(&db, id, 10, false, 11).await;
    let full = manga.get_manga_full(id, false).await.unwrap();
    assert_eq!(full.unread_count, Some(10));

    // category flow: default category initially holds the library manga
    assert_eq!(cm.get_category_manga_list(CategoryService::DEFAULT_CATEGORY_ID).await.unwrap().len(), 1);
    let cat_id = category.create_categories(&["Old".to_string()]).await.unwrap()[0];
    assert_eq!(cm.get_category_manga_list(cat_id).await.unwrap().len(), 0);

    cm.add_mangas_to_categories(&[id], &[cat_id]).await.unwrap();
    let list = cm.get_category_manga_list(cat_id).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].unread_count, Some(10));

    let dc = manga.get_manga(id, false).await.unwrap();
    assert!(dc.in_library);
    library.remove_manga_from_library(id).await.unwrap();
    let dc = manga.get_manga(id, false).await.unwrap();
    assert!(!dc.in_library);
    library.add_manga_to_library(id).await.unwrap();
    let dc = manga.get_manga(id, false).await.unwrap();
    assert!(dc.in_library);
}

#[tokio::test]
async fn duplicate_category_manga_pairing_rejected_by_constraint() {
    let Some((db, _, _, category, cm, _)) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };
    let manga_id = create_library_manga(&db, "Naruto", 1).await;
    let cat_id = category.create_categories(&["Shonen".to_string()]).await.unwrap()[0];

    cm.add_mangas_to_categories(&[manga_id], &[cat_id]).await.unwrap();
    // app layer dedupes; direct insert must fail on the unique constraint
    let res = sqlx::query("INSERT INTO suwayomi.category_manga (category, manga) VALUES ($1, $2)")
        .bind(cat_id)
        .bind(manga_id)
        .execute(pool(&db))
        .await;
    assert!(res.is_err(), "unique constraint must reject duplicate pairing");

    let row_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM suwayomi.category_manga WHERE manga = $1 AND category = $2")
            .bind(manga_id)
            .bind(cat_id)
            .fetch_one(pool(&db))
            .await
            .unwrap();
    assert_eq!(row_count, 1, "Only one row should exist for a given pairing");
}

#[tokio::test]
async fn adding_manga_twice_does_not_create_duplicates() {
    let Some((db, _, _, category, cm, _)) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };
    let manga_id = create_library_manga(&db, "One Piece", 1).await;
    let cat_id = category.create_categories(&["Adventure".to_string()]).await.unwrap()[0];

    cm.add_mangas_to_categories(&[manga_id], &[cat_id]).await.unwrap();
    cm.add_mangas_to_categories(&[manga_id], &[cat_id]).await.unwrap();

    let row_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM suwayomi.category_manga WHERE manga = $1 AND category = $2")
            .bind(manga_id)
            .bind(cat_id)
            .fetch_one(pool(&db))
            .await
            .unwrap();
    assert_eq!(row_count, 1);
    assert_eq!(cm.get_category_manga_list(cat_id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn category_create_filters_default_name_and_dedupes() {
    let Some((db, _, _, category, _, _)) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };
    let _ = create_library_manga(&db, "A", 1).await;

    let ids =
        category.create_categories(&["Default".to_string(), "Manga".to_string(), "manga".to_string()]).await.unwrap();
    assert_eq!(ids[0], CategoryService::DEFAULT_CATEGORY_ID);
    assert_eq!(ids[1], ids[2], "duplicate names (case-insensitive) must map to the same id");

    let list = category.get_category_list().await.unwrap();
    let names: Vec<&str> = list.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Manga"));
    assert!(!names.contains(&"Default"), "default category hidden when not needed");
}

#[tokio::test]
async fn chapter_list_sorting_and_modify() {
    let Some((db, _, chapter, _, _, _)) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };
    let manga_id = create_library_manga(&db, "Chapters", 1).await;
    create_chapters(&db, manga_id, 5, false, 1).await;

    let list = chapter.get_chapter_list(manga_id, false).await.unwrap();
    assert_eq!(list.len(), 5);
    assert_eq!(list[0].index, 5);
    assert_eq!(list[4].index, 1);

    let id = chapter.modify_chapter(manga_id, 3, Some(true), Some(true), None, Some(2)).await.unwrap();
    let row = chapter.fetch_by_id(id).await.unwrap();
    assert!(row.read);
    assert!(row.bookmark);
    assert_eq!(row.last_page_read, 2);
    assert!(row.last_read_at > 0);

    chapter.modify_chapter(manga_id, 5, None, None, Some(true), None).await.unwrap();
    let all = chapter.get_chapter_list(manga_id, false).await.unwrap();
    for c in all {
        if c.index < 5 {
            assert!(c.read, "chapters before index 5 should be marked read");
        }
    }

    // delete a chapter download → is_downloaded cleared, row stays (Kotlin semantics)
    sqlx::query("UPDATE suwayomi.chapter SET is_downloaded = TRUE WHERE manga = $1 AND source_order = $2")
        .bind(manga_id)
        .bind(1)
        .execute(pool(&db))
        .await
        .unwrap();
    chapter.delete_chapter(manga_id, 1).await.unwrap();
    let all = chapter.get_chapter_list(manga_id, false).await.unwrap();
    assert_eq!(all.len(), 5, "deleteChapter clears download state, not the row");
    let row = chapter.fetch_by_id(all[4].id).await.unwrap();
    assert!(!row.is_downloaded, "deleted chapter download must be flagged as not downloaded");
}

#[tokio::test]
async fn update_chapter_progress_marks_read_at_last_page() {
    let Some((db, _, chapter, _, _, _)) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };
    let manga_id = create_library_manga(&db, "Progress", 1).await;
    sqlx::query("INSERT INTO suwayomi.chapter (url, name, source_order, page_count, manga) VALUES ($1, $2, $3, 3, $4)")
        .bind("/c/1")
        .bind("C1")
        .bind(1)
        .bind(manga_id)
        .execute(pool(&db))
        .await
        .unwrap();
    let id = chapter.update_chapter_progress(manga_id, 1, 2).await.unwrap();
    let row = chapter.fetch_by_id(id).await.unwrap();
    assert!(row.read);
    assert_eq!(row.last_page_read, 2);
}

#[tokio::test]
async fn recent_chapters_requires_library_membership() {
    let Some((db, _, chapter, _, _, _)) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };
    sqlx::query(
        "INSERT INTO suwayomi.manga (url, title, source, in_library, in_library_at) VALUES ($1, $2, 1, FALSE, 0)",
    )
    .bind("/m/notlib")
    .bind("NotLib")
    .execute(pool(&db))
    .await
    .unwrap();
    sqlx::query("INSERT INTO suwayomi.chapter (url, name, source_order, fetched_at, manga) VALUES ($1, $2, 1, 999, 1)")
        .bind("/c/1")
        .bind("C1")
        .execute(pool(&db))
        .await
        .unwrap();
    let page: PaginatedList<suwayomi_core::models::MangaChapterDataClass> =
        chapter.get_recent_chapters(0).await.unwrap();
    assert!(page.page.is_empty(), "non-library manga chapters must not appear");
}

#[tokio::test]
async fn manga_list_insert_or_update_dedupes_and_updates() {
    let Some((db, _, _, _, _, _)) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };
    let fetcher: Arc<dyn suwayomi_domain::source::SourceFetcher> = Arc::new(StubFetcher);
    let list_svc = MangaListService::new(db.clone(), fetcher);

    let s = |url: &str, title: &str| SManga {
        url: url.to_string(),
        title: title.to_string(),
        status: MangaStatus::Ongoing.to_i32(),
        update_strategy: UpdateStrategy::AlwaysUpdate,
        ..Default::default()
    };

    let ids1 = list_svc.insert_or_update(1, &[s("/m/1", "One"), s("/m/2", "Two")]).await.unwrap();
    assert_eq!(ids1.len(), 2);

    let ids2 = list_svc.insert_or_update(1, &[s("/m/1", "One v2"), s("/m/3", "Three")]).await.unwrap();
    assert_eq!(ids2.len(), 2);
    assert_eq!(ids2[0], ids1[0]);

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM suwayomi.manga").fetch_one(pool(&db)).await.unwrap();
    assert_eq!(count, 3, "no duplicate manga rows for repeated source urls");

    let title: String =
        sqlx::query_scalar("SELECT title FROM suwayomi.manga WHERE url = '/m/1'").fetch_one(pool(&db)).await.unwrap();
    assert_eq!(title, "One v2", "existing non-library manga title should be updated");
}

#[tokio::test]
async fn category_reorder_and_update() {
    let Some((db, _, _, category, _, _)) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };
    let _ = create_library_manga(&db, "R", 1).await;
    let a = category.create_categories(&["A".to_string(), "B".to_string(), "C".to_string()]).await.unwrap();
    category.normalize_categories().await.unwrap();

    category.update_category(a[1], Some("B2".to_string()), None, Some(1), Some(0)).await.unwrap();
    let b = category.get_category_by_id(a[1]).await.unwrap().unwrap();
    assert_eq!(b.name, "B2");
    assert_eq!(b.include_in_update, IncludeOrExclude::Include);
    assert_eq!(b.include_in_download, IncludeOrExclude::Exclude);

    category.reorder_category(1, 3).await.unwrap();
    let list = category.get_category_list().await.unwrap();
    assert_eq!(list[2].id, a[0], "category A should now be last");
}
