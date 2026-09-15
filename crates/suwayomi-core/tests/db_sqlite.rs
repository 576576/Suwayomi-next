//! SQLite integration tests — the default database backend.
//!
//! No external server is required: `Db::sqlite_in_memory()` opens a throw-away
//! database and `Db::sqlite(path)` a persistent file. The PostgreSQL side of the
//! same contract lives in `db_pg.rs`.

use suwayomi_core::db::{BackendKind, Db};
use suwayomi_core::schema::{ChapterRow, MangaRow, PageRow};

const TABLES: &[&str] = &[
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

async fn setup() -> Db {
    let db = Db::sqlite_in_memory().await.expect("open in-memory sqlite");
    assert_eq!(db.kind(), BackendKind::Sqlite);
    db.migrate().await.expect("migrate");
    db
}

#[tokio::test]
async fn in_memory_runs_on_the_sqlite_backend() {
    let db = setup().await;
    assert_eq!(db.kind(), BackendKind::Sqlite);
}

#[tokio::test]
async fn migration_creates_all_tables() {
    let db = setup().await;
    for table in TABLES {
        let count: i64 =
            suwayomi_db::query_scalar("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?")
                .bind(*table)
                .fetch_one(&db)
                .await
                .unwrap_or_else(|e| panic!("query table {table}: {e}"));
        assert_eq!(count, 1, "table {table} must exist after migration");
    }
}

#[tokio::test]
async fn migration_is_idempotent() {
    let db = setup().await;
    // Running the migrator twice must not fail: every statement is guarded and
    // the tracking table remembers what already ran.
    db.migrate().await.expect("second migrate");
    let count: i64 = suwayomi_db::query_scalar("SELECT count(*) FROM manga").fetch_one(&db).await.expect("select");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn manga_chapter_page_roundtrip() {
    let db = setup().await;

    suwayomi_db::query("INSERT INTO manga (url, title, source) VALUES (?, ?, ?)")
        .bind("/manga/1")
        .bind("Test Manga")
        .bind(1_i64)
        .execute(&db)
        .await
        .expect("insert manga");

    let manga: MangaRow = suwayomi_db::query_as("SELECT * FROM manga WHERE id = 1")
        .fetch_one(&db)
        .await
        .expect("fetch manga");
    assert_eq!(manga.title, "Test Manga");
    assert_eq!(manga.source, 1);
    assert!(!manga.in_library);
    assert_eq!(manga.update_strategy, "ALWAYS_UPDATE");

    suwayomi_db::query("INSERT INTO chapter (url, name, source_order, manga) VALUES (?, ?, ?, ?)")
        .bind("/chapter/1")
        .bind("Chapter 1")
        .bind(1_i32)
        .bind(manga.id)
        .execute(&db)
        .await
        .expect("insert chapter");

    let chapter: ChapterRow = suwayomi_db::query_as("SELECT * FROM chapter WHERE id = 1")
        .fetch_one(&db)
        .await
        .expect("fetch chapter");
    assert_eq!(chapter.name, "Chapter 1");
    assert_eq!(chapter.chapter_number, -1.0);
    assert_eq!(chapter.manga, manga.id);

    // `index` is a SQLite keyword — the column must be quoted here and in the
    // three handwritten queries in `download.rs`.
    suwayomi_db::query("INSERT INTO page (\"index\", url, image_url, chapter) VALUES (?, ?, ?, ?)")
        .bind(0_i32)
        .bind("/page/0")
        .bind("https://example.com/img.jpg")
        .bind(chapter.id)
        .execute(&db)
        .await
        .expect("insert page");

    let page: PageRow = suwayomi_db::query_as("SELECT * FROM page WHERE id = 1")
        .fetch_one(&db)
        .await
        .expect("fetch page");
    assert_eq!(page.index, 0);
    assert_eq!(page.chapter, chapter.id);
}

/// `= ANY($1)` is rewritten to `IN (?, ?, …)` on SQLite (see `suwayomi-db::dialect`).
#[tokio::test]
async fn array_parameter_expands_into_an_in_list() {
    let db = setup().await;
    for (id, title) in [(1_i64, "A"), (2, "B"), (3, "C")] {
        suwayomi_db::query("INSERT INTO manga (id, url, title, source) VALUES (?, ?, ?, ?)")
            .bind(id)
            .bind(format!("/manga/{id}"))
            .bind(title)
            .bind(1_i64)
            .execute(&db)
            .await
            .expect("insert");
    }

    let rows: Vec<MangaRow> = suwayomi_db::query_as("SELECT * FROM manga WHERE id = ANY(?) ORDER BY id")
        .bind(vec![1_i32, 3_i32])
        .fetch_all(&db)
        .await
        .expect("array query");
    let titles: Vec<&str> = rows.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["A", "C"]);
}

#[tokio::test]
async fn persistent_file_survives_reopen() {
    let dir = std::env::temp_dir().join(format!("suwayomi-sqlite-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("suwayomi.db");

    {
        let db = Db::sqlite(&file).await.expect("open persistent");
        db.migrate().await.expect("migrate");
        suwayomi_db::query("INSERT INTO manga (url, title, source) VALUES (?, ?, ?)")
            .bind("/persist/1")
            .bind("Persistent Manga")
            .bind(1_i64)
            .execute(&db)
            .await
            .expect("insert");
        db.close().await.expect("close");
    }

    // Reopen the same file — the row must still be there.
    let db = Db::sqlite(&file).await.expect("reopen persistent");
    let manga: MangaRow = suwayomi_db::query_as("SELECT * FROM manga WHERE url = ?")
        .bind("/persist/1")
        .fetch_one(&db)
        .await
        .expect("fetch persisted manga");
    assert_eq!(manga.title, "Persistent Manga");
    db.close().await.expect("close");

    let _ = std::fs::remove_dir_all(&dir);
}
