//! PostgreSQL integration tests — migration baseline + basic CRUD.
//!
//! Requires a running PostgreSQL and `DATABASE_URL` (e.g.
//! `postgres://postgres:postgres@localhost:5432/postgres`).
//! Tests are skipped when the env var is absent.

use sqlx::postgres::PgPool;
use suwayomi_core::db::Db;
use suwayomi_core::schema::{ChapterRow, MangaRow, PageRow};

fn db_url() -> Option<String> {
    std::env::var("DATABASE_URL").or_else(|_| std::env::var("SUWAYOMI_TEST_DB")).ok()
}

async fn setup() -> Option<(Db, PgPool)> {
    let url = db_url()?;
    let db = Db::connect(&url).await.expect("connect postgres");
    db.migrate().await.expect("migrate");
    let pool = db.pool().clone();
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
        let _ = sqlx::query(&format!("TRUNCATE TABLE suwayomi.{t} RESTART IDENTITY CASCADE")).execute(&pool).await;
    }
    Some((db, pool))
}

#[tokio::test]
async fn migration_creates_all_tables() {
    let Some((_db, pool)) = setup().await else {
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
        let row: (i64,) = sqlx::query_as(
            "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'suwayomi' AND table_name = $1",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| panic!("query table {table}: {e}"));
        assert_eq!(row.0, 1, "table {table} must exist after migration");
    }
}

#[tokio::test]
async fn manga_chapter_page_roundtrip() {
    let Some((_db, pool)) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };

    sqlx::query("INSERT INTO suwayomi.manga (url, title, source) VALUES ($1, $2, $3)")
        .bind("/manga/1")
        .bind("Test Manga")
        .bind(1_i64)
        .execute(&pool)
        .await
        .expect("insert manga");

    let manga: MangaRow =
        sqlx::query_as("SELECT * FROM suwayomi.manga WHERE id = 1").fetch_one(&pool).await.expect("fetch manga");
    assert_eq!(manga.title, "Test Manga");
    assert_eq!(manga.source, 1);
    assert!(!manga.in_library);
    assert_eq!(manga.update_strategy, "ALWAYS_UPDATE");

    sqlx::query("INSERT INTO suwayomi.chapter (url, name, source_order, manga) VALUES ($1, $2, $3, $4)")
        .bind("/chapter/1")
        .bind("Chapter 1")
        .bind(1_i32)
        .bind(manga.id)
        .execute(&pool)
        .await
        .expect("insert chapter");

    let chapter: ChapterRow =
        sqlx::query_as("SELECT * FROM suwayomi.chapter WHERE id = 1").fetch_one(&pool).await.expect("fetch chapter");
    assert_eq!(chapter.name, "Chapter 1");
    assert_eq!(chapter.chapter_number, -1.0);
    assert_eq!(chapter.manga, manga.id);

    sqlx::query("INSERT INTO suwayomi.page (index, url, image_url, chapter) VALUES ($1, $2, $3, $4)")
        .bind(0_i32)
        .bind("/page/0")
        .bind("https://example.com/img.jpg")
        .bind(chapter.id)
        .execute(&pool)
        .await
        .expect("insert page");

    let page: PageRow =
        sqlx::query_as("SELECT * FROM suwayomi.page WHERE id = 1").fetch_one(&pool).await.expect("fetch page");
    assert_eq!(page.index, 0);
    assert_eq!(page.chapter, chapter.id);
}
