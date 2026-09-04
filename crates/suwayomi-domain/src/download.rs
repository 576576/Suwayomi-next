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
    data_dir: std::path::PathBuf,
    client: reqwest::Client,
    server_base_url: String,
    queue: Arc<Mutex<VecDeque<DownloadJob>>>,
    /// Live per-chapter progress (0..1) written synchronously by the page
    /// downloader; merged into snapshots. Kept separate from `queue` so no
    /// async lock is needed for progress ticks.
    progress_by_id: Arc<std::sync::Mutex<std::collections::HashMap<i32, f64>>>,
    tx: broadcast::Sender<DownloadEvent>,
    running: Arc<AtomicBool>,
    worker_spawned: Arc<AtomicBool>,
}

impl DownloadManager {
    pub fn new(db: Db, fetcher: Arc<dyn SourceFetcher>, data_dir: std::path::PathBuf) -> Self {
        let (tx, _) = broadcast::channel(128);
        let client = reqwest::Client::builder()
            .user_agent("Suwayomi-next/1.0")
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            db,
            fetcher,
            data_dir,
            client,
            server_base_url: String::from("http://127.0.0.1:8090"),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            progress_by_id: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
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
        let mut jobs = self.queue.lock().await.iter().cloned().collect::<Vec<_>>();
        // Merge live progress ticks (written synchronously during downloads).
        let progress = self.progress_by_id.lock().unwrap();
        for job in &mut jobs {
            if let Some(p) = progress.get(&job.chapter_id) {
                job.progress = *p;
            }
        }
        jobs
    }

    /// Records a per-page progress tick. Synchronous so it can be called from
    /// the downloader while pages are being fetched concurrently.
    pub fn set_progress(&self, chapter_id: i32, progress: f64) {
        self.progress_by_id.lock().unwrap().insert(chapter_id, progress);
    }

    fn clear_progress(&self, chapter_id: i32) {
        self.progress_by_id.lock().unwrap().remove(&chapter_id);
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
        // Auto-start the worker: downloading from the manga/reader pages must
        // progress without the user having to open the queue and press play.
        self.start().await;
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
            // Peek (do not pop) the head: the job stays in the queue while it
            // is processed so its state/progress are visible to snapshots.
            let job = { let queue = self.queue.lock().await; queue.front().cloned() };
            let Some(job) = job else {
                // idle: keep the worker alive waiting for new jobs
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                continue;
            };
            if matches!(job.state, JobState::Finished | JobState::Error) {
                // terminal leftover: drop it so the next queued chapter runs
                // (re-processing it would re-download the same chapter forever)
                { let mut queue = self.queue.lock().await; queue.pop_front(); }
                self.clear_progress(job.chapter_id);
                self.emit_snapshot().await;
                continue;
            }
            self.process_job(job).await;
            self.emit_snapshot().await;
        }
    }

    async fn process_job(&self, mut job: DownloadJob) {
        job.state = JobState::Downloading;
        job.tries += 1;
        self.patch_job(&job).await;
        self.emit_snapshot().await;

        // Page list source, in priority order:
        //   1. page rows already in the DB (online reading populated them)
        //   2. fetch from the source through the sandbox (slow path; can
        //      hit the okhttp callTimeout for large chapters on slow CDNs)
        let mut pages: Vec<suwayomi_core::source::SourcePage> = Vec::new();
        match read_db_pages(self.db.pool(), job.chapter_id).await {
            Ok(rows) => pages = rows,
            Err(e) => tracing::debug!("read_db_pages: {e}"),
        }
        let result = if !pages.is_empty() {
            Ok(pages)
        } else {
            self.fetcher
                .fetch_pages(job.source_id, &job.manga_url, &job.chapter_url)
                .await
        };

        match result {
            Ok(pages) => {
                let total = pages.len().max(1) as f64;
                let mut stored = 0usize;
                let mut failed = 0usize;
                let mut page_files: Vec<(i32, String)> = Vec::new(); // (index, file name in archive)
                let mut archive_opt: Option<std::path::PathBuf> = None;
                let mut archive_err: Option<String> = None;
                // Download every page image (server-side, so CDN CORS/referer
                // rules don't matter), then bundle them into a CBZ under
                // `{data_dir}/downloads/…` and wire the chapter up for offline
                // reading (mirroring reconcile_downloads' layout).
                let tx = self.tx.clone();
                let cid = job.chapter_id;
                match self.download_chapter_archive(&job, &pages, &mut |i| {
                    let progress = ((i + 1) as f64 / total).min(1.0);
                    // Keep the queued job's progress accurate at all times.
                    self.set_progress(cid, progress);
                    let _ = tx.send(DownloadEvent::Progress { chapter_id: cid, progress });
                }).await
                {
                    Ok((archive, files)) => {
                        archive_opt = Some(archive);
                        page_files = files;
                        stored = page_files.len();
                    }
                    Err(e) => {
                        failed = pages.len();
                        archive_err = Some(e.to_string());
                    }
                }

                if let Some(archive) = archive_opt {
                    // Register the download so offline reading works through
                    // `/api/v1/manga/{manga}/chapter/{order}/page/{n}/image`.
                    let pool = self.db.pool();
                    let chapter_row: Option<(i32, i32)> = sqlx::query_as(
                        "SELECT c.manga, c.source_order FROM chapter c WHERE c.id = $1",
                    )
                    .bind(job.chapter_id)
                    .fetch_optional(pool)
                    .await
                    .ok()
                    .flatten();
                    let img_base = chapter_row
                        .map(|(manga_id, source_order)| format!("/api/v1/manga/{manga_id}/chapter/{source_order}/page"))
                        .unwrap_or_default();
                    for (pi, name) in &page_files {
                        let image_url = format!("{img_base}/{pi}/image");
                        let sql = bind_placeholders("SELECT id FROM page WHERE chapter = ? AND index = ?");
                        let existing: Option<(i32,)> = sqlx::query_as(&sql)
                            .bind(job.chapter_id)
                            .bind(pi)
                            .fetch_optional(pool)
                            .await
                            .ok()
                            .flatten();
                        match existing {
                            Some((pid,)) => {
                                let sql = bind_placeholders("UPDATE page SET url = ?, image_url = ? WHERE id = ?");
                                let _ = sqlx::query(&sql).bind(name).bind(&image_url).bind(pid).execute(pool).await;
                            }
                            None => {
                                let sql = bind_placeholders("INSERT INTO page (index, url, image_url, chapter) VALUES (?, ?, ?, ?)");
                                let _ = sqlx::query(&sql).bind(pi).bind(name).bind(&image_url).bind(job.chapter_id).execute(pool).await;
                            }
                        }
                    }
                    let _ = sqlx::query(
                        "UPDATE chapter SET is_downloaded = TRUE, real_url = $1, page_count = $2, fetched_at = $3 WHERE id = $4",
                    )
                    .bind(archive.to_string_lossy().to_string())
                    .bind(page_files.len() as i32)
                    .bind(now_epoch_secs())
                    .bind(job.chapter_id)
                    .execute(pool)
                    .await;
                    stored = page_files.len();
                } else {
                    tracing::warn!(chapter_id = job.chapter_id, "download failed: {}", archive_err.unwrap_or_default());
                }

                if stored > 0 {
                    job.state = JobState::Finished;
                    job.progress = 1.0;
                } else {
                    job.state = JobState::Error;
                }
                let _ = failed;
            }
            Err(e) => {
                tracing::warn!(chapter_id = job.chapter_id, "download failed: {e}");
                job.state = JobState::Error;
            }
        }
        self.patch_job(&job).await;
    }

    /// Builds a `ComicInfo.xml` (ComicRack standard) payload for the archive,
    /// based on the manga/chapter rows in the DB.
    async fn build_comic_info(&self, job: &DownloadJob, page_count: usize) -> Option<String> {
        use sqlx::Row;
        #[derive(Clone, Default)]
        struct Meta {
            title: String,
            series: String,
            number: String,
            writer: String,
            penciller: String,
            genre: String,
            summary: String,
            scan_info: String,
            page_count: String,
            pub_date: String,
        }
        let row = sqlx::query(
            "SELECT m.title, m.author, m.artist, m.genre, m.description, \
                    c.chapter_number, c.scanlator, c.date_upload \
             FROM chapter c JOIN manga m ON m.id = c.manga WHERE c.id = $1",
        )
        .bind(job.chapter_id)
        .fetch_optional(self.db.pool())
        .await
        .ok()?;
        let row = row?;
        let mut meta = Meta::default();
        meta.title = row.try_get::<String, _>("title").unwrap_or_default();
        meta.series = meta.title.clone();
        meta.writer = row.try_get::<String, _>("author").unwrap_or_default();
        meta.penciller = row.try_get::<String, _>("artist").unwrap_or_default();
        meta.genre = row.try_get::<String, _>("genre").unwrap_or_default();
        meta.summary = row.try_get::<String, _>("description").unwrap_or_default();
        meta.scan_info = row.try_get::<String, _>("scanlator").unwrap_or_default();
        meta.page_count = page_count.to_string();
        let number: f32 = row.try_get("chapter_number").unwrap_or(0.0);
        if number > 0.0 {
            meta.number = if (number - number.trunc()).abs() < f32::EPSILON {
                format!("{}", number as i64)
            } else {
                format!("{number}")
            };
        }
        let date_upload: i64 = row.try_get("date_upload").unwrap_or(0);
        if date_upload > 0 {
            let secs = if date_upload > 10_000_000_000 { date_upload / 1000 } else { date_upload };
            if let Some(dt) = chrono::DateTime::from_timestamp(secs, 0) {
                meta.pub_date = dt.format("%Y-%m-%d").to_string();
            }
        }
        if meta.title.is_empty() {
            meta.title = job.chapter_name.clone();
        }
        let e = xml_escape;
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ComicInfo xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\">\n");
        for (tag, value) in [
            ("Title", &meta.title),
            ("Series", &meta.series),
            ("Number", &meta.number),
            ("Writer", &meta.writer),
            ("Penciller", &meta.penciller),
            ("Genre", &meta.genre),
            ("Summary", &meta.summary),
            ("PageCount", &meta.page_count),
            ("ScanInformation", &meta.scan_info),
            ("PublicationDate", &meta.pub_date),
        ] {
            if !value.is_empty() {
                xml.push_str(&format!("  <{tag}>{}</{tag}>\n", e(value)));
            }
        }
        xml.push_str("</ComicInfo>");
        Some(xml)
    }

    /// Fetch image bytes for every page, bundle them into a CBZ and store it
    /// under `{data_dir}/downloads/{Source} ({LANG})/{MangaTitle}/{Chapter}.cbz`.
    /// Returns the archive path and `(page_index, file_name_in_archive)` pairs.
    async fn download_chapter_archive(
        &self,
        job: &DownloadJob,
        pages: &[suwayomi_core::source::SourcePage],
        progress: &mut (dyn FnMut(usize) + Send),
    ) -> Result<(std::path::PathBuf, Vec<(i32, String)>), String> {
        // Resolve the source directory tag "{name} ({LANG})".
        let src: Option<(String, String)> = sqlx::query_as("SELECT name, lang FROM source WHERE id = $1")
            .bind(job.source_id)
            .fetch_optional(self.db.pool())
            .await
            .map_err(|e| format!("source lookup: {e}"))?;
        let (src_name, src_lang) = src.ok_or_else(|| "source row missing".to_string())?;
        let source_dir = format!("{src_name} ({})", src_lang.to_uppercase());
        let manga_dir = sanitize_file_name(&job.manga_title);
        let chapter_file = format!("{}.cbz", sanitize_file_name(&job.chapter_name));

        let dir = self
            .data_dir
            .join("downloads")
            .join(&source_dir)
            .join(&manga_dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
        let cbz_path = dir.join(&chapter_file);

        // ComicInfo.xml（ComicRack 标准）随包写入，携带作品/章节元数据
        let comic_info = self.build_comic_info(job, pages.len()).await;

        // 并发经同源图片代理下载页面：已代理缓存的在线阅读页命中磁盘秒回
        // （warm path），冷页绕开 CORS/hotlink，中断后可断点续拉。
        // 8 并发对 CDN 友好且比旧的串行循环快约 8×。
        const CONCURRENCY: usize = 8;
        let fetches: Vec<(i32, String)> = pages
            .iter()
            .map(|p| {
                let raw = p.image_url.clone().unwrap_or_else(|| p.url.clone());
                (p.index, raw)
            })
            .collect();
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(CONCURRENCY));
        let client = self.client.clone();
        let mut join = tokio::task::JoinSet::new();
        for (idx, raw_url) in fetches.clone() {
            let permit_src = sem.clone();
            let proxy_path = crate::source::image_proxy_url(&raw_url);
            let url = format!("{}{}", self.server_base_url, proxy_path);
            let client = client.clone();
            join.spawn(async move {
                let _permit = permit_src.acquire_owned().await.expect("semaphore closed");
                let r = client.get(&url).send().await;
                let resp = match r {
                    Ok(r) if r.status().is_success() => r,
                    Ok(r) => return Err(format!("page {idx}: HTTP {}", r.status())),
                    Err(e) => return Err(format!("page {idx}: {e}")),
                };
                let bytes = match resp.bytes().await {
                    Ok(b) => b,
                    Err(e) => return Err(format!("page {idx}: read {e}")),
                };
                Ok::<(i32, Vec<u8>), String>((idx, bytes.to_vec()))
            });
        }
        let mut downloaded: Vec<(i32, String, Vec<u8>)> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        while let Some(joined) = join.join_next().await {
            match joined {
                    Ok(Ok((idx, bytes))) => {
                        let ext = image_ext_from_content_type(&bytes);
                        downloaded.push((idx, format!("{idx}.{ext}"), bytes));
                        progress(downloaded.len());
                    }
                    Ok(Err(e)) => errors.push(e),
                    Err(e) => errors.push(format!("join: {e}")),
                }
        }
        if downloaded.is_empty() {
            // No page could be fetched — don't leave behind an empty manga
            // folder (or empty {Source} parent chain) in the downloads tree.
            remove_empty_dir_ancestors(&dir, &self.data_dir.join("downloads"));
            return Err(format!("no page image could be downloaded: {}", errors.join("; ")));
        }
        if !errors.is_empty() {
            tracing::warn!(chapter_id = job.chapter_id, "download: {} page(s) failed: {}", errors.len(), errors.join("; "));
        }

        // Bundle into a CBZ.
        let cursor = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        if let Some(xml) = &comic_info {
            zip.start_file("ComicInfo.xml", options).map_err(|e| format!("zip: {e}"))?;
            std::io::Write::write_all(&mut zip, xml.as_bytes()).map_err(|e| format!("zip write: {e}"))?;
        }
        for (_, name, bytes) in &downloaded {
            zip.start_file(name.clone(), options).map_err(|e| format!("zip: {e}"))?;
            std::io::Write::write_all(&mut zip, bytes).map_err(|e| format!("zip write: {e}"))?;
        }
        let cursor = zip.finish().map_err(|e| format!("zip finish: {e}"))?;
        let bytes = cursor.into_inner();
        // avoid partial archives on crash: write tmp then rename
        let tmp = cbz_path.with_extension("cbz.tmp");
        std::fs::write(&tmp, &bytes).map_err(|e| format!("write: {e}"))?;
        if let Err(e) = std::fs::rename(&tmp, &cbz_path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("rename: {e}"));
        }
        Ok((cbz_path, downloaded.into_iter().map(|(i, n, _)| (i, n)).collect()))
    }

    async fn patch_job(&self, job: &DownloadJob) {
        // The processed job stays in the queue while it runs; update it in
        // place. Never (re-)insert here — that used to push a copy back to the
        // front, which the worker then popped again and re-downloaded forever.
        let mut queue = self.queue.lock().await;
        if let Some(existing) = queue.iter_mut().find(|j| j.chapter_id == job.chapter_id) {
            existing.state = job.state;
            existing.progress = job.progress;
            existing.tries = job.tries;
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
        let mgr = DownloadManager::new(db, Arc::new(StubFetcher), std::env::temp_dir());

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
        let mgr = DownloadManager::new(db, Arc::new(StubFetcher), std::env::temp_dir());
        let err = mgr.enqueue_chapter(999).await.unwrap_err();
        assert!(err.contains("not found"), "got: {err}");
    }

    #[tokio::test]
    async fn start_stop_marks_jobs_failed_with_stub_fetcher() {
        let db = seed().await;
        let mgr = DownloadManager::new(db.clone(), Arc::new(StubFetcher), std::env::temp_dir());
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
// 布局：{downloads}/{SourceName} ({LANG})/{MangaTitle}/{Chapter}.cbz。
// 匹配：目录名先对 manga.title，再以解析出的 manga.url 匹配该源所有语言变体
// 的行；章节按 (manga,url) upsert 并标 is_downloaded。

/// 用磁盘 data/downloads/ 对账数据库：让磁盘上已有（如备份导入或旧版本写的）
/// 下载在 WebUI 显示「已下载」角标。
/// 布局：{downloads}/{SourceName} ({LANG})/{MangaTitle}/{Chapter}.cbz。
/// 匹配：目录名先对 manga.title，再以解析出的 manga.url 匹配该源所有语言
/// 变体的行；章节按 (manga,url) upsert 并标 is_downloaded。
pub async fn reconcile_downloads(db: &Db, data_dir: &std::path::Path) -> crate::error::Result<usize> {
    let downloads_root = data_dir.join("downloads");
    // Older builds "downloaded" chapters by flipping is_downloaded without
    // ever storing an archive (real_url stays empty) — nothing to read
    // offline. Clear those stale markers so the chapters can be downloaded
    // again; real downloads always set real_url to the CBZ path.
    // Same for markers whose archive file has since disappeared from disk.
    let stale_ids: Vec<i32> = {
        use sqlx::Row;
        let sql = bind_placeholders(
            "SELECT id, real_url FROM chapter WHERE is_downloaded = TRUE",
        );
        let rows = sqlx::query(&sql).fetch_all(db.pool()).await.unwrap_or_default();
        let mut ids: Vec<i32> = Vec::new();
        for r in rows {
            let id: i32 = r.get("id");
            let real: Option<String> = r.try_get("real_url").ok().flatten();
            let missing = match &real {
                Some(p) if !p.is_empty() => !std::path::Path::new(p).exists(),
                _ => true,
            };
            if missing {
                ids.push(id);
            }
        }
        ids
    };
    let cleared = if stale_ids.is_empty() {
        sqlx::query(
            bind_placeholders(
                "UPDATE chapter SET is_downloaded = FALSE WHERE is_downloaded = TRUE AND (real_url IS NULL OR real_url = '')",
            ).as_str(),
        )
        .execute(db.pool())
        .await
        .map(|r| r.rows_affected())
        .unwrap_or(0)
    } else {
        sqlx::query(
            bind_placeholders("UPDATE chapter SET is_downloaded = FALSE WHERE id = ANY($1)").as_str(),
        )
        .bind(&stale_ids)
        .execute(db.pool())
        .await
        .map(|r| r.rows_affected())
        .unwrap_or(0)
    };
    if cleared > 0 {
        tracing::info!("downloads: cleared {cleared} stale download marker(s) without an archive");
    }
    if !stale_ids.is_empty() {
        // Drop page rows for the cleared chapters so the next reader/download
        // re-hydrates from the source instead of reusing archive endpoints
        // (`/api/v1/...`) that no longer serve bytes.
        let sql = bind_placeholders("DELETE FROM page WHERE chapter = ANY($1)");
        let _ = sqlx::query(&sql).bind(&stale_ids).execute(db.pool()).await;
    }
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
                    let cbz_path = entry.path().to_string_lossy().into_owned();
                    // Match order matters: first try the chapter this server
                    // downloaded itself (real_url = this archive — its url is
                    // the remote source url, not the file name); only then
                    // fall back to external imports matched by url = file
                    // name. Without the real_url match, reconcile re-inserted
                    // our own downloads as duplicate "Chapter.cbz" chapters.
                    let sql = bind_placeholders("SELECT id FROM chapter WHERE manga = ? AND real_url = ?");
                    let mut existing: Option<(i32, bool)> = match sqlx::query_as::<_, (i32,)>(&sql)
                        .bind(vid)
                        .bind(&cbz_path)
                        .fetch_optional(db.pool())
                        .await
                    {
                        Ok(v) => v.map(|(id,)| (id, true)),
                        Err(_) => None,
                    };
                    if existing.is_none() {
                        let sql = bind_placeholders("SELECT id FROM chapter WHERE manga = ? AND url = ?");
                        existing = match sqlx::query_as::<_, (i32,)>(&sql)
                            .bind(vid)
                            .bind(&cname)
                            .fetch_optional(db.pool())
                            .await
                        {
                            Ok(v) => v.map(|(id,)| (id, false)),
                            Err(_) => None,
                        };
                    }
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
                    let res = match existing {
                        Some((cid, true)) => {
                            // Our own download: keep the chapter's source
                            // order and name (they mirror the remote chapter
                            // list / extension metadata), just sync state.
                            let _ = sqlx::query(
                                bind_placeholders(
                                    "UPDATE chapter SET is_downloaded = TRUE, fetched_at = ?, real_url = ? WHERE id = ?",
                                )
                                .as_str(),
                            )
                            .bind(now)
                            .bind(&cbz_path)
                            .bind(cid)
                            .execute(db.pool())
                            .await;
                            Some(cid)
                        }
                        Some((cid, false)) => {
                            // External import matched by url = file name:
                            // keep the original ordering behaviour.
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

    // The downloads tree should only contain content-bearing folders (a CBZ
    // per chapter). Drop any empty directories left behind by failed runs or
    // earlier builds — deepest first, keeping the downloads root itself.
    if let Ok(entries) = std::fs::read_dir(&downloads_root) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                prune_empty_dir_tree(&entry.path());
            }
        }
    }

    Ok(total_chapters)
}

/// Read page rows already in the DB for a chapter.
async fn read_db_pages(
    pool: &sqlx::PgPool,
    chapter_id: i32,
) -> sqlx::Result<Vec<suwayomi_core::source::SourcePage>> {
    use sqlx::Row;
    // Skip rows whose image_url points at the offline archive endpoint
    // (`/api/v1/manga/.../page/N/image`): after a chapter has been downloaded
    // the download step rewrites rows to serve the CBZ. Those rows are only
    // usable while the archive exists, and re-downloading (archive removed)
    // must fall back to the source instead of re-fetching the archive URLs.
    let sql = bind_placeholders(
        "SELECT index, url, image_url FROM page \
         WHERE chapter = ? AND (image_url IS NULL OR image_url NOT LIKE '/api/v1/%') \
         ORDER BY index ASC ",
    );
    let rows = sqlx::query(&sql).bind(chapter_id).fetch_all(pool).await?;
    Ok(rows.into_iter().map(|r| {
        let url: String = r.try_get("url").unwrap_or_default();
        let image_url: Option<String> = r.try_get("image_url").ok().flatten();
        suwayomi_core::source::SourcePage {
            index: r.try_get("index").unwrap_or(0),
            image_url: image_url.or(Some(url.clone())),
            url,
            uri: None,
        }
    }).collect())
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

/// XML text escaping for metadata written into `ComicInfo.xml`.
fn xml_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Recursively removes empty directories below `path` (deepest first); the
/// passed directory itself is removed once it (and its children) are empty.
fn prune_empty_dir_tree(path: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                prune_empty_dir_tree(&entry.path());
            }
        }
    }
    let is_empty = std::fs::read_dir(path).map(|mut rd| rd.next().is_none()).unwrap_or(false);
    if is_empty {
        let _ = std::fs::remove_dir(path);
    }
}

/// Walks upward from `start` removing directories that are empty, stopping at
/// (and never removing) `stop`.
fn remove_empty_dir_ancestors(start: &std::path::Path, stop: &std::path::Path) {
    let mut cur = start.to_path_buf();
    loop {
        if cur == stop || !cur.starts_with(stop) {
            break;
        }
        let is_empty = std::fs::read_dir(&cur).map(|mut rd| rd.next().is_none()).unwrap_or(false);
        if !is_empty {
            break;
        }
        if std::fs::remove_dir(&cur).is_err() {
            break;
        }
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => break,
        }
    }
}

/// Filesystem-safe directory/file name: strip Windows-invalid characters
/// and trailing dots/spaces, collapse runs to a single character.
fn sanitize_file_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let collapsed = cleaned
        .chars()
        .fold(String::new(), |mut acc, c| {
            if acc.ends_with('_') && c == '_' {
                // skip duplicate underscores
            } else {
                acc.push(c);
            }
            acc
        });
    let trimmed = collapsed.trim_matches(|c| c == '.' || c == ' ' || c == '_');
    if trimmed.is_empty() { "_".to_string() } else { trimmed.to_string() }
}

/// Best-effort image extension from magic bytes (order matters).
fn image_ext_from_content_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        "jpg"
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "webp"
    } else if bytes.starts_with(&[b'G', b'I', b'F']) {
        "gif"
    } else if bytes.len() > 8 && &bytes[4..8] == b"ftyp" {
        "avif"
    } else {
        "jpg"
    }
}
