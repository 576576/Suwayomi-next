//! Embedded PGlite integration tests — the default database backend.
//!
//! Unlike `db_pg.rs` these do NOT require an external PostgreSQL server:
//! `Db::connect_embedded(None)` boots an ephemeral in-process PGlite
//! (PostgreSQL 17 engine) and serves it over local TCP.

use sqlx::postgres::PgPool;
use suwayomi_core::db::{Db, DbMode};
use suwayomi_core::schema::{ChapterRow, MangaRow, PageRow};

async fn setup() -> (Db, PgPool) {
    let db = Db::connect_embedded(None).await.expect("connect embedded pglite");
    assert_eq!(db.mode(), DbMode::Embedded);
    db.migrate().await.expect("migrate");
    let pool = db.pool().clone();
    (db, pool)
}

#[tokio::test]
async fn embedded_runs_in_embedded_mode() {
    let (db, _pool) = setup().await;
    assert_eq!(db.mode(), DbMode::Embedded);
}

#[tokio::test]
async fn migration_creates_all_tables() {
    let (_db, pool) = setup().await;
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
        // `to_regclass` returns NULL for a missing relation instead of
        // raising 42P01 — an error would terminate the embedded session
        // (the PGlite proxy closes the connection on any SQL error).
        let row: Option<String> = sqlx::query_scalar("SELECT to_regclass($1)::text")
            .bind(format!("suwayomi.{table}"))
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| panic!("query table {table}: {e}"));
        assert!(row.is_some(), "table {table} must exist after migration");
    }
}

#[tokio::test]
async fn search_path_is_set_on_connections() {
    // The after_connect hook must put `suwayomi` first in search_path, so
    // unqualified queries resolve without a schema prefix.
    let (_db, pool) = setup().await;
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM manga").fetch_one(&pool).await.expect("unqualified query");
    assert_eq!(row.0, 0);
}

#[tokio::test]
async fn manga_chapter_page_roundtrip() {
    let (_db, pool) = setup().await;

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

#[tokio::test]
async fn persistent_data_dir_survives_reopen() {
    // A persistent data dir must keep data across server restarts.
    let dir = std::env::temp_dir().join(format!("suwayomi-pglite-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let (db, pool) = {
        let db = Db::connect_embedded(Some(&dir)).await.expect("open persistent");
        db.migrate().await.expect("migrate");
        let pool = db.pool().clone();
        sqlx::query("INSERT INTO suwayomi.manga (url, title, source) VALUES ($1, $2, $3)")
            .bind("/persist/1")
            .bind("Persistent Manga")
            .bind(1_i64)
            .execute(&pool)
            .await
            .expect("insert");
        (db, pool)
    };
    drop(db);
    drop(pool);

    // Reopen the same directory — the row must still be there.
    let db2 = Db::connect_embedded(Some(&dir)).await.expect("reopen persistent");
    let pool2 = db2.pool().clone();
    let manga: MangaRow = sqlx::query_as("SELECT * FROM suwayomi.manga WHERE url = '/persist/1'")
        .fetch_one(&pool2)
        .await
        .expect("fetch persisted manga");
    assert_eq!(manga.title, "Persistent Manga");

    let _ = std::fs::remove_dir_all(&dir);
}
