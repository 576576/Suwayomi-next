//! Business logic layer — mirrors `suwayomi.manga.impl.*`.
//! Phase 2 scope: database-backed services (Manga / Chapter / Page / Library /
//! MangaList / Category / CategoryManga / Meta). Source-fetching paths are
//! behind the `SourceFetcher` trait and get a real implementation in Phase 5.

pub mod category;
pub mod download;
pub mod extension_store;
pub mod koreader_sync;
pub mod chapter;
pub mod error;
pub mod manga;
pub mod meta;
pub mod page;
pub mod source;
pub mod sql;
pub mod sync_yomi;

#[cfg(test)]
mod sandbox_e2e {
    //! End-to-end test against a running JVM sandbox (see `jvm-sandbox/`).
    //! Skipped when no sandbox is listening on the test port.
    use crate::source::sandbox::HttpSandboxFetcher;
    use crate::source::SourceFetcher;

    const SANDBOX: &str = "http://127.0.0.1:8088";

    fn fetcher() -> HttpSandboxFetcher {
        HttpSandboxFetcher::new(SANDBOX)
    }

    #[tokio::test]
    async fn nhentai_full_chain() {
        let f = fetcher();
        if !f.health().await {
            eprintln!("SKIP: no sandbox on {SANDBOX}");
            return;
        }
        let sources = f.list_sources().await.expect("list sources");
        let en = sources.iter().find(|s| s.lang == "en").expect("en source");
        // popular
        let page = f.get_popular_manga(en.id, 1).await.expect("popular");
        assert!(!page.mangas.is_empty(), "popular returned no mangas");
        let m = &page.mangas[0];
        eprintln!("popular[0]: {} ({})", m.title, m.url);
        // details + chapters
        let smanga = suwayomi_core::source::SManga {
            url: m.url.clone(),
            ..Default::default()
        };
        let (updated, chapters) = f
            .fetch_manga_update(en.id, &smanga, &[], true, true)
            .await
            .expect("fetch update");
        assert!(!updated.title.is_empty(), "details returned empty title");
        assert!(!chapters.is_empty(), "no chapters");
        eprintln!("details: {} | chapters: {}", updated.title, chapters.len());
        // pages for the first chapter
        let pages = f
            .fetch_pages(en.id, &smanga.url, &chapters[0].url)
            .await
            .expect("fetch pages");
        assert!(!pages.is_empty(), "no pages");
        eprintln!("pages: {} (first: {})", pages.len(), pages[0].url);
        assert!(pages[0].url.starts_with("http"), "page url looks like a URL");
    }
}
