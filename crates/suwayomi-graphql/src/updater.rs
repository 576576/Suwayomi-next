//! Library updater with a broadcast event bus.
//!
//! Mirrors `manga/impl/update/UpdateLibraryService.kt`: a background task
//! walks the library, fetches each manga's chapters from its source via the
//! `SourceFetcher`, inserts new chapters, and streams `LibraryUpdateStatus`
//! snapshots on a broadcast channel that the `libraryUpdateStatusChanged`
//! subscription consumes.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::broadcast;

use suwayomi_core::db::Db;
use suwayomi_core::schema::{ChapterRow, MangaRow};
use suwayomi_core::source::{SChapter, SManga};
use suwayomi_domain::source::SourceFetcher;

use crate::mutation_b4::{LibraryUpdateStatus, MangaJobStatus, MangaUpdateType, UpdaterJobsInfoType};
use crate::types::MangaType;

/// Shared updater handle (clonable; one per server).
#[derive(Clone)]
pub struct UpdateManager {
    db: Db,
    fetcher: Arc<dyn SourceFetcher>,
    tx: broadcast::Sender<LibraryUpdateStatus>,
    state: Arc<tokio::sync::Mutex<UpdaterState>>,
}

#[derive(Default)]
struct UpdaterState {
    running: bool,
    stop_requested: bool,
}

impl UpdateManager {
    pub fn new(db: Db, fetcher: Arc<dyn SourceFetcher>) -> Self {
        let (tx, _) = broadcast::channel(64);
        Self { db, fetcher, tx, state: Arc::new(tokio::sync::Mutex::new(UpdaterState::default())) }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LibraryUpdateStatus> {
        self.tx.subscribe()
    }

    pub async fn is_running(&self) -> bool {
        self.state.lock().await.running
    }

    /// Starts the update job. No-op if already running.
    pub async fn start(&self, categories: Option<Vec<i32>>) {
        let mut st = self.state.lock().await;
        if st.running {
            return;
        }
        st.running = true;
        st.stop_requested = false;
        let mgr = self.clone();
        tokio::spawn(async move {
            mgr.update_loop(categories).await;
            mgr.state.lock().await.running = false;
        });
    }

    pub async fn stop(&self) {
        self.state.lock().await.stop_requested = true;
    }

    fn emit(&self, status: LibraryUpdateStatus) {
        let _ = self.tx.send(status);
    }

    async fn update_loop(&self, categories: Option<Vec<i32>>) {
        let pool = self.db.pool();
        let ids = match library_manga_ids(pool, categories.as_deref()).await {
            Ok(ids) => ids,
            Err(e) => {
                tracing::error!(%e, "updater: failed to load library");
                self.emit(LibraryUpdateStatus::idle());
                return;
            }
        };

        let total = ids.len() as i32;
        let mut finished = 0;
        let mut skipped = 0;
        let mut manga_updates: Vec<MangaUpdateType> = Vec::new();

        self.emit(LibraryUpdateStatus {
            category_updates: vec![],
            jobs_info: UpdaterJobsInfoType {
                finished_jobs: 0,
                is_running: true,
                skipped_categories_count: 0,
                skipped_mangas_count: 0,
                total_jobs: total,
            },
            manga_updates: vec![],
        });

        for manga_id in ids {
            if self.state.lock().await.stop_requested {
                break;
            }
            let (status, manga_type) = match self.update_one(pool, manga_id).await {
                Ok(new_chapters) => {
                    if new_chapters == 0 {
                        skipped += 1;
                        (MangaJobStatus::Skipped, None)
                    } else {
                        (MangaJobStatus::Complete, None)
                    }
                }
                Err(_) => (MangaJobStatus::Failed, None),
            };
            // refresh the manga type for the event payload
            let manga_type = match manga_type {
                Some(t) => Some(t),
                None => fetch_manga_type(pool, manga_id).await.ok().flatten(),
            };
            if let Some(t) = manga_type {
                manga_updates.push(MangaUpdateType { status, manga: t });
            }
            finished += 1;
            self.emit(LibraryUpdateStatus {
                category_updates: vec![],
                jobs_info: UpdaterJobsInfoType {
                    finished_jobs: finished,
                    is_running: true,
                    skipped_categories_count: 0,
                    skipped_mangas_count: skipped,
                    total_jobs: total,
                },
                manga_updates: manga_updates.clone(),
            });
        }

        self.emit(LibraryUpdateStatus {
            category_updates: vec![],
            jobs_info: UpdaterJobsInfoType {
                finished_jobs: finished,
                is_running: false,
                skipped_categories_count: 0,
                skipped_mangas_count: skipped,
                total_jobs: total,
            },
            manga_updates,
        });
    }

    /// Fetches one manga's chapters from its source and inserts new ones.
    /// Returns the number of newly inserted chapters.
    async fn update_one(&self, pool: &PgPool, manga_id: i32) -> Result<usize, String> {
        let manga: MangaRow = sqlx::query_as("SELECT * FROM manga WHERE id = $1")
            .bind(manga_id)
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;

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

        let existing: Vec<SChapter> = sqlx::query_as::<_, ChapterRow>("SELECT * FROM chapter WHERE manga = $1")
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
        for (idx, ch) in chapters.iter().enumerate() {
            let res = sqlx::query(
                "INSERT INTO chapter (url, name, date_upload, chapter_number, scanlator, source_order, real_url, manga) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (url, manga) DO NOTHING",
            )
            .bind(&ch.url)
            .bind(&ch.name)
            .bind(ch.date_upload)
            .bind(ch.chapter_number)
            .bind(ch.scanlator.clone())
            .bind(idx as i32)
            .bind(ch.url.clone())
            .bind(manga_id)
            .execute(pool)
            .await;
            match res {
                Ok(r) => inserted += r.rows_affected() as usize,
                Err(e) => tracing::warn!(manga_id, url = %ch.url, "updater: insert chapter failed: {e}"),
            }
        }

        let _ = sqlx::query(
            "UPDATE manga SET last_fetched_at = $1, chapters_last_fetched_at = $1, last_modified_at = $1, version = version + 1 WHERE id = $2",
        )
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(manga_id)
        .execute(pool)
        .await;

        Ok(inserted)
    }
}

/// Library manga ids, optionally restricted to the given categories.
async fn library_manga_ids(pool: &PgPool, categories: Option<&[i32]>) -> Result<Vec<i32>, sqlx::Error> {
    match categories {
        Some(cats) if !cats.is_empty() => {
            sqlx::query_as::<_, (i32,)>(
                "SELECT DISTINCT m.id FROM manga m JOIN category_manga cm ON cm.manga = m.id \
                 WHERE m.in_library = TRUE AND cm.category = ANY($1) ORDER BY m.id",
            )
            .bind(cats)
            .fetch_all(pool)
            .await
            .map(|rows| rows.into_iter().map(|r| r.0).collect())
        }
        _ => {
            sqlx::query_as::<_, (i32,)>("SELECT id FROM manga WHERE in_library = TRUE ORDER BY id")
                .fetch_all(pool)
                .await
                .map(|rows| rows.into_iter().map(|r| r.0).collect())
        }
    }
}

async fn fetch_manga_type(pool: &PgPool, manga_id: i32) -> Result<Option<MangaType>, sqlx::Error> {
    let row: Option<MangaRow> = sqlx::query_as("SELECT * FROM manga WHERE id = $1").bind(manga_id).fetch_optional(pool).await?;
    Ok(row.map(|r| MangaType::from_row(&r)))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use suwayomi_core::config::ServerConfig;
    use suwayomi_core::source::{MangasPage, SChapter, SManga};
    use suwayomi_domain::source::SourceFetcher;

    use super::*;
    use crate::mutation_b4::MangaJobStatus;
    use crate::state::GraphQLState;

    /// Fake source: `fetch_manga_update` returns two fixed chapters.
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
        ) -> suwayomi_domain::error::Result<(SManga, Vec<SChapter>)> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok((
                SManga::default(),
                vec![
                    SChapter { url: "/c/1".into(), name: "Ch 1".into(), chapter_number: 1.0, scanlator: None, date_upload: 1_700_000_000_000, memo: Default::default() },
                    SChapter { url: "/c/2".into(), name: "Ch 2".into(), chapter_number: 2.0, scanlator: Some("TL".into()), date_upload: 1_700_000_100_000, memo: Default::default() },
                ],
            ))
        }

        async fn get_popular_manga(&self, _source_id: i64, _page: u32) -> suwayomi_domain::error::Result<MangasPage> {
            Ok(MangasPage::default())
        }

        async fn get_latest_updates(&self, _source_id: i64, _page: u32) -> suwayomi_domain::error::Result<MangasPage> {
            Ok(MangasPage::default())
        }

        async fn search_manga(&self, _source_id: i64, _query: &str, _page: u32) -> suwayomi_domain::error::Result<MangasPage> {
            Ok(MangasPage::default())
        }

        fn supports_latest(&self, _source_id: i64) -> bool {
            true
        }
    }

    async fn setup() -> (Db, FakeFetcher) {
        let db = Db::connect_embedded(None).await.expect("connect embedded");
        db.migrate().await.expect("migrate");
        let pool = db.pool();
        sqlx::query("INSERT INTO extension (name, pkg_name, version_name, version_code, lang, content_warning) VALUES ('E','p','1',1,'en',0)")
            .execute(pool)
            .await
            .expect("ext");
        sqlx::query("INSERT INTO source (name, lang, extension) VALUES ('S','en',1)").execute(pool).await.expect("src");
        sqlx::query("INSERT INTO manga (url, title, in_library, source) VALUES ('/m','Manga One',TRUE,1)")
            .execute(pool)
            .await
            .expect("manga");
        (db, FakeFetcher::default())
    }

    #[tokio::test]
    async fn updater_inserts_chapters_and_emits_events() {
        let (db, fetcher) = setup().await;
        let state = GraphQLState::new(db.clone(), ServerConfig::default(), Arc::new(fetcher), None);
        let mut rx = state.update.subscribe();

        state.update.start(None).await;
        assert!(state.update.is_running().await, "updater should be running after start");

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
                            assert!(m.status == MangaJobStatus::Complete, "manga should complete with new chapters");
                        }
                        break;
                    }
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
                Err(_) => break,
            }
        }
        assert!(saw_running, "must see a running=true event");
        assert!(saw_complete, "must see a finished event with finished_jobs >= 1");

        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chapter").fetch_one(db.pool()).await.expect("count chapters");
        assert_eq!(n, 2, "two chapters inserted by the updater");
        let names: Vec<String> = sqlx::query_scalar("SELECT name FROM chapter ORDER BY source_order").fetch_all(db.pool()).await.expect("names");
        assert_eq!(names, vec!["Ch 1".to_string(), "Ch 2".to_string()]);
    }

    #[tokio::test]
    async fn updater_marks_manga_failed_when_source_errors() {
        let (db, _fetcher) = setup().await;
        let state = GraphQLState::new(db.clone(), ServerConfig::default(), Arc::new(suwayomi_domain::source::StubFetcher), None);
        let mut rx = state.update.subscribe();
        state.update.start(None).await;

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
        assert!(saw_failed, "manga should be marked Failed when the source errors");
    }
}
