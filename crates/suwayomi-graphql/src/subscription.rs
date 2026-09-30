//! Subscription root — mirrors `graphql/subscriptions/*.kt`.
//! Each stream emits an initial snapshot then follows its live broadcast
//! channel (download / update / sync events).

use std::collections::{HashMap, HashSet};

use async_graphql::{Context, InputObject, SimpleObject, Subscription};
use futures::StreamExt;
use futures::stream::{self, Stream};

use suwayomi_domain::download::DownloadEvent;

use crate::mutation_b4::{
    DownloadState, DownloadType, DownloadUpdate, DownloadUpdateType, DownloadUpdates, LibraryUpdateStatus,
};
use crate::query::UpdateStatusPayload;

#[derive(SimpleObject, Clone)]
pub struct UpdaterUpdates {
    pub category_updates: Vec<crate::mutation_b4::CategoryUpdateType>,
    pub initial: Option<LibraryUpdateStatus>,
    pub jobs_info: crate::mutation_b4::UpdaterJobsInfoType,
    pub manga_updates: Vec<crate::mutation_b4::MangaUpdateType>,
    pub omitted_updates: bool,
}

#[derive(InputObject)]
pub struct DownloadChangedInput {
    pub max_updates: Option<i32>,
}

#[derive(InputObject)]
pub struct LibraryUpdateStatusChangedInput {
    pub max_updates: Option<i32>,
}

#[derive(Default)]
pub struct SubscriptionRoot;

#[Subscription(name = "Subscription")]
impl SubscriptionRoot {
    /// Mirrors `downloadChanged` (deprecated). Streams live queue snapshots.
    async fn download_changed(&self, ctx: &Context<'_>) -> impl Stream<Item = crate::mutation_b4::DownloadStatus> {
        let state = match ctx.data::<crate::state::GraphQLState>() {
            Ok(s) => s.clone(),
            Err(_) => return futures::stream::empty().boxed(),
        };
        let rx = state.download.subscribe();
        futures::stream::unfold((state, rx), |(state, mut rx)| async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let DownloadEvent::Progress { chapter_id, progress } = event {
                            state.download.set_progress(chapter_id, progress);
                        }
                        let status = crate::mutation_b4::download_status(&state).await.ok()?;
                        return Some((status, (state.clone(), rx)));
                    }
                    // 落后于广播只是丢事件；match 已是循环体末尾，等价于 continue。
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        })
        .boxed()
    }

    /// Mirrors `downloadStatusChanged(input:)`. Streams live queue snapshots
    /// from the download manager's broadcast channel.
    async fn download_status_changed(
        &self,
        ctx: &Context<'_>,
        _input: DownloadChangedInput,
    ) -> impl Stream<Item = DownloadUpdates> {
        let state = match ctx.data::<crate::state::GraphQLState>() {
            Ok(s) => s.clone(),
            Err(_) => return futures::stream::empty().boxed(),
        };
        let rx = state.download.subscribe();
        // 上一条事件发出去的队列，用来算差量。空 map = 客户端手上什么都没有，
        // 此时整条队列都算新增。
        let prev: HashMap<i32, DownloadType> = HashMap::new();
        futures::stream::unfold((state, rx, prev), |(state, mut rx, mut prev)| async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        // Apply per-page progress ticks to the queued job so
                        // the emitted snapshot reports real progress.
                        if let DownloadEvent::Progress { chapter_id, progress } = event {
                            state.download.set_progress(chapter_id, progress);
                        }
                        let status = crate::mutation_b4::download_status(&state).await.ok()?;
                        let updates = diff_download_updates(&prev, &status.queue);
                        prev = status.queue.iter().map(|d| (d.chapter.id, d.clone())).collect();
                        return Some((
                            DownloadUpdates {
                                initial: Some(status.queue),
                                omitted_updates: false,
                                state: status.state,
                                updates,
                            },
                            (state.clone(), rx, prev),
                        ));
                    }
                    // 落后于广播只是丢事件；match 已是循环体末尾，等价于 continue。
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        })
        .boxed()
    }

    /// Mirrors `syncStatusChanged`.
    async fn sync_status_changed(&self, _ctx: &Context<'_>) -> impl Stream<Item = crate::query::SyncStatus> {
        stream::once(async {
            crate::query::SyncStatus {
                backup_restore_id: None,
                end_date: None,
                error_message: None,
                start_date: crate::scalars::LongString(0),
                state: crate::query::SyncState::Success,
            }
        })
    }

    /// Mirrors `libraryUpdateStatusChanged(input:)`.
    /// Streams live `LibraryUpdateStatus` snapshots from the updater's
    /// broadcast channel (real events once `updateLibrary` starts a job).
    async fn library_update_status_changed(
        &self,
        ctx: &Context<'_>,
        _input: LibraryUpdateStatusChangedInput,
    ) -> impl Stream<Item = UpdaterUpdates> {
        let state = match ctx.data::<crate::state::GraphQLState>() {
            Ok(s) => s.clone(),
            Err(_) => {
                return futures::stream::empty().boxed();
            }
        };
        let rx = state.update.subscribe();
        futures::stream::unfold(rx, |mut rx| async move {
            let status = loop {
                match rx.recv().await {
                    Ok(s) => break s,
                    // 落后于广播只是丢事件；match 已是循环体末尾，等价于 continue。
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            };
            Some((
                UpdaterUpdates {
                    category_updates: status.category_updates.into_iter().map(Into::into).collect(),
                    initial: None,
                    jobs_info: status.jobs_info.into(),
                    manga_updates: status.manga_updates.into_iter().map(Into::into).collect(),
                    omitted_updates: false,
                },
                rx,
            ))
        })
        .boxed()
    }

    /// Mirrors `updateStatusChanged` (deprecated).
    async fn update_status_changed(&self, _ctx: &Context<'_>) -> impl Stream<Item = UpdateStatusPayload> {
        stream::once(async { UpdateStatusPayload::idle() })
    }
}

/// 两条队列快照之间的差量，对应上游 `DownloadUpdates.updates`。
///
/// 队伍里没有的按 `Dequeued` 报（WebUI 据此从缓存里删），新出现的按 `Queued`，
/// 其余按状态 / 进度 / 位置变化取对应类型。**没变化的不发** —— 每次事件都把整条
/// 队列当更新推一遍，客户端的增量合并逻辑会被反复触发。
fn diff_download_updates(prev: &HashMap<i32, DownloadType>, current: &[DownloadType]) -> Vec<DownloadUpdate> {
    let update =
        |download: &DownloadType, r#type: DownloadUpdateType| DownloadUpdate { download: download.clone(), r#type };

    let mut updates = Vec::new();
    let mut seen = HashSet::with_capacity(current.len());
    for item in current {
        let id = item.chapter.id;
        seen.insert(id);
        let Some(before) = prev.get(&id) else {
            updates.push(update(item, DownloadUpdateType::Queued));
            continue;
        };
        let kind = if before.state != item.state {
            Some(match item.state {
                DownloadState::Finished => DownloadUpdateType::Finished,
                DownloadState::Error => DownloadUpdateType::Error,
                DownloadState::Queued | DownloadState::Downloading => DownloadUpdateType::Progress,
            })
        } else if before.progress != item.progress {
            Some(DownloadUpdateType::Progress)
        } else if before.position != item.position {
            Some(DownloadUpdateType::Position)
        } else {
            None
        };
        if let Some(kind) = kind {
            updates.push(update(item, kind));
        }
    }

    // 按 id 排序再发：HashMap 的迭代顺序不定，同一份队列会产出不同顺序的 updates，
    // 让事件流无法逐字节比对。
    let mut removed: Vec<i32> = prev.keys().copied().filter(|id| !seen.contains(id)).collect();
    removed.sort_unstable();
    for id in removed {
        if let Some(item) = prev.get(&id) {
            updates.push(update(item, DownloadUpdateType::Dequeued));
        }
    }
    updates
}
