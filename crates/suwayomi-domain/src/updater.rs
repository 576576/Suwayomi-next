//! 库更新器 —— 把库内漫画的新章节抓回来写进 `chapter` 表，并在广播通道上
//! 发出进度快照。REST 的 `/api/v1/update/*` 与 GraphQL 的
//! `libraryUpdateStatus(Changed)` 读同一个句柄。
//!
//! 状态分三处保存：`jobs` 是每个漫画本轮的结果（`summary` 的 jobs 列表），
//! `categories` 记录本轮分类的参与情况（`summary` 的 categories），`latest`
//! 是最近一次广播出去的快照。三者都只活在内存里，`reset` 会把它们清空。

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, Mutex};

use suwayomi_core::db::Db;
use suwayomi_core::schema::{CategoryRow, ChapterRow, MangaRow};
use suwayomi_core::source::{SChapter, SManga};

use crate::meta::{MetaService, MetaTable};
use crate::source::SourceFetcher;

/// `global_meta` 里保存「上一次全库更新完成时刻」（epoch 毫秒）的键。
pub const LAST_GLOBAL_UPDATE_AT: &str = "last_global_update_at";

/// 单个漫画本轮的更新结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MangaJobStatus {
    Pending,
    Running,
    Complete,
    Failed,
    Skipped,
}

/// 单个分类本轮的参与情况。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CategoryJobStatus {
    Updating,
    Skipped,
}

/// 一个漫画的 job 结果。带上整行，GraphQL 侧才能映射成 `MangaType`。
#[derive(Debug, Clone)]
pub struct MangaUpdate {
    pub status: MangaJobStatus,
    pub manga: MangaRow,
}

/// 一个分类的 job 结果。
#[derive(Debug, Clone)]
pub struct CategoryUpdate {
    pub category: CategoryRow,
    pub status: CategoryJobStatus,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UpdaterJobsInfo {
    pub finished_jobs: i32,
    pub is_running: bool,
    pub skipped_categories_count: i32,
    pub skipped_mangas_count: i32,
    pub total_jobs: i32,
}

/// 一次广播出去的更新进度快照。
#[derive(Debug, Clone)]
pub struct LibraryUpdateStatus {
    pub category_updates: Vec<CategoryUpdate>,
    pub jobs_info: UpdaterJobsInfo,
    pub manga_updates: Vec<MangaUpdate>,
}

impl LibraryUpdateStatus {
    pub fn idle() -> Self {
        Self {
            category_updates: vec![],
            jobs_info: UpdaterJobsInfo::default(),
            manga_updates: vec![],
        }
    }
}

#[derive(Default)]
struct UpdaterState {
    running: bool,
    stop_requested: bool,
    /// 每个漫画本轮的 job 结果（`summary` 的 jobs / skippedMangas 来源）。
    jobs: HashMap<i32, MangaUpdate>,
    /// 本轮分类的参与情况（`summary` 的 categories 来源）。
    categories: HashMap<CategoryJobStatus, Vec<CategoryRow>>,
}

/// 共享句柄（可克隆；整个进程一份）。
#[derive(Clone)]
pub struct UpdateManager {
    db: Db,
    fetcher: Arc<dyn SourceFetcher>,
    tx: broadcast::Sender<LibraryUpdateStatus>,
    state: Arc<Mutex<UpdaterState>>,
    /// 最近一次广播的快照，供 `libraryUpdateStatus` 这类轮询解析器直接取。
    latest: Arc<Mutex<LibraryUpdateStatus>>,
}

impl UpdateManager {
    pub fn new(db: Db, fetcher: Arc<dyn SourceFetcher>) -> Self {
        let (tx, _) = broadcast::channel(64);
        Self {
            db,
            fetcher,
            tx,
            state: Arc::new(Mutex::new(UpdaterState::default())),
            latest: Arc::new(Mutex::new(LibraryUpdateStatus::idle())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LibraryUpdateStatus> {
        self.tx.subscribe()
    }

    pub async fn is_running(&self) -> bool {
        self.state.lock().await.running
    }

    /// 最近的快照（轮询用）。
    pub async fn latest_status(&self) -> LibraryUpdateStatus {
        self.latest.lock().await.clone()
    }

    /// 本轮所有 job（`summary` 的 jobs 列表）。
    pub async fn jobs(&self) -> Vec<MangaUpdate> {
        let st = self.state.lock().await;
        let mut jobs: Vec<MangaUpdate> = st.jobs.values().cloned().collect();
        jobs.sort_by_key(|j| j.manga.id);
        jobs
    }

    /// 本轮被跳过的漫画（`summary` 的 skippedMangas）。
    pub async fn skipped_mangas(&self) -> Vec<MangaRow> {
        let st = self.state.lock().await;
        let mut rows: Vec<MangaRow> = st
            .jobs
            .values()
            .filter(|j| j.status == MangaJobStatus::Skipped)
            .map(|j| j.manga.clone())
            .collect();
        rows.sort_by_key(|m| m.id);
        rows
    }

    /// 本轮分类的参与情况（`summary` 的 categories）。
    pub async fn category_status(&self) -> HashMap<CategoryJobStatus, Vec<CategoryRow>> {
        self.state.lock().await.categories.clone()
    }

    /// 上一次全库更新完成时刻（epoch 毫秒，持久化在 `global_meta`）。
    pub async fn last_update_timestamp_ms(&self) -> i64 {
        MetaService::new(self.db.clone())
            .get_map(MetaTable::Global, 0)
            .await
            .ok()
            .and_then(|m| m.get(LAST_GLOBAL_UPDATE_AT).and_then(|v| v.parse().ok()))
            .unwrap_or(0)
    }

    /// 启动任务；已在跑时是空操作。`categories` 为 None 表示整库。
    pub async fn start(&self, categories: Option<Vec<i32>>) {
        let mut st = self.state.lock().await;
        if st.running {
            return;
        }
        st.running = true;
        st.stop_requested = false;
        st.jobs.clear();
        st.categories = match self.load_category_rows(categories.as_deref()).await {
            Ok(cats) => cats,
            Err(e) => {
                tracing::warn!(%e, "updater: 读取分类失败，本轮不记录分类状态");
                HashMap::new()
            }
        };
        drop(st);
        let mgr = self.clone();
        tokio::spawn(async move {
            mgr.update_loop(categories).await;
            mgr.state.lock().await.running = false;
        });
    }

    /// 停止任务：worker 在下一个漫画开始前退出。
    pub async fn stop(&self) {
        self.state.lock().await.stop_requested = true;
    }

    /// 取消当前任务并清空内存状态；不动数据库，也不改 `last_global_update_at`。
    pub async fn reset(&self) {
        let mut st = self.state.lock().await;
        st.stop_requested = true;
        st.running = false;
        st.jobs.clear();
        st.categories.clear();
        drop(st);
        // 取消后 `update_loop` 仍会走完收尾并 emit 一次 idle —— 这里再补一次，
        // 保证 reset 之后读到的快照立刻是空闲态。
        self.emit(LibraryUpdateStatus::idle()).await;
    }

    async fn emit(&self, status: LibraryUpdateStatus) {
        *self.latest.lock().await = status.clone();
        let _ = self.tx.send(status);
    }

    /// 把本轮涉及的分类按「参与 / 跳过」归类。`include_in_update = FALSE`
    /// 的分类只有在被显式点名（`force_all`）时才算参与。
    async fn load_category_rows(
        &self,
        requested: Option<&[i32]>,
    ) -> Result<HashMap<CategoryJobStatus, Vec<CategoryRow>>, suwayomi_db::Error> {
        let mut all: Vec<CategoryRow> =
            suwayomi_db::query_as("SELECT * FROM category ORDER BY id").fetch_all(self.db.pool()).await?;
        let mut out: HashMap<CategoryJobStatus, Vec<CategoryRow>> = HashMap::new();
        let (updating, skipped): (Vec<CategoryRow>, Vec<CategoryRow>) = all.drain(..).partition(|c| match requested {
            Some(ids) => ids.contains(&c.id),
            None => c.include_in_update != 0,
        });
        out.insert(CategoryJobStatus::Updating, updating);
        out.insert(CategoryJobStatus::Skipped, skipped);
        Ok(out)
    }

    /// 记录一个漫画的 job 结果。
    async fn put_job(&self, manga: MangaRow, status: MangaJobStatus) {
        self.state.lock().await.jobs.insert(manga.id, MangaUpdate { status, manga });
    }

    /// 持久化本轮完成时刻。
    async fn persist_last_update(&self, now_ms: i64) {
        let mut m = HashMap::new();
        m.insert(LAST_GLOBAL_UPDATE_AT.to_string(), now_ms.to_string());
        let mut by_ref = HashMap::new();
        by_ref.insert(0i64, m);
        if let Err(e) = MetaService::new(self.db.clone()).modify(MetaTable::Global, &by_ref).await {
            tracing::warn!(%e, "updater: 写入 last_global_update_at 失败");
        }
    }

    async fn update_loop(&self, categories: Option<Vec<i32>>) {
        let pool = self.db.pool();
        let ids = match library_manga_ids(pool, categories.as_deref()).await {
            Ok(ids) => ids,
            Err(e) => {
                tracing::error!(%e, "updater: 读取书库失败");
                self.emit(LibraryUpdateStatus::idle()).await;
                return;
            }
        };

        let total = ids.len() as i32;
        let mut finished = 0;
        let mut skipped = 0;
        let mut manga_updates: Vec<MangaUpdate> = Vec::new();

        self.emit(LibraryUpdateStatus {
            category_updates: vec![],
            jobs_info: UpdaterJobsInfo {
                finished_jobs: 0,
                is_running: true,
                skipped_categories_count: self.category_status().await.get(&CategoryJobStatus::Skipped).map_or(0, |c| c.len() as i32),
                skipped_mangas_count: 0,
                total_jobs: total,
            },
            manga_updates: vec![],
        })
        .await;

        for manga_id in ids {
            if self.state.lock().await.stop_requested {
                break;
            }
            let outcome = self.update_one(pool, manga_id).await;
            let (status, row) = match outcome {
                Ok((new_chapters, row)) => {
                    if new_chapters == 0 {
                        skipped += 1;
                        (MangaJobStatus::Skipped, row)
                    } else {
                        (MangaJobStatus::Complete, row)
                    }
                }
                Err(e) => {
                    tracing::warn!(manga_id, "updater: 抓取章节失败: {e}");
                    match fetch_manga_row(pool, manga_id).await {
                        Ok(row) => (MangaJobStatus::Failed, row),
                        Err(_) => continue,
                    }
                }
            };
            self.put_job(row.clone(), status).await;
            manga_updates.push(MangaUpdate { status, manga: row });
            finished += 1;
            self.emit(LibraryUpdateStatus {
                category_updates: vec![],
                jobs_info: UpdaterJobsInfo {
                    finished_jobs: finished,
                    is_running: true,
                    skipped_categories_count: self.category_status().await.get(&CategoryJobStatus::Skipped).map_or(0, |c| c.len() as i32),
                    skipped_mangas_count: skipped,
                    total_jobs: total,
                },
                manga_updates: manga_updates.clone(),
            })
            .await;
        }

        self.emit(LibraryUpdateStatus {
            category_updates: vec![],
            jobs_info: UpdaterJobsInfo {
                finished_jobs: finished,
                is_running: false,
                skipped_categories_count: self.category_status().await.get(&CategoryJobStatus::Skipped).map_or(0, |c| c.len() as i32),
                skipped_mangas_count: skipped,
                total_jobs: total,
            },
            manga_updates,
        })
        .await;

        // 记下本轮完成时刻（UI 的「上次更新」用它）。
        self.persist_last_update(chrono::Utc::now().timestamp_millis()).await;
    }

    /// 抓一个漫画的章节列表并把新章节插库。返回 (新插入数, 漫画行)。
    async fn update_one(&self, pool: &Db, manga_id: i32) -> Result<(usize, MangaRow), String> {
        let manga = fetch_manga_row(pool, manga_id).await.map_err(|e| e.to_string())?;

        let smanga = SManga {
            url: manga.url.clone(),
            title: manga.title.clone(),
            thumbnail_url: manga.thumbnail_url.clone(),
            author: manga.author.clone(),
            status: manga.status,
            description: manga.description.clone(),
            genre: manga.genre.clone(),
            initialized: manga.initialized,
            ..Default::default()
        };

        let existing: Vec<SChapter> = suwayomi_db::query_as::<ChapterRow>("SELECT * FROM chapter WHERE manga = $1")
            .bind(manga_id)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|c| SChapter {
                url: c.url,
                name: c.name,
                chapter_number: c.chapter_number,
                scanlator: c.scanlator,
                date_upload: c.date_upload,
                ..Default::default()
            })
            .collect();

        let (_, chapters) = self
            .fetcher
            .fetch_manga_update(manga.source, &smanga, &existing, true, true)
            .await
            .map_err(|e| e.to_string())?;

        let mut inserted = 0usize;
        // 新章节盖上「发现时刻」（epoch 秒），这样它会出现在 updates 列表里
        // （判据是 fetched_at > in_library_at，两者都是 epoch 秒）。
        let now = chrono::Utc::now().timestamp();
        for (idx, ch) in chapters.iter().enumerate() {
            let res = suwayomi_db::query(
                "INSERT INTO chapter (url, name, date_upload, chapter_number, scanlator, source_order, real_url, fetched_at, manga) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) ON CONFLICT (url, manga) DO NOTHING",
            )
            .bind(&ch.url)
            .bind(&ch.name)
            .bind(ch.date_upload)
            .bind(ch.chapter_number)
            .bind(ch.scanlator.clone())
            .bind(idx as i32)
            .bind(ch.url.clone())
            .bind(now)
            .bind(manga_id)
            .execute(pool)
            .await;
            match res {
                Ok(r) => inserted += r.rows_affected() as usize,
                Err(e) => tracing::warn!(manga_id, url = %ch.url, "updater: 插入章节失败: {e}"),
            }
        }

        // `manga` 上的抓取时间字段都是 epoch 秒（age = now_epoch_secs -
        // last_fetched_at）；这里写成 timestamp_millis() 会把它们算错。
        let _ = suwayomi_db::query(
            "UPDATE manga SET last_fetched_at = $1, chapters_last_fetched_at = $1, last_modified_at = $1, version = version + 1 WHERE id = $2",
        )
        .bind(now)
        .bind(manga_id)
        .execute(pool)
        .await;

        Ok((inserted, manga))
    }
}

/// 库内漫画 id，可按分类过滤。
async fn library_manga_ids(pool: &Db, categories: Option<&[i32]>) -> Result<Vec<i32>, suwayomi_db::Error> {
    match categories {
        Some(cats) if !cats.is_empty() => {
            suwayomi_db::query_as::<(i32,)>(
                "SELECT DISTINCT m.id FROM manga m JOIN category_manga cm ON cm.manga = m.id \
                 WHERE m.in_library = TRUE AND cm.category = ANY($1) ORDER BY m.id",
            )
            .bind(cats)
            .fetch_all(pool)
            .await
            .map(|rows| rows.into_iter().map(|r| r.0).collect())
        }
        _ => {
            suwayomi_db::query_as::<(i32,)>("SELECT id FROM manga WHERE in_library = TRUE ORDER BY id")
                .fetch_all(pool)
                .await
                .map(|rows| rows.into_iter().map(|r| r.0).collect())
        }
    }
}

pub(crate) async fn fetch_manga_row(pool: &Db, manga_id: i32) -> Result<MangaRow, suwayomi_db::Error> {
    suwayomi_db::query_as("SELECT * FROM manga WHERE id = $1").bind(manga_id).fetch_one(pool).await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;

    use suwayomi_core::source::{MangasPage, SChapter, SManga};

    use super::*;
    use crate::source::SourceFetcher;

    /// 假源：`fetch_manga_update` 固定返回两章。
    #[derive(Default)]
    struct FakeFetcher {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl SourceFetcher for FakeFetcher {
        async fn fetch_manga_update(
            &self,
            _source_id: i64,
            _manga: &SManga,
            _chapters: &[SChapter],
            _fetch_details: bool,
            _fetch_chapters: bool,
        ) -> crate::error::Result<(SManga, Vec<SChapter>)> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok((
                SManga::default(),
                vec![
                    SChapter { url: "/c/1".into(), name: "Ch 1".into(), chapter_number: 1.0, scanlator: None, date_upload: 1_700_000_000_000, memo: Default::default() },
                    SChapter { url: "/c/2".into(), name: "Ch 2".into(), chapter_number: 2.0, scanlator: Some("TL".into()), date_upload: 1_700_000_100_000, memo: Default::default() },
                ],
            ))
        }

        async fn get_popular_manga(&self, _source_id: i64, _page: u32) -> crate::error::Result<MangasPage> {
            Ok(MangasPage::default())
        }

        async fn get_latest_updates(&self, _source_id: i64, _page: u32) -> crate::error::Result<MangasPage> {
            Ok(MangasPage::default())
        }

        async fn search_manga(&self, _source_id: i64, _query: &str, _page: u32) -> crate::error::Result<MangasPage> {
            Ok(MangasPage::default())
        }

        fn supports_latest(&self, _source_id: i64) -> bool {
            true
        }
    }

    async fn setup() -> Db {
        let db = Db::sqlite_in_memory().await.expect("connect embedded");
        db.migrate().await.expect("migrate");
        let pool = db.pool();
        suwayomi_db::query("INSERT INTO extension (name, pkg_name, version_name, version_code, lang, content_warning) VALUES ('E','p','1',1,'en',0)")
            .execute(pool)
            .await
            .expect("ext");
        suwayomi_db::query("INSERT INTO source (name, lang, extension) VALUES ('S','en',1)").execute(pool).await.expect("src");
        suwayomi_db::query("INSERT INTO manga (url, title, in_library, source) VALUES ('/m','Manga One',TRUE,1)")
            .execute(pool)
            .await
            .expect("manga");
        db
    }

    #[tokio::test]
    async fn updater_inserts_chapters_and_emits_events() {
        let db = setup().await;
        let update = UpdateManager::new(db.clone(), Arc::new(FakeFetcher::default()));
        let mut rx = update.subscribe();

        update.start(None).await;
        assert!(update.is_running().await, "start 之后应处于运行中");

        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        let mut saw_running = false;
        let mut saw_complete = false;
        while tokio::time::Instant::now() < deadline {
            let event = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await;
            match event {
                Ok(Ok(ev)) => {
                    if ev.jobs_info.is_running {
                        saw_running = true;
                    }
                    if !ev.jobs_info.is_running && ev.jobs_info.finished_jobs >= 1 {
                        saw_complete = true;
                        if let Some(m) = ev.manga_updates.first() {
                            assert!(m.status == MangaJobStatus::Complete, "有新章节时该漫画应为 Complete");
                        }
                        break;
                    }
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
                Err(_) => break,
            }
        }
        assert!(saw_running, "必须出现 is_running=true 的事件");
        assert!(saw_complete, "必须出现 finished_jobs >= 1 的收尾事件");

        let n: i64 = suwayomi_db::query_scalar("SELECT COUNT(*) FROM chapter").fetch_one(db.pool()).await.expect("统计章节");
        assert_eq!(n, 2, "更新器应插入两章");
        let names: Vec<String> = suwayomi_db::query_scalar("SELECT name FROM chapter ORDER BY source_order").fetch_all(db.pool()).await.expect("章节名");
        assert_eq!(names, vec!["Ch 1".to_string(), "Ch 2".to_string()]);

        // jobs / skippedMangas 是 REST summary 的数据源。
        let jobs = update.jobs().await;
        assert_eq!(jobs.len(), 1, "一个漫画对应一个 job");
        assert!(update.skipped_mangas().await.is_empty(), "有新章节的漫画不算 skipped");
    }

    #[tokio::test]
    async fn updater_marks_manga_failed_when_source_errors() {
        let db = setup().await;
        let update = UpdateManager::new(db, Arc::new(crate::source::StubFetcher));
        let mut rx = update.subscribe();
        update.start(None).await;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        let mut saw_failed = false;
        while tokio::time::Instant::now() < deadline {
            let event = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await;
            match event {
                Ok(Ok(ev)) => {
                    if !ev.jobs_info.is_running && ev.jobs_info.finished_jobs >= 1 {
                        if let Some(m) = ev.manga_updates.first() {
                            saw_failed = m.status == MangaJobStatus::Failed;
                        }
                        break;
                    }
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                _ => break,
            }
        }
        assert!(saw_failed, "源报错时该漫画应标 Failed");
    }

    #[tokio::test]
    async fn reset_clears_jobs_and_clears_running_flag() {
        let db = setup().await;
        let update = UpdateManager::new(db, Arc::new(crate::source::StubFetcher));
        update.start(None).await;
        update.reset().await;
        assert!(!update.is_running().await, "reset 之后不该再是运行中");
        assert!(update.jobs().await.is_empty(), "reset 应清空 jobs");
        assert!(!update.latest_status().await.jobs_info.is_running, "reset 应推出空闲快照");
    }
}
