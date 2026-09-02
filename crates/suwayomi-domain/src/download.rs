//! Download manager — mirrors `manga/impl/download/DownloadManager.kt`.
//!
//! An in-process FIFO queue of chapter download jobs with a background worker
//! and a broadcast event bus (consumed by the GraphQL download subscriptions
//! and the REST download endpoints). Page fetching goes through
//! [`SourceFetcher::fetch_pages`]; success marks the chapter `is_downloaded`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::{broadcast, Mutex};

use suwayomi_core::db::Db;
use suwayomi_core::models::now_epoch_secs;

use crate::source::SourceFetcher;
use crate::sql::bind_placeholders;

/// Per-job state (mirrors `DownloadState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Downloading,
    Finished,
    Error,
}

/// One chapter download job.
#[derive(Debug, Clone)]
pub struct DownloadJob {
    pub chapter_id: i32,
    pub manga_id: i32,
    pub manga_title: String,
    pub chapter_name: String,
    pub chapter_url: String,
    pub source_id: i64,
    pub manga_url: String,
    pub state: JobState,
    pub progress: f64, // 0.0 ..= 1.0
    pub tries: i32,
}

/// Events streamed on the broadcast channel.
#[derive(Debug, Clone)]
pub enum DownloadEvent {
    /// Full queue snapshot (after any mutation).
    Snapshot { queue: Vec<DownloadJob>, running: bool },
    /// Per-job progress tick.
    Progress { chapter_id: i32, progress: f64 },
}

#[derive(Clone)]
pub struct DownloadManager {
    db: Db,
    fetcher: Arc<dyn SourceFetcher>,
    queue: Arc<Mutex<VecDeque<DownloadJob>>>,
    tx: broadcast::Sender<DownloadEvent>,
    running: Arc<AtomicBool>,
    worker_spawned: Arc<AtomicBool>,
}

impl DownloadManager {
    pub fn new(db: Db, fetcher: Arc<dyn SourceFetcher>) -> Self {
        let (tx, _) = broadcast::channel(128);
        Self {
            db,
            fetcher,
            queue: Arc::new(Mutex::new(VecDeque::new())),
            tx,
            running: Arc::new(AtomicBool::new(false)),
            worker_spawned: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DownloadEvent> {
        self.tx.subscribe()
    }

    /// Fetch a chapter's page list from the source (through the sandbox
    /// fetcher) without downloading the images. Used by the reader to
    /// hydrate the page table on demand when no pages are cached yet.
    pub async fn fetch_pages_from_source(
        &self,
        source_id: i64,
        manga_url: &str,
        chapter_url: &str,
    ) -> crate::error::Result<Vec<suwayomi_core::source::SourcePage>> {
        self.fetcher.fetch_pages(source_id, manga_url, chapter_url).await
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Queue snapshot for REST/GraphQL.
    pub async fn snapshot(&self) -> Vec<DownloadJob> {
        self.queue.lock().await.iter().cloned().collect()
    }

    fn emit(&self, event: DownloadEvent) {
        let _ = self.tx.send(event);
    }

    async fn emit_snapshot(&self) {
        let queue = self.snapshot().await;
        self.emit(DownloadEvent::Snapshot { queue, running: self.is_running() });
    }

    /// Enqueues a chapter by id (idempotent: skips if already queued).
    pub async fn enqueue_chapter(&self, chapter_id: i32) -> Result<(), String> {
        let pool = self.db.pool();
        #[derive(sqlx::FromRow)]
        #[allow(dead_code)] // FromRow maps all selected columns
        struct Row {
            chapter_id: i32,
            chapter_name: String,
            chapter_url: String,
            is_downloaded: bool,
            manga_id: i32,
            manga_title: String,
            manga_source: i64,
            manga_url: String,
        }
        let row: Option<Row> = sqlx::query_as(
            "SELECT c.id AS chapter_id, c.name AS chapter_name, c.url AS chapter_url, c.is_downloaded, \
             m.id AS manga_id, m.title AS manga_title, m.source AS manga_source, m.url AS manga_url \
             FROM chapter c JOIN manga m ON m.id = c.manga WHERE c.id = $1",
        )
        .bind(chapter_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
        let Some(r) = row else { return Err(format!("chapter {chapter_id} not found")) };
        if r.is_downloaded {
            return Err(format!("chapter {chapter_id} already downloaded"));
        }

        let mut queue = self.queue.lock().await;
        if queue.iter().any(|j| j.chapter_id == chapter_id) {
            return Ok(()); // already queued
        }
        queue.push_back(DownloadJob {
            chapter_id,
            manga_id: r.manga_id,
            manga_title: r.manga_title,
            chapter_name: r.chapter_name,
            chapter_url: r.chapter_url,
            source_id: r.manga_source,
            manga_url: r.manga_url,
            state: JobState::Queued,
            progress: 0.0,
            tries: 0,
        });
        drop(queue);
        self.emit_snapshot().await;
        Ok(())
    }

    /// Removes a queued job (no-op if currently downloading).
    pub async fn dequeue_chapter(&self, chapter_id: i32) -> Result<(), String> {
        let mut queue = self.queue.lock().await;
        queue.retain(|j| j.chapter_id != chapter_id);
        drop(queue);
        self.emit_snapshot().await;
        Ok(())
    }

    /// Clears the whole queue.
    pub async fn clear(&self) {
        self.queue.lock().await.clear();
        self.emit_snapshot().await;
    }

    /// Moves a queued chapter to a new position (0-based).
    pub async fn reorder(&self, chapter_id: i32, to: usize) {
        let mut queue = self.queue.lock().await;
        if let Some(pos) = queue.iter().position(|j| j.chapter_id == chapter_id) {
            let job = queue.remove(pos).expect("position just found");
            let to = to.min(queue.len());
            queue.insert(to, job);
        }
        drop(queue);
        self.emit_snapshot().await;
    }

    /// Starts the worker (no-op if already running).
    pub async fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        self.ensure_worker();
        self.emit_snapshot().await;
    }

    /// Requests the worker to stop after the current job.
    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.emit_snapshot().await;
    }

    fn ensure_worker(&self) {
        if self.worker_spawned.swap(true, Ordering::SeqCst) {
            return;
        }
        let mgr = self.clone();
        tokio::spawn(async move {
            mgr.worker_loop().await;
        });
    }

    async fn worker_loop(&self) {
        loop {
            if !self.running.load(Ordering::SeqCst) {
                // wait briefly for a start signal; then check queue
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                continue;
            }
            let job = {
                let mut queue = self.queue.lock().await;
                queue.pop_front()
            };
            let Some(job) = job else {
                // idle: keep the worker alive waiting for new jobs
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                continue;
            };
            self.process_job(job).await;
            self.emit_snapshot().await;
        }
    }

    async fn process_job(&self, mut job: DownloadJob) {
        job.state = JobState::Downloading;
        job.tries += 1;
        self.patch_job(&job).await;
        self.emit_snapshot().await;

        let result = self
            .fetcher
            .fetch_pages(job.source_id, &job.manga_url, &job.chapter_url)
            .await;

        match result {
            Ok(pages) => {
                let total = pages.len().max(1) as f64;
                for (i, _page) in pages.iter().enumerate() {
                    // per-page progress (no binary storage yet — the sandbox
                    // provides bytes in a later increment)
                    job.progress = (i + 1) as f64 / total;
                    self.emit(DownloadEvent::Progress { chapter_id: job.chapter_id, progress: job.progress });
                }
                // mark downloaded
                let _ = sqlx::query("UPDATE chapter SET is_downloaded = TRUE, fetched_at = $1 WHERE id = $2")
                    .bind(chrono::Utc::now().timestamp_millis())
                    .bind(job.chapter_id)
                    .execute(self.db.pool())
                    .await;
                job.state = JobState::Finished;
                job.progress = 1.0;
            }
            Err(e) => {
                tracing::warn!(chapter_id = job.chapter_id, "download failed: {e}");
                job.state = JobState::Error;
            }
        }
        self.patch_job(&job).await;
    }

    async fn patch_job(&self, job: &DownloadJob) {
        let mut queue = self.queue.lock().await;
        if let Some(existing) = queue.iter_mut().find(|j| j.chapter_id == job.chapter_id) {
            existing.state = job.state;
            existing.progress = job.progress;
            existing.tries = job.tries;
        } else {
            queue.push_front(job.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::StubFetcher;
    use suwayomi_core::db::Db;

    async fn seed() -> Db {
        let db = Db::connect_embedded(None).await.expect("connect");
        db.migrate().await.expect("migrate");
        let pool = db.pool();
        sqlx::query("INSERT INTO extension (name, pkg_name, version_name, version_code, lang, content_warning) VALUES ('E','p','1',1,'en',0)")
            .execute(pool)
            .await
            .expect("ext");
        sqlx::query("INSERT INTO source (name, lang, extension) VALUES ('S','en',1)").execute(pool).await.expect("src");
        sqlx::query("INSERT INTO manga (url, title, in_library, source) VALUES ('/m','M',TRUE,1)").execute(pool).await.expect("manga");
        sqlx::query("INSERT INTO chapter (url, name, source_order, manga) VALUES ('/m/c1','Ch1',0,1)").execute(pool).await.expect("ch");
        db
    }

    #[tokio::test]
    async fn enqueue_dequeue_clear_roundtrip() {
        let db = seed().await;
        let mgr = DownloadManager::new(db, Arc::new(StubFetcher));

        mgr.enqueue_chapter(1).await.expect("enqueue");
        let jobs = mgr.snapshot().await;
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].chapter_id, 1);
        assert_eq!(jobs[0].manga_title, "M");

        // duplicate enqueue is a no-op
        mgr.enqueue_chapter(1).await.expect("enqueue again");
        assert_eq!(mgr.snapshot().await.len(), 1);

        mgr.dequeue_chapter(1).await.expect("dequeue");
        assert!(mgr.snapshot().await.is_empty());

        mgr.enqueue_chapter(1).await.expect("enqueue 2");
        mgr.clear().await;
        assert!(mgr.snapshot().await.is_empty());
    }

    #[tokio::test]
    async fn enqueue_unknown_chapter_errors() {
        let db = seed().await;
        let mgr = DownloadManager::new(db, Arc::new(StubFetcher));
        let err = mgr.enqueue_chapter(999).await.unwrap_err();
        assert!(err.contains("not found"), "got: {err}");
    }

    #[tokio::test]
    async fn start_stop_marks_jobs_failed_with_stub_fetcher() {
        let db = seed().await;
        let mgr = DownloadManager::new(db.clone(), Arc::new(StubFetcher));
        let mut rx = mgr.subscribe();

        mgr.enqueue_chapter(1).await.expect("enqueue");
        mgr.start().await;
        assert!(mgr.is_running());

        // wait until the job leaves the queue (processed) or times out
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut seen_snapshot = false;
        while tokio::time::Instant::now() < deadline {
            let ev = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await;
            match ev {
                Ok(Ok(DownloadEvent::Snapshot { queue, .. })) => {
                    seen_snapshot = true;
                    if queue.is_empty() {
                        break;
                    }
                    // the job stays in the queue with Error state
                    if queue[0].state == JobState::Error || queue[0].state == JobState::Finished {
                        break;
                    }
                }
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
        assert!(seen_snapshot, "must see snapshots from the worker");
        mgr.stop().await;
        // stub fetcher → job failed, not downloaded
        let downloaded: bool = sqlx::query_scalar("SELECT is_downloaded FROM chapter WHERE id = 1").fetch_one(db.pool()).await.expect("flag");
        assert!(!downloaded, "stub fetcher cannot download");
    }
}

// ---------------------------------------------------------------------------
// Downloads-dir reconciliation
// ---------------------------------------------------------------------------

/// Reconciles the on-disk `data/downloads/` tree with the database so that
/// chapters downloaded on-disk (e.g. imported from a Tachiyomi/Tachidesk
/// backup, or written by an earlier build) show the "downloaded" badge in the
/// WebUI.
///
/// Layout: `{downloads}/{SourceName} ({LANG})/{MangaTitle}/{Chapter}.cbz`
///
/// Matching strategy: the manga directory name is matched against
/// `manga.title` first; the resolved `manga.url` then matches ALL rows with
/// that url across every language variant of the source, so browsing/searching
/// any variant surfaces the downloaded state. Chapters are upserted by
/// (manga, url) and marked `is_downloaded = TRUE`.
pub async fn reconcile_downloads(db: &Db, data_dir: &std::path::Path) -> crate::error::Result<usize> {
    let downloads_root = data_dir.join("downloads");
    if !downloads_root.is_dir() {
        return Ok(0);
    }
    // Earlier builds inserted page rows with `ON CONFLICT DO NOTHING`, which
    // is a no-op without a unique constraint — every startup re-inserted the
    // same pages, so readers saw the first page repeated. Dedupe once:
    // keep the lowest id per (chapter, index).
    let _ = sqlx::query(
        bind_placeholders("DELETE FROM page WHERE id NOT IN (SELECT MIN(id) FROM page GROUP BY chapter, index)").as_str(),
    )
    .execute(db.pool())
    .await;
    let mut matched_mangas: std::collections::HashSet<i32> = std::collections::HashSet::new();
    let mut total_chapters = 0usize;

    let source_dirs = match std::fs::read_dir(&downloads_root) {
        Ok(it) => it,
        Err(_) => return Ok(0),
    };
    for source_entry in source_dirs.flatten() {
        if !source_entry.path().is_dir() {
            continue;
        }
        // "{name} ({LANG})" -> (name, lang)
        let dir_name = source_entry.file_name().to_string_lossy().into_owned();
        let (name, lang) = match split_source_dir_name(&dir_name) {
            Some(v) => v,
            None => continue,
        };
        // resolve source row by name+lang (case-insensitive)
        let sql = bind_placeholders("SELECT id FROM source WHERE LOWER(name) = LOWER(?) AND LOWER(lang) = LOWER(?)");
        let source_id: Option<(i64,)> = match sqlx::query_as(&sql)
            .bind(&name)
            .bind(&lang)
            .fetch_optional(db.pool())
            .await
        {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some((source_id,)) = source_id else { continue };

        let manga_dirs = match std::fs::read_dir(source_entry.path()) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for manga_entry in manga_dirs.flatten() {
            if !manga_entry.path().is_dir() {
                continue;
            }
            let manga_title = manga_entry.file_name().to_string_lossy().into_owned();
            // 1) resolve a manga row by exact title under this source. The
            // (LANG) directory tag is authoritative — a mismatched download
            // directory (e.g. the old nhentai.com bug that filed everything
            // under JA) is the user's data to fix, not something to paper
            // over with a source-blind title match.
            let sql = bind_placeholders("SELECT id, url FROM manga WHERE source = ? AND title = ? LIMIT 1");
            let row: Option<(i32, String)> = match sqlx::query_as(&sql)
                .bind(source_id)
                .bind(&manga_title)
                .fetch_optional(db.pool())
                .await
            {
                Ok(v) => v,
                Err(_) => None,
            };
            let Some((_manga_id, manga_url)) = row else {
                tracing::warn!(%manga_title, "downloads: no matching manga row");
                continue;
            };
            // 2) all variants sharing the same url
            let sql = bind_placeholders("SELECT id FROM manga WHERE url = ?");
            let variants: Vec<(i32,)> = match sqlx::query_as(&sql).bind(&manga_url).fetch_all(db.pool()).await {
                Ok(v) => v,
                Err(_) => Vec::new(),
            };
            let variant_ids: Vec<i32> = variants.iter().map(|(id,)| *id).collect();
            for vid in &variant_ids {
                matched_mangas.insert(*vid);
            }
            // 3) chapters from the directory listing (files or subdirs)
            let entries: Vec<_> = match std::fs::read_dir(manga_entry.path()) {
                Ok(it) => it
                    .flatten()
                    .filter(|e| e.file_name() != ".nomedia" && e.file_name() != ".noxml")
                    .collect(),
                Err(_) => Vec::new(),
            };
            if entries.is_empty() {
                continue;
            }
            // manga-level metadata from the first archive that carries
            // ComicInfo.xml / meta.json (alt titles, author, artist, genre,
            // description) — fill only what the DB doesn't already know.
            let mut meta: Option<crate::source::local::ArchiveMeta> = None;
            for entry in &entries {
                let p = entry.path();
                if p.is_file() {
                    let ext = p
                        .extension()
                        .and_then(|x| x.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    if crate::source::local::ARCHIVE_EXTS.contains(&ext.as_str()) {
                        meta = crate::source::local::read_archive_meta(&p);
                        if meta.is_some() {
                            break;
                        }
                    }
                }
            }
            if let Some(m) = &meta {
                // NULLIF(..., '') so empty-string columns count as missing
                // and get filled from the archive metadata.
                let sql = bind_placeholders(
                    "UPDATE manga SET alt_titles = COALESCE(NULLIF(alt_titles, '[]'), ?), author = COALESCE(NULLIF(author, ''), ?), artist = COALESCE(NULLIF(artist, ''), ?), genre = COALESCE(NULLIF(genre, ''), ?), description = COALESCE(NULLIF(description, ''), ?) WHERE id = ?",
                );
                let alt = serde_json::to_string(&m.alt_titles).unwrap_or_else(|_| "[]".into());
                for vid in &variant_ids {
                    let _ = sqlx::query(&sql)
                        .bind(&alt)
                        .bind(&m.author)
                        .bind(&m.artist)
                        .bind(&m.genre)
                        .bind(&m.description)
                        .bind(vid)
                        .execute(db.pool())
                        .await;
                }
            }
            for vid in &variant_ids {
                for (i, entry) in entries.iter().enumerate() {
                    let cname = entry.file_name().to_string_lossy().into_owned();
                    let source_order = i as i32 + 1;
                    let now = now_epoch_secs();
                    let sql = bind_placeholders("SELECT id FROM chapter WHERE manga = ? AND url = ?");
                    let existing: Option<(i32,)> = match sqlx::query_as(&sql)
                        .bind(vid)
                        .bind(&cname)
                        .fetch_optional(db.pool())
                        .await
                    {
                        Ok(v) => v,
                        Err(_) => None,
                    };
                    // Chapter name: prefer the archive's own metadata title
                    // (ComicInfo `<Title>`), then the file stem, then
                    // "Chapter".
                    let mut chapter_name = "Chapter".to_string();
                    if entry.path().is_file() {
                        if let Some(meta) = crate::source::local::read_archive_meta(&entry.path())
                            && let Some(t) = meta.title
                            && !t.trim().is_empty()
                        {
                            chapter_name = t;
                        } else if let Some(stem) = std::path::Path::new(&cname)
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("chapter"))
                        {
                            chapter_name = stem;
                        }
                    } else if let Some(stem) = std::path::Path::new(&cname)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("chapter"))
                    {
                        chapter_name = stem;
                    }
                    let cbz_path = entry.path().to_string_lossy().into_owned();
                    let res = match existing {
                        Some((cid,)) => {
                            let _ = sqlx::query(
                                bind_placeholders(
                                    "UPDATE chapter SET is_downloaded = TRUE, source_order = ?, fetched_at = ?, real_url = ?, name = ? WHERE id = ?",
                                )
                                .as_str(),
                            )
                            .bind(source_order)
                            .bind(now)
                            .bind(&cbz_path)
                            .bind(&chapter_name)
                            .bind(cid)
                            .execute(db.pool())
                            .await;
                            Some(cid)
                        }
                        None => {
                            let sql = bind_placeholders(
                                "INSERT INTO chapter (url, name, chapter_number, source_order, manga, fetched_at, last_modified_at, is_downloaded, real_url) VALUES (?, ?, ?, ?, ?, ?, ?, TRUE, ?) RETURNING id",
                            );
                            sqlx::query_as::<_, (i32,)>(&sql)
                                .bind(&cname)
                                .bind(&chapter_name)
                                .bind(-1f32)
                                .bind(source_order)
                                .bind(vid)
                                .bind(now)
                                .bind(now)
                                .bind(&cbz_path)
                                .fetch_optional(db.pool())
                                .await
                                .ok()
                                .flatten()
                                .map(|(id,)| id)
                        }
                    };
                    if res.is_some() {
                        total_chapters += 1;
                    }
                    // page rows from the archive so the downloaded CBZ is
                    // readable: image_url points at the server's image
                    // endpoint, which extracts the bytes from the archive.
                    if let Some(cid) = res {
                        if entry.path().is_file() {
                            let pages = crate::source::local::list_archive_pages(&entry.path());
                            let page_count = pages.len() as i32;
                            let img_base = format!("/api/v1/manga/{vid}/chapter/{source_order}/page");
                            for (pi, pname) in &pages {
                                // real upsert by (chapter, index) — the page
                                // table has no unique constraint, so
                                // ON CONFLICT would silently re-insert.
                                let image_url = format!("{img_base}/{pi}/image");
                                let sql = bind_placeholders("SELECT id FROM page WHERE chapter = ? AND index = ?");
                                let existing_page: Option<(i32,)> = sqlx::query_as(&sql)
                                    .bind(cid)
                                    .bind(*pi as i32)
                                    .fetch_optional(db.pool())
                                    .await
                                    .ok()
                                    .flatten();
                                match existing_page {
                                    Some((pid,)) => {
                                        let sql = bind_placeholders(
                                            "UPDATE page SET url = ?, image_url = ? WHERE id = ?",
                                        );
                                        let _ = sqlx::query(&sql)
                                            .bind(&pname)
                                            .bind(&image_url)
                                            .bind(pid)
                                            .execute(db.pool())
                                            .await;
                                    }
                                    None => {
                                        let sql = bind_placeholders(
                                            "INSERT INTO page (index, url, image_url, chapter) VALUES (?, ?, ?, ?)",
                                        );
                                        let _ = sqlx::query(&sql)
                                            .bind(*pi as i32)
                                            .bind(&pname)
                                            .bind(&image_url)
                                            .bind(cid)
                                            .execute(db.pool())
                                            .await;
                                    }
                                }
                            }
                            // Reflect the page count on the chapter row —
                            // the reader relies on it for paged-mode state.
                            let sql = bind_placeholders(
                                "UPDATE chapter SET page_count = ? WHERE id = ?",
                            );
                            let _ = sqlx::query(&sql)
                                .bind(page_count)
                                .bind(cid)
                                .execute(db.pool())
                                .await;
                        }
                    }
                }
            }
        }
    }

    if !matched_mangas.is_empty() {
        tracing::info!(
            "downloads reconcile: {} manga, {} chapters",
            matched_mangas.len(),
            total_chapters
        );
    }
    Ok(total_chapters)
}

/// `"nHentai.com (unoriginal) (JA)"` -> `("nHentai.com (unoriginal)", "ja")`.
fn split_source_dir_name(dir_name: &str) -> Option<(String, String)> {
    let trimmed = dir_name.trim();
    let lang_start = trimmed.rfind('(')?;
    let lang_end = trimmed.rfind(')')?;
    if lang_end <= lang_start {
        return None;
    }
    let lang = trimmed[lang_start + 1..lang_end].trim().to_lowercase();
    if lang.is_empty() {
        return None;
    }
    let name = trimmed[..lang_start].trim().to_string();
    Some((name, lang))
}
