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

use crate::source::SourceFetcher;

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
