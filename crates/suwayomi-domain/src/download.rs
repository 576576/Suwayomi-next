//! Download manager — mirrors `manga/impl/download/DownloadManager.kt`.
//!
//! An in-process FIFO queue of chapter download jobs with a background worker
//! and a broadcast event bus (consumed by the GraphQL download subscriptions
//! and the REST download endpoints). Page fetching goes through
//! [`SourceFetcher::fetch_pages`]; success marks the chapter `is_downloaded`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::broadcast;

use suwayomi_core::config::AppPaths;
use suwayomi_core::db::Db;
use suwayomi_core::models::now_epoch_secs;

use crate::source::{SourceBackend, SourceFetcher};
use crate::sql::bind_placeholders;
use std::fmt::Write as _;

/// 同一章内并发拉取页面的上限。对 CDN 友好，比旧的串行循环快约 8×。
const PAGE_FETCH_CONCURRENCY: usize = 8;

/// 下载根的默认落点：`<数据目录>/downloads`。
///
/// 实际生效的值由 [`AppPaths`] 持有（`downloadsPath` 设置可覆盖并在保存后立即
/// 生效），[`DownloadManager`] 通过 `&AppPaths` 取用，不再有进程级单例。
pub fn default_downloads_root(data_dir: &Path) -> PathBuf {
    data_dir.join("downloads")
}

/// Per-job state (mirrors `DownloadState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Downloading,
    Finished,
    Error,
}

impl JobState {
    /// 终止态：队首是它就必须出队，否则 worker 会把同一章反复重下。
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Finished | Self::Error)
    }
}

/// One chapter download job.
#[derive(Debug, Clone, PartialEq)]
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

/// 队列上能发生的事 —— 对外命令的语义化形式。
///
/// 把"命令"（`enqueue` / `dequeue` / `reorder` / …）与"状态怎么变"分开：
/// 命令负责校验与副作用（查库、发事件、拉起 worker），
/// 迁移只负责 [`QueueState::apply`] 里那几条规则。
#[derive(Debug, Clone)]
pub enum QueueEvent {
    /// 追加一个作业（同 `chapter_id` 已在队里就忽略，保持幂等）。
    Enqueued(DownloadJob),
    /// 移除一个作业（包括正在下载的那个）。
    Dequeued { chapter_id: i32 },
    /// 把作业移到新位置（`to` 越界截到队尾）。
    Reordered { chapter_id: i32, to: usize },
    /// 清空整个队列。
    Cleared,
    /// 就地更新作业的 state / progress / tries（正在下载的那个）。
    Patched { chapter_id: i32, state: JobState, progress: f64, tries: i32 },
    /// 队首的终止态作业出队，并丢掉它的实时进度。
    DrainedTerminalFront,
    /// 记录一页下载完成后的进度（不改作业结构）。
    Progress { chapter_id: i32, progress: f64 },
    /// 开始处理队列。
    Started,
    /// 处理完当前作业后停下来。
    Stopped,
}

/// 队列状态 —— 值语义，可整体移动、可比较。
///
/// 所有迁移都是 `self -> Self` 的纯函数（见 [`QueueState::apply`]）：没有锁、
/// 没有 IO、没有全局状态，队列语义（入队去重、终止项出队、重排越界）可以直接
/// 跑单测，不必起 tokio runtime 或数据库。
///
/// 实时进度与作业列表分开存：进度每页都在变，而作业列表的结构变化很少；
/// 合成一个字段的话每次进度心跳都要重建整个列表。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueueState {
    jobs: Vec<DownloadJob>,
    progress: std::collections::HashMap<i32, f64>,
    running: bool,
}

impl QueueState {
    /// 合并实时进度后的对外快照。
    pub fn snapshot(&self) -> Vec<DownloadJob> {
        self.jobs
            .iter()
            .map(|job| {
                self.progress
                    .get(&job.chapter_id)
                    .map_or_else(|| job.clone(), |progress| DownloadJob { progress: *progress, ..job.clone() })
            })
            .collect()
    }

    pub fn front(&self) -> Option<&DownloadJob> {
        self.jobs.first()
    }

    pub fn contains(&self, chapter_id: i32) -> bool {
        self.jobs.iter().any(|j| j.chapter_id == chapter_id)
    }

    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// 状态迁移（纯函数）：旧状态 + 事件 → 新状态。
    ///
    /// 刻意不返回 `Result`：非法输入（重复入队、越界重排、移除不存在的作业）
    /// 一律按幂等处理 —— 队列是多个对外入口的汇聚点，让每个入口各自处理
    /// "队列里没有这条作业"只会把同一份判断散到各处。
    #[must_use]
    pub fn apply(mut self, event: QueueEvent) -> Self {
        match event {
            QueueEvent::Enqueued(job) => {
                if !self.contains(job.chapter_id) {
                    self.jobs.push(job);
                }
            }
            QueueEvent::Dequeued { chapter_id } => self.jobs.retain(|j| j.chapter_id != chapter_id),
            QueueEvent::Reordered { chapter_id, to } => {
                if let Some(pos) = self.jobs.iter().position(|j| j.chapter_id == chapter_id) {
                    let job = self.jobs.remove(pos);
                    // 先移除再夹取上界：`Vec::insert` 在 `to > len` 时 panic，
                    // 而调用方给的是"客户端想要的位置"，越界只该落到队尾。
                    let to = to.min(self.jobs.len());
                    self.jobs.insert(to, job);
                }
            }
            QueueEvent::Cleared => self.jobs.clear(),
            QueueEvent::Patched { chapter_id, state, progress, tries } => {
                if let Some(job) = self.jobs.iter_mut().find(|j| j.chapter_id == chapter_id) {
                    job.state = state;
                    job.progress = progress;
                    job.tries = tries;
                }
            }
            QueueEvent::DrainedTerminalFront => {
                if self.front().is_some_and(|j| j.state.is_terminal()) {
                    if let Some(done) = self.jobs.first() {
                        self.progress.remove(&done.chapter_id);
                    }
                    self.jobs.remove(0);
                }
            }
            QueueEvent::Progress { chapter_id, progress } => {
                self.progress.insert(chapter_id, progress);
            }
            QueueEvent::Started => self.running = true,
            QueueEvent::Stopped => self.running = false,
        }
        self
    }
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
    fetcher: SourceBackend,
    /// 路径句柄（`downloadsPath` 设置可覆盖下载根；覆盖后立即生效）。
    paths: AppPaths,
    client: reqwest::Client,
    server_base_url: String,
    /// 唯一的可变状态：队列 + 实时进度 + 运行开关，一把锁持有。
    ///
    /// 用 `std::sync::Mutex` 而不是 tokio 的：所有迁移都是"移出状态 → 跑纯函数
    /// → 放回"，临界区里没有 `.await`，同步锁反而让 `set_progress` 这类高频
    /// 同步调用不必进 async 上下文。
    state: Arc<std::sync::Mutex<QueueState>>,
    tx: broadcast::Sender<DownloadEvent>,
    /// spawn-once 闩：worker 只拉一次，之后常驻等活（不是领域状态，是生命周期
    /// 守卫，所以留在原子量里）。
    worker_spawned: Arc<AtomicBool>,
}

/// worker 空转时的轮询间隔。
const IDLE_POLL: std::time::Duration = std::time::Duration::from_millis(300);

impl DownloadManager {
    pub fn new(db: Db, fetcher: SourceBackend, paths: AppPaths) -> Self {
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
            paths,
            client,
            server_base_url: String::from("http://127.0.0.1:8090"),
            state: Arc::new(std::sync::Mutex::new(QueueState::default())),
            tx,
            worker_spawned: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 取状态锁。中毒只说明"某个持有锁的线程 panic 过"，队列值本身仍然完整；
    /// 继续用旧值比让整个下载管理器崩掉好（与 `RuntimeConfig::snapshot` 同一口径）。
    fn state(&self) -> std::sync::MutexGuard<'_, QueueState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 施加一次队列迁移：把状态移出锁、跑纯函数、再放回。
    ///
    /// 用 `mem::take` 而不是 `clone`：迁移函数按值接收状态（这是它保持纯的原因），
    /// 而队列里每个作业都带若干 `String`；`take` 只是把 `Vec` / `HashMap` 的
    /// 几个指针挪走再挪回，不碰堆上的内容。
    fn apply(&self, event: QueueEvent) {
        let mut guard = self.state();
        let current = std::mem::take(&mut *guard);
        *guard = current.apply(event);
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
        self.state().is_running()
    }

    /// Queue snapshot for REST/GraphQL.
    pub async fn snapshot(&self) -> Vec<DownloadJob> {
        self.state().snapshot()
    }

    /// Records a per-page progress tick. Synchronous so it can be called from
    /// the downloader while pages are being fetched concurrently.
    pub fn set_progress(&self, chapter_id: i32, progress: f64) {
        self.apply(QueueEvent::Progress { chapter_id, progress });
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
        #[derive(suwayomi_db::FromRow)]
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
        let pool = self.db.pool();
        let row: Option<Row> = suwayomi_db::query_as(
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
        // 已在队里就直接返回，不发快照也不重拉 worker（批量入队时不刷消息）。
        if self.state().contains(chapter_id) {
            return Ok(());
        }

        self.apply(QueueEvent::Enqueued(DownloadJob {
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
        }));
        self.emit_snapshot().await;
        // Auto-start the worker: downloading from the manga/reader pages must
        // progress without the user having to open the queue and press play.
        self.start().await;
        Ok(())
    }

    /// Removes a queued job (no-op if not queued).
    pub async fn dequeue_chapter(&self, chapter_id: i32) -> Result<(), String> {
        self.apply(QueueEvent::Dequeued { chapter_id });
        self.emit_snapshot().await;
        Ok(())
    }

    /// Clears the whole queue.
    pub async fn clear(&self) {
        self.apply(QueueEvent::Cleared);
        self.emit_snapshot().await;
    }

    /// Moves a queued chapter to a new position (0-based).
    pub async fn reorder(&self, chapter_id: i32, to: usize) {
        self.apply(QueueEvent::Reordered { chapter_id, to });
        self.emit_snapshot().await;
    }

    /// Starts the worker (no-op if already running).
    pub async fn start(&self) {
        let was_running = {
            let mut guard = self.state();
            let current = std::mem::take(&mut *guard);
            let was = current.is_running();
            *guard = current.apply(QueueEvent::Started);
            was
        };
        if was_running {
            return;
        }
        self.ensure_worker();
        self.emit_snapshot().await;
    }

    /// Requests the worker to stop after the current job.
    pub async fn stop(&self) {
        self.apply(QueueEvent::Stopped);
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
            if !self.is_running() {
                // wait briefly for a start signal; then check queue
                tokio::time::sleep(IDLE_POLL).await;
                continue;
            }
            // Peek (do not pop) the head: the job stays in the queue while it
            // is processed so its state/progress are visible to snapshots.
            let job = { self.state().front().cloned() };
            let Some(job) = job else {
                // idle: keep the worker alive waiting for new jobs
                tokio::time::sleep(IDLE_POLL).await;
                continue;
            };
            if job.state.is_terminal() {
                // terminal leftover: drop it so the next queued chapter runs
                // (re-processing it would re-download the same chapter forever)
                self.apply(QueueEvent::DrainedTerminalFront);
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
        self.patch_job(&job);
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
        let result = if pages.is_empty() {
            self.fetcher.fetch_pages(job.source_id, &job.manga_url, &job.chapter_url).await
        } else {
            Ok(pages)
        };

        match result {
            Ok(pages) => {
                let page_total = pages.len();
                let mut stored = 0usize;
                let mut failed = 0usize;
                let mut page_files: Vec<(i32, String)> = Vec::new(); // (index, file name in archive)
                let mut archive_opt: Option<std::path::PathBuf> = None;
                let mut archive_err: Option<String> = None;
                // Download every page image (server-side, so CDN CORS/referer
                // rules don't matter), then bundle them into a CBZ under
                // `{downloads}/…` and wire the chapter up for offline reading
                // (mirroring reconcile_downloads' layout).
                let tx = self.tx.clone();
                let cid = job.chapter_id;
                match self
                    .download_chapter_archive(&job, &pages, &mut |i| {
                        let progress = page_progress(i + 1, page_total);
                        // Keep the queued job's progress accurate at all times.
                        self.set_progress(cid, progress);
                        let _ = tx.send(DownloadEvent::Progress { chapter_id: cid, progress });
                    })
                    .await
                {
                    Ok((archive, files)) => {
                        archive_opt = Some(archive);
                        page_files = files;
                        stored = page_files.len();
                    }
                    Err(e) => {
                        failed = pages.len();
                        archive_err = Some(e);
                    }
                }

                if let Some(archive) = archive_opt {
                    // Register the download so offline reading works through
                    // `/api/v1/manga/{manga}/chapter/{order}/page/{n}/image`.
                    let pool = self.db.pool();
                    let chapter_row: Option<(i32, i32)> =
                        suwayomi_db::query_as("SELECT c.manga, c.source_order FROM chapter c WHERE c.id = $1")
                            .bind(job.chapter_id)
                            .fetch_optional(pool)
                            .await
                            .ok()
                            .flatten();
                    for (pi, name) in &page_files {
                        let image_url = chapter_row
                            .map_or_else(String::new, |(manga_id, order)| page_image_url(manga_id, order, *pi));
                        let sql = bind_placeholders("SELECT id FROM page WHERE chapter = ? AND \"index\" = ?");
                        let existing: Option<(i32,)> = suwayomi_db::query_as(&sql)
                            .bind(job.chapter_id)
                            .bind(pi)
                            .fetch_optional(pool)
                            .await
                            .ok()
                            .flatten();
                        if let Some((pid,)) = existing {
                            let sql = bind_placeholders("UPDATE page SET url = ?, image_url = ? WHERE id = ?");
                            let _ = suwayomi_db::query(&sql).bind(name).bind(&image_url).bind(pid).execute(pool).await;
                        } else {
                            let sql = bind_placeholders(
                                "INSERT INTO page (\"index\", url, image_url, chapter) VALUES (?, ?, ?, ?)",
                            );
                            let _ = suwayomi_db::query(&sql)
                                .bind(pi)
                                .bind(name)
                                .bind(&image_url)
                                .bind(job.chapter_id)
                                .execute(pool)
                                .await;
                        }
                    }
                    let _ = suwayomi_db::query(
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
        self.patch_job(&job);
    }

    /// Builds a `ComicInfo.xml` (ComicRack standard) payload for the archive,
    /// based on the manga/chapter rows in the DB.
    async fn build_comic_info(&self, job: &DownloadJob, page_count: usize) -> Option<String> {
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
        let row = suwayomi_db::query(
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
            meta.title.clone_from(&job.chapter_name);
        }
        let e = xml_escape;
        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ComicInfo xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\">\n",
        );
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
                let _ = writeln!(xml, "  <{tag}>{}</{tag}>", e(value));
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
        let src: Option<(String, String)> = suwayomi_db::query_as("SELECT name, lang FROM source WHERE id = $1")
            .bind(job.source_id)
            .fetch_optional(self.db.pool())
            .await
            .map_err(|e| format!("source lookup: {e}"))?;
        let (src_name, src_lang) = src.ok_or_else(|| "source row missing".to_string())?;
        let root = self.paths.downloads();
        let cbz_path = chapter_archive_path(&root, &src_name, &src_lang, &job.manga_title, &job.chapter_name);
        // 归档目录恒有上级（路径由 root + 三段拼出），取不到就退回下载根。
        let dir = cbz_path.parent().map_or_else(|| root.clone(), Path::to_path_buf);
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;

        // ComicInfo.xml（ComicRack 标准）随包写入，携带作品/章节元数据
        let comic_info = self.build_comic_info(job, pages.len()).await;

        // 并发经同源图片代理下载页面：已代理缓存的在线阅读页命中磁盘秒回
        // （warm path），冷页绕开 CORS/hotlink，中断后可断点续拉。
        let fetches: Vec<(i32, String)> = pages
            .iter()
            .map(|p| {
                let raw = p.image_url.clone().unwrap_or_else(|| p.url.clone());
                (p.index, raw)
            })
            .collect();
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(PAGE_FETCH_CONCURRENCY));
        let client = self.client.clone();
        let mut join = tokio::task::JoinSet::new();
        for (idx, raw_url) in fetches.clone() {
            let permit_src = sem.clone();
            let proxy_path = crate::source::image_proxy_url(&raw_url);
            let url = format!("{}{}", self.server_base_url, proxy_path);
            let client = client.clone();
            join.spawn(async move {
                // 信号量关闭只在所有 `Arc` 都释放后发生；真发生了也只是这一页下载失败，
                // 让 `JoinSet` 收成一条错误，而不是 panic 掉整个下载任务。
                let _permit =
                    permit_src.acquire_owned().await.map_err(|_| format!("page {idx}: download queue closed"))?;
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
            remove_empty_dir_ancestors(&dir, &self.paths.downloads());
            return Err(format!("no page image could be downloaded: {}", errors.join("; ")));
        }
        if !errors.is_empty() {
            tracing::warn!(
                chapter_id = job.chapter_id,
                "download: {} page(s) failed: {}",
                errors.len(),
                errors.join("; ")
            );
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

    /// 把作业的 state / progress / tries 落回队列。
    ///
    /// The processed job stays in the queue while it runs; update it in
    /// place. Never (re-)insert here — that used to push a copy back to the
    /// front, which the worker then popped again and re-downloaded forever.
    fn patch_job(&self, job: &DownloadJob) {
        self.apply(QueueEvent::Patched {
            chapter_id: job.chapter_id,
            state: job.state,
            progress: job.progress,
            tries: job.tries,
        });
    }
}

// ---------------------------------------------------------------------------
// 纯逻辑：路径拼装与进度换算
// ---------------------------------------------------------------------------
// 这三件事跟"下载"这件事本身无关（没有 IO、没有状态），单独拆出来既能让
// 落盘路径的规则只有一处定义，也让它们可以直接跑单测。

/// 下载目录里一个源的子目录名：`"{SourceName} ({LANG})"`。
///
/// `reconcile_downloads` 按同一规则反推目录名，两边必须一致。
pub fn source_dir_name(source_name: &str, lang: &str) -> String {
    format!("{source_name} ({})", lang.to_uppercase())
}

/// 一章归档在磁盘上的文件名（净化后的章节名 + `.cbz`）。
pub fn archive_file_name(chapter_name: &str) -> String {
    format!("{}.cbz", sanitize_file_name(chapter_name))
}

/// 一章归档的落盘路径：
/// `{root}/{SourceName} ({LANG})/{MangaTitle}/{Chapter}.cbz`。
///
/// 目录名与文件名都先过 [`sanitize_file_name`]（Windows 非法字符被替换成 `_`）。
pub fn chapter_archive_path(
    root: &Path,
    source_name: &str,
    lang: &str,
    manga_title: &str,
    chapter_name: &str,
) -> PathBuf {
    root.join(source_dir_name(source_name, lang))
        .join(sanitize_file_name(manga_title))
        .join(archive_file_name(chapter_name))
}

/// 页面下载进度（0..1）：已完成页数 / 总页数，超过 1 截到 1。
/// 总页数为 0 时按 1 处理，避免除零。
pub fn page_progress(done: usize, total: usize) -> f64 {
    (done as f64 / total.max(1) as f64).min(1.0)
}

/// 一章某页在 REST 路由下的图片地址（离线阅读用）。
fn page_image_url(manga_id: i32, source_order: i32, index: i32) -> String {
    format!("/api/v1/manga/{manga_id}/chapter/{source_order}/page/{index}/image")
}

// ---------------------------------------------------------------------------
// Downloads-dir reconciliation
// ---------------------------------------------------------------------------
// 布局：{downloads}/{SourceName} ({LANG})/{MangaTitle}/{Chapter}.cbz。
// 匹配：目录名先对 manga.title，再以解析出的 manga.url 匹配该源所有语言变体
// 的行；章节按 (manga,url) upsert 并标 is_downloaded。

/// 用磁盘 `{downloads}/` 对账数据库：让磁盘上已有（如备份导入或旧版本写的）
/// 下载在 WebUI 显示「已下载」角标。`root` 由调用方给出（`AppPaths::downloads()`）。
/// 布局：{downloads}/{SourceName} ({LANG})/{MangaTitle}/{Chapter}.cbz。
/// 匹配：目录名先对 manga.title，再以解析出的 manga.url 匹配该源所有语言
/// 变体的行；章节按 (manga,url) upsert 并标 is_downloaded。
pub async fn reconcile_downloads(db: &Db, root: &Path) -> crate::error::Result<usize> {
    // Older builds "downloaded" chapters by flipping is_downloaded without
    // ever storing an archive (real_url stays empty) — nothing to read
    // offline. Clear those stale markers so the chapters can be downloaded
    // again; real downloads always set real_url to the CBZ path.
    // Same for markers whose archive file has since disappeared from disk.
    let stale_ids: Vec<i32> = {
        let sql = bind_placeholders("SELECT id, real_url FROM chapter WHERE is_downloaded = TRUE");
        let rows = suwayomi_db::query(&sql).fetch_all(db.pool()).await.unwrap_or_default();
        let mut ids: Vec<i32> = Vec::new();
        for r in rows {
            // `try_get` 而不是 `get`：缺列 / NULL 时跳过这一行，不该让一次清理
            // 扫描把整个进程打挂（库里已经没有会 panic 的 `get` 了）。
            let Ok(id) = r.try_get::<i32, _>("id") else { continue };
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
        suwayomi_db::query(
            bind_placeholders(
                "UPDATE chapter SET is_downloaded = FALSE WHERE is_downloaded = TRUE AND (real_url IS NULL OR real_url = '')",
            ).as_str(),
        )
        .execute(db.pool())
        .await
        .map_or(0, |r| r.rows_affected())
    } else {
        suwayomi_db::query(bind_placeholders("UPDATE chapter SET is_downloaded = FALSE WHERE id = ANY($1)").as_str())
            .bind(&stale_ids)
            .execute(db.pool())
            .await
            .map_or(0, |r| r.rows_affected())
    };
    if cleared > 0 {
        tracing::info!("downloads: cleared {cleared} stale download marker(s) without an archive");
    }
    if !stale_ids.is_empty() {
        // Drop page rows for the cleared chapters so the next reader/download
        // re-hydrates from the source instead of reusing archive endpoints
        // (`/api/v1/...`) that no longer serve bytes.
        let sql = bind_placeholders("DELETE FROM page WHERE chapter = ANY($1)");
        let _ = suwayomi_db::query(&sql).bind(&stale_ids).execute(db.pool()).await;
    }
    if !root.is_dir() {
        return Ok(0);
    }
    // Earlier builds inserted page rows with `ON CONFLICT DO NOTHING`, which
    // is a no-op without a unique constraint — every startup re-inserted the
    // same pages, so readers saw the first page repeated. Dedupe once:
    // keep the lowest id per (chapter, index).
    let _ = suwayomi_db::query(
        bind_placeholders("DELETE FROM page WHERE id NOT IN (SELECT MIN(id) FROM page GROUP BY chapter, \"index\")")
            .as_str(),
    )
    .execute(db.pool())
    .await;
    let mut matched_mangas: std::collections::HashSet<i32> = std::collections::HashSet::new();
    let mut total_chapters = 0usize;

    let Ok(source_dirs) = std::fs::read_dir(root) else {
        return Ok(0);
    };
    for source_entry in source_dirs.flatten() {
        if !source_entry.path().is_dir() {
            continue;
        }
        // "{name} ({LANG})" -> (name, lang)
        let dir_name = source_entry.file_name().to_string_lossy().into_owned();
        let Some((name, lang)) = split_source_dir_name(&dir_name) else {
            continue;
        };
        // resolve source row by name+lang (case-insensitive)
        let sql = bind_placeholders("SELECT id FROM source WHERE LOWER(name) = LOWER(?) AND LOWER(lang) = LOWER(?)");
        let source_id: Option<(i64,)> =
            match suwayomi_db::query_as(&sql).bind(&name).bind(&lang).fetch_optional(db.pool()).await {
                Ok(v) => v,
                Err(_) => continue,
            };
        let Some((source_id,)) = source_id else { continue };

        let Ok(manga_dirs) = std::fs::read_dir(source_entry.path()) else {
            continue;
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
            let row: Option<(i32, String)> = suwayomi_db::query_as(&sql)
                .bind(source_id)
                .bind(&manga_title)
                .fetch_optional(db.pool())
                .await
                .unwrap_or_default();
            // 1b) 精确匹配不上时按**落盘时用的净化规则**再找一遍：写目录走的是
            // `sanitize_file_name(title)`，标题里有 Windows 非法字符（`|` `:` `?` 等）
            // 时目录名与 title 本来就不相等 —— 只按 title 找会永远匹配不上，磁盘上
            // 明明有归档却显示未下载。
            let row = if let Some(r) = row {
                Some(r)
            } else {
                let sql = bind_placeholders("SELECT id, title, url FROM manga WHERE source = ?");
                let candidates: Vec<(i32, String, String)> =
                    suwayomi_db::query_as(&sql).bind(source_id).fetch_all(db.pool()).await.unwrap_or_default();
                candidates
                    .into_iter()
                    .find(|(_, title, _)| manga_dir_matches(title, &manga_title))
                    .map(|(id, _, url)| (id, url))
            };
            let Some((_manga_id, manga_url)) = row else {
                tracing::warn!(%manga_title, "downloads: no matching manga row");
                continue;
            };
            // 2) all variants sharing the same url
            let sql = bind_placeholders("SELECT id FROM manga WHERE url = ?");
            let variants: Vec<(i32,)> =
                suwayomi_db::query_as(&sql).bind(&manga_url).fetch_all(db.pool()).await.unwrap_or_default();
            let variant_ids: Vec<i32> = variants.iter().map(|(id,)| *id).collect();
            for vid in &variant_ids {
                matched_mangas.insert(*vid);
            }
            // 3) chapters from the directory listing (files or subdirs)
            let entries: Vec<_> = std::fs::read_dir(manga_entry.path()).map_or_else(
                |_| Vec::new(),
                |it| it.flatten().filter(|e| e.file_name() != ".nomedia" && e.file_name() != ".noxml").collect(),
            );
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
                    let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
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
                    let _ = suwayomi_db::query(&sql)
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
                    let mut existing: Option<(i32, bool)> = suwayomi_db::query_as::<(i32,)>(&sql)
                        .bind(vid)
                        .bind(&cbz_path)
                        .fetch_optional(db.pool())
                        .await
                        .map_or(None, |v| v.map(|(id,)| (id, true)));
                    if existing.is_none() {
                        let sql = bind_placeholders("SELECT id FROM chapter WHERE manga = ? AND url = ?");
                        existing = suwayomi_db::query_as::<(i32,)>(&sql)
                            .bind(vid)
                            .bind(&cname)
                            .fetch_optional(db.pool())
                            .await
                            .map_or(None, |v| v.map(|(id,)| (id, false)));
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
                            let _ = suwayomi_db::query(
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
                            let _ = suwayomi_db::query(
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
                            suwayomi_db::query_as::<(i32,)>(&sql)
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
                    if let Some(cid) = res
                        && entry.path().is_file()
                    {
                        let pages = crate::source::local::list_archive_pages(&entry.path());
                        let page_count = pages.len() as i32;
                        let img_base = format!("/api/v1/manga/{vid}/chapter/{source_order}/page");
                        for (pi, pname) in &pages {
                            // real upsert by (chapter, index) — the page
                            // table has no unique constraint, so
                            // ON CONFLICT would silently re-insert.
                            let image_url = format!("{img_base}/{pi}/image");
                            let sql = bind_placeholders("SELECT id FROM page WHERE chapter = ? AND \"index\" = ?");
                            let existing_page: Option<(i32,)> = suwayomi_db::query_as(&sql)
                                .bind(cid)
                                .bind(*pi as i32)
                                .fetch_optional(db.pool())
                                .await
                                .ok()
                                .flatten();
                            if let Some((pid,)) = existing_page {
                                let sql = bind_placeholders("UPDATE page SET url = ?, image_url = ? WHERE id = ?");
                                let _ = suwayomi_db::query(&sql)
                                    .bind(pname)
                                    .bind(&image_url)
                                    .bind(pid)
                                    .execute(db.pool())
                                    .await;
                            } else {
                                let sql = bind_placeholders(
                                    "INSERT INTO page (\"index\", url, image_url, chapter) VALUES (?, ?, ?, ?)",
                                );
                                let _ = suwayomi_db::query(&sql)
                                    .bind(*pi as i32)
                                    .bind(pname)
                                    .bind(&image_url)
                                    .bind(cid)
                                    .execute(db.pool())
                                    .await;
                            }
                        }
                        // Reflect the page count on the chapter row —
                        // the reader relies on it for paged-mode state.
                        let sql = bind_placeholders("UPDATE chapter SET page_count = ? WHERE id = ?");
                        let _ = suwayomi_db::query(&sql).bind(page_count).bind(cid).execute(db.pool()).await;
                    }
                }
            }
        }
    }

    if !matched_mangas.is_empty() {
        tracing::info!("downloads reconcile: {} manga, {} chapters", matched_mangas.len(), total_chapters);
    }

    // The downloads tree should only contain content-bearing folders (a CBZ
    // per chapter). Drop any empty directories left behind by failed runs or
    // earlier builds — deepest first, keeping the downloads root itself.
    if let Ok(entries) = std::fs::read_dir(root) {
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
    pool: &suwayomi_db::Db,
    chapter_id: i32,
) -> suwayomi_db::Result<Vec<suwayomi_core::source::SourcePage>> {
    // Skip rows whose image_url points at the offline archive endpoint
    // (`/api/v1/manga/.../page/N/image`): after a chapter has been downloaded
    // the download step rewrites rows to serve the CBZ. Those rows are only
    // usable while the archive exists, and re-downloading (archive removed)
    // must fall back to the source instead of re-fetching the archive URLs.
    let sql = bind_placeholders(
        "SELECT \"index\", url, image_url FROM page \
         WHERE chapter = ? AND (image_url IS NULL OR image_url NOT LIKE '/api/v1/%') \
         ORDER BY \"index\" ASC ",
    );
    let rows = suwayomi_db::query(&sql).bind(chapter_id).fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let url: String = r.try_get("url").unwrap_or_default();
            let image_url: Option<String> = r.try_get("image_url").ok().flatten();
            suwayomi_core::source::SourcePage {
                index: r.try_get("index").unwrap_or(0),
                image_url: image_url.or_else(|| Some(url.clone())),
                url,
                uri: None,
            }
        })
        .collect())
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
    let is_empty = std::fs::read_dir(path).is_ok_and(|mut rd| rd.next().is_none());
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
        let is_empty = std::fs::read_dir(&cur).is_ok_and(|mut rd| rd.next().is_none());
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
/// 磁盘上的漫画目录能否对上某一行的标题。目录是按 `sanitize_file_name(title)` 落盘的，
/// 标题里有 Windows 非法字符时目录名与 title 本来就不相等，所以净化后还要再比一次。
fn manga_dir_matches(title: &str, dir_name: &str) -> bool {
    title == dir_name || sanitize_file_name(title) == dir_name
}

fn sanitize_file_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let collapsed = cleaned.chars().fold(String::new(), |mut acc, c| {
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
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && bytes.get(8..12).is_some_and(|w| w == b"WEBP") {
        "webp"
    } else if bytes.starts_with(b"GIF") {
        "gif"
    } else if bytes.len() > 8 && bytes.get(4..8).is_some_and(|w| w == b"ftyp") {
        "avif"
    } else {
        "jpg"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suwayomi_core::db::Db;

    /// 测试用路径句柄：数据根与缓存根都落在系统临时目录下。
    fn test_paths() -> AppPaths {
        let tmp = std::env::temp_dir();
        AppPaths::new(tmp.clone(), tmp.join("cache"))
    }

    /// 一个排队中的作业；只关心 `chapter_id`，其余字段给固定值。
    fn job(chapter_id: i32) -> DownloadJob {
        DownloadJob {
            chapter_id,
            manga_id: 1,
            manga_title: "M".to_owned(),
            chapter_name: format!("Ch{chapter_id}"),
            chapter_url: format!("/m/c{chapter_id}"),
            source_id: 1,
            manga_url: "/m".to_owned(),
            state: JobState::Queued,
            progress: 0.0,
            tries: 0,
        }
    }

    fn queued(ids: &[i32]) -> QueueState {
        ids.iter().fold(QueueState::default(), |s, id| s.apply(QueueEvent::Enqueued(job(*id))))
    }

    fn ids(state: &QueueState) -> Vec<i32> {
        state.snapshot().iter().map(|j| j.chapter_id).collect()
    }

    #[test]
    fn queue_enqueue_is_idempotent() {
        let state = queued(&[1, 2, 1]);
        assert_eq!(ids(&state), [1, 2]);
        assert!(state.contains(2));
        assert!(!state.contains(3));
    }

    #[test]
    fn queue_dequeue_removes_matching_job() {
        let state = queued(&[1, 2, 3]).apply(QueueEvent::Dequeued { chapter_id: 2 });
        assert_eq!(ids(&state), [1, 3]);
    }

    #[test]
    fn queue_reorder_moves_job_and_clamps_out_of_range_to_tail() {
        let state = queued(&[1, 2, 3]).apply(QueueEvent::Reordered { chapter_id: 1, to: 1 });
        assert_eq!(ids(&state), [2, 1, 3]);

        // `to == len` 是客户端能给到的最大位置。移除后再夹取上界，
        // 落到队尾而不是越界 —— 旧实现按移除前的长度夹取，会 `insert` 越界 panic。
        let state = queued(&[1, 2, 3]).apply(QueueEvent::Reordered { chapter_id: 1, to: 3 });
        assert_eq!(ids(&state), [2, 3, 1]);

        // 越界到远超长度也一样，且不存在的作业是 no-op
        let state = queued(&[1, 2, 3]).apply(QueueEvent::Reordered { chapter_id: 1, to: usize::MAX });
        assert_eq!(ids(&state), [2, 3, 1]);
        let state = state.apply(QueueEvent::Reordered { chapter_id: 99, to: 0 });
        assert_eq!(ids(&state), [2, 3, 1]);
    }

    #[test]
    fn queue_clears_everything() {
        assert!(queued(&[1, 2]).apply(QueueEvent::Cleared).is_empty());
    }

    #[test]
    fn queue_patch_updates_job_in_place_without_reordering() {
        let state = queued(&[1, 2, 3]).apply(QueueEvent::Patched {
            chapter_id: 2,
            state: JobState::Downloading,
            progress: 0.5,
            tries: 1,
        });
        assert_eq!(ids(&state), [1, 2, 3], "增量更新不该动队列顺序");
        assert_eq!(state.front().map(|j| j.state), Some(JobState::Queued));
        assert_eq!(state.snapshot()[1].state, JobState::Downloading);
        assert_eq!(state.snapshot()[1].progress, 0.5);
        assert_eq!(state.snapshot()[1].tries, 1);
        // 不存在的作业同样只是 no-op
        let state =
            state.apply(QueueEvent::Patched { chapter_id: 99, state: JobState::Error, progress: 0.0, tries: 9 });
        assert_eq!(ids(&state), [1, 2, 3]);
    }

    #[test]
    fn queue_drains_terminal_front_and_drops_its_progress() {
        // 队首没到终止态 → 不出队
        let state = queued(&[1, 2])
            .apply(QueueEvent::Progress { chapter_id: 1, progress: 0.25 })
            .apply(QueueEvent::DrainedTerminalFront);
        assert_eq!(ids(&state), [1, 2]);

        // 队首 Finished → 出队，进度也一并丢掉
        let state = state
            .apply(QueueEvent::Patched { chapter_id: 1, state: JobState::Finished, progress: 1.0, tries: 1 })
            .apply(QueueEvent::DrainedTerminalFront);
        assert_eq!(ids(&state), [2]);
        assert_eq!(state.snapshot()[0].progress, 0.0, "被清掉的是 1 的进度，不该串到 2 上");
    }

    #[test]
    fn queue_snapshot_merges_live_progress() {
        let state = queued(&[1, 2]).apply(QueueEvent::Progress { chapter_id: 2, progress: 0.75 });
        let jobs = state.snapshot();
        assert_eq!(jobs[0].progress, 0.0);
        assert_eq!(jobs[1].progress, 0.75);
    }

    #[test]
    fn queue_start_stop_toggles_running() {
        assert!(!QueueState::default().is_running());
        assert!(QueueState::default().apply(QueueEvent::Started).is_running());
        assert!(!QueueState::default().apply(QueueEvent::Started).apply(QueueEvent::Stopped).is_running());
    }

    #[test]
    fn archive_path_sanitizes_every_segment() {
        let path = chapter_archive_path(Path::new("/dl"), "nHentai.com", "ja", "A|B", "Ch:1");
        assert_eq!(path, Path::new("/dl").join("nHentai.com (JA)").join("A_B").join("Ch_1.cbz"));
    }

    #[test]
    fn page_progress_clamps_and_handles_zero_total() {
        assert_eq!(page_progress(0, 0), 0.0);
        assert_eq!(page_progress(1, 0), 1.0);
        assert_eq!(page_progress(1, 4), 0.25);
        assert_eq!(page_progress(9, 4), 1.0);
    }

    async fn seed() -> Db {
        let db = Db::sqlite_in_memory().await.expect("connect");
        db.migrate().await.expect("migrate");
        let pool = db.pool();
        suwayomi_db::query("INSERT INTO extension (name, pkg_name, version_name, version_code, lang, content_warning) VALUES ('E','p','1',1,'en',0)")
            .execute(pool)
            .await
            .expect("ext");
        suwayomi_db::query("INSERT INTO source (name, lang, extension) VALUES ('S','en',1)")
            .execute(pool)
            .await
            .expect("src");
        suwayomi_db::query("INSERT INTO manga (url, title, in_library, source) VALUES ('/m','M',TRUE,1)")
            .execute(pool)
            .await
            .expect("manga");
        suwayomi_db::query("INSERT INTO chapter (url, name, source_order, manga) VALUES ('/m/c1','Ch1',0,1)")
            .execute(pool)
            .await
            .expect("ch");
        db
    }

    #[tokio::test]
    async fn enqueue_dequeue_clear_roundtrip() {
        let db = seed().await;
        let mgr = DownloadManager::new(db, SourceBackend::Stub, test_paths());

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

    #[test]
    fn manga_dir_matches_sanitized_title() {
        assert!(manga_dir_matches("M", "M"));
        // 标题里的 `|` 落盘时变 `_`：目录名跟 title 不相等，但就是同一部
        assert!(manga_dir_matches("A | B", "A _ B"));
        assert!(manga_dir_matches("[X] A | B [Chinese]", "[X] A _ B [Chinese]"));
        assert!(!manga_dir_matches("A | B", "A _ C"));
    }

    /// 回归：目录名是净化过的（`|` → `_`），对账只按 title 找会永远匹配不上 ——
    /// 磁盘上明明有归档，章节的已下载标记却会被清掉。
    #[tokio::test]
    async fn reconcile_matches_sanitized_manga_dir() {
        let db = seed().await;
        suwayomi_db::query("UPDATE manga SET title = 'A | B' WHERE id = 1").execute(db.pool()).await.unwrap();
        // 外部导入按 `url = 文件名` 匹配，这里把章节 url 设成归档名
        suwayomi_db::query("UPDATE chapter SET url = 'Ch1.cbz' WHERE id = 1").execute(db.pool()).await.unwrap();

        let data = std::env::temp_dir().join(format!("reconcile-sanitize-{}", std::process::id()));
        let manga_dir = data.join("downloads").join("S (EN)").join("A _ B");
        std::fs::create_dir_all(&manga_dir).unwrap();
        std::fs::write(manga_dir.join("Ch1.cbz"), b"x").unwrap();

        let chapters = reconcile_downloads(&db, &data.join("downloads")).await.expect("reconcile");
        assert!(chapters >= 1, "净化过的目录名没对上，处理了 {chapters} 个章节");
        let downloaded: Option<bool> = suwayomi_db::query_scalar("SELECT is_downloaded FROM chapter WHERE id = 1")
            .fetch_optional(db.pool())
            .await
            .unwrap();
        assert_eq!(downloaded, Some(true), "章节的已下载标记没被认回来");

        std::fs::remove_dir_all(&data).ok();
    }

    #[tokio::test]
    async fn enqueue_unknown_chapter_errors() {
        let db = seed().await;
        let mgr = DownloadManager::new(db, SourceBackend::Stub, test_paths());
        let err = mgr.enqueue_chapter(999).await.unwrap_err();
        assert!(err.contains("not found"), "got: {err}");
    }

    #[tokio::test]
    async fn start_stop_marks_jobs_failed_with_stub_fetcher() {
        let db = seed().await;
        let mgr = DownloadManager::new(db.clone(), SourceBackend::Stub, test_paths());
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
        let downloaded: bool = suwayomi_db::query_scalar("SELECT is_downloaded FROM chapter WHERE id = 1")
            .fetch_one(db.pool())
            .await
            .expect("flag");
        assert!(!downloaded, "stub fetcher cannot download");
    }
}
