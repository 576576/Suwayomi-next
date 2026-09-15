//! PostgreSQL integration tests — migration baseline + basic CRUD.
//!
//! The same contract as `db_sqlite.rs`, against the alternative backend.
//! Requires a running PostgreSQL and `DATABASE_URL` (e.g.
//! `postgres://postgres:postgres@localhost:5432/postgres`).
//! Tests are skipped when the env var is absent.

use suwayomi_core::db::{BackendKind, Db};
use suwayomi_core::schema::{ChapterRow, MangaRow, PageRow};

fn db_url() -> Option<String> {
    std::env::var("DATABASE_URL").or_else(|_| std::env::var("SUWAYOMI_TEST_DB")).ok()
}

/// Serialises the tests in this binary: they all talk to the same database and
/// every `setup()` truncates the tables the others are working on.
async fn lock() -> suwayomi_db::test_support::DbLock {
    suwayomi_db::test_support::db_lock().await
}

async fn setup() -> Option<Db> {
    let url = db_url()?;
    let db = Db::postgres(&url).await.expect("connect postgres");
    db.migrate().await.expect("migrate");
    assert_eq!(db.kind(), BackendKind::Postgres);
    // clear business tables to keep tests independent
    for t in [
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
    ] {
        let _ = suwayomi_db::query(&format!("TRUNCATE TABLE suwayomi.{t} RESTART IDENTITY CASCADE")).execute(&db).await;
    }
    Some(db)
}

#[tokio::test]
async fn migration_creates_all_tables() {
    let _guard = lock().await;
    let Some(db) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };
    let expected: &[&str] = &[
        "extension",
        "source",
        "manga",
        "chapter",
        "page",
        "category",
        "category_manga",
        "category_meta",
        "chapter_meta",
        "manga_meta",
        "source_meta",
        "global_meta",
        "extension_store",
        "track_record",
        "track_search",
    ];
    for table in expected {
        let row: (i64,) = suwayomi_db::query_as(
            "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'suwayomi' AND table_name = $1",
        )
        .bind(*table)
        .fetch_one(&db)
        .await
        .unwrap_or_else(|e| panic!("query table {table}: {e}"));
        assert_eq!(row.0, 1, "table {table} must exist after migration");
    }
}

#[tokio::test]
async fn manga_chapter_page_roundtrip() {
    let _guard = lock().await;
    let Some(db) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };

    suwayomi_db::query("INSERT INTO suwayomi.manga (url, title, source) VALUES ($1, $2, $3)")
        .bind("/manga/1")
        .bind("Test Manga")
        .bind(1_i64)
        .execute(&db)
        .await
        .expect("insert manga");

    let manga: MangaRow = suwayomi_db::query_as("SELECT * FROM suwayomi.manga WHERE id = 1")
        .fetch_one(&db)
        .await
        .expect("fetch manga");
    assert_eq!(manga.title, "Test Manga");
    assert_eq!(manga.source, 1);
    assert!(!manga.in_library);
    assert_eq!(manga.update_strategy, "ALWAYS_UPDATE");

    suwayomi_db::query("INSERT INTO suwayomi.chapter (url, name, source_order, manga) VALUES ($1, $2, $3, $4)")
        .bind("/chapter/1")
        .bind("Chapter 1")
        .bind(1_i32)
        .bind(manga.id)
        .execute(&db)
        .await
        .expect("insert chapter");

    let chapter: ChapterRow = suwayomi_db::query_as("SELECT * FROM suwayomi.chapter WHERE id = 1")
        .fetch_one(&db)
        .await
        .expect("fetch chapter");
    assert_eq!(chapter.name, "Chapter 1");
    assert_eq!(chapter.chapter_number, -1.0);
    assert_eq!(chapter.manga, manga.id);

    suwayomi_db::query("INSERT INTO suwayomi.page (\"index\", url, image_url, chapter) VALUES ($1, $2, $3, $4)")
        .bind(0_i32)
        .bind("/page/0")
        .bind("https://example.com/img.jpg")
        .bind(chapter.id)
        .execute(&db)
        .await
        .expect("insert page");

    let page: PageRow = suwayomi_db::query_as("SELECT * FROM suwayomi.page WHERE id = 1")
        .fetch_one(&db)
        .await
        .expect("fetch page");
    assert_eq!(page.index, 0);
    assert_eq!(page.chapter, chapter.id);
}

/// The `= ANY($1)` form survives unchanged on PostgreSQL.
#[tokio::test]
async fn array_parameter_uses_any() {
    let _guard = lock().await;
    let Some(db) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };
    for (id, title) in [(1_i64, "A"), (2, "B"), (3, "C")] {
        suwayomi_db::query("INSERT INTO suwayomi.manga (id, url, title, source) VALUES ($1, $2, $3, $4)")
            .bind(id)
            .bind(format!("/manga/{id}"))
            .bind(title)
            .bind(1_i64)
            .execute(&db)
            .await
            .expect("insert");
    }

    let rows: Vec<MangaRow> = suwayomi_db::query_as("SELECT * FROM suwayomi.manga WHERE id = ANY($1) ORDER BY id")
        .bind(vec![1_i32, 3_i32])
        .fetch_all(&db)
        .await
        .expect("array query");
    let titles: Vec<&str> = rows.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["A", "C"]);
}
