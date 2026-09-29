//! 装配层建一次、各协议层共享的应用状态。
//!
//! 这里的每个字段都是**句柄**（`Clone` 只共享内部 `Arc`），所以 server 装配一次
//! 之后可以交给 REST / GraphQL / OPDS 三边用，看到的是同一份队列、同一份配置、
//! 同一份追踪器登录态。各自 `new` 一份会得到互不可见的副本——REST 的取页与
//! GraphQL 的订阅都读 `config`，分成两份时改设置只对其中一边生效。

use std::sync::Arc;

use suwayomi_core::auth::AuthContext;
use suwayomi_core::config::{AppPaths, RuntimeConfig};
use suwayomi_core::db::Db;
use suwayomi_domain::category::CategoryService;
use suwayomi_domain::category::category_manga::CategoryMangaService;
use suwayomi_domain::chapter::ChapterService;
use suwayomi_domain::download::DownloadManager;
use suwayomi_domain::extension_store::ExtensionStoreService;
use suwayomi_domain::manga::MangaService;
use suwayomi_domain::manga::library::LibraryService;
use suwayomi_domain::manga::manga_list::MangaListService;
use suwayomi_domain::page::PageService;
use suwayomi_domain::source::SourceBackend;
use suwayomi_domain::tracker::TrackerManager;
use suwayomi_domain::updater::UpdateManager;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    /// 运行时配置（env 基线 + `global_meta` 里持久化的 settings blob）。与
    /// GraphQL 侧共享同一个句柄，`setSettings` 改完不用重启就生效。
    pub config: RuntimeConfig,
    /// 认证参数（模式、凭据、会话/JWT 密钥），启动时解析一次后只读。
    pub auth: Arc<AuthContext>,
    /// Extension source fetcher (stub until the JVM sandbox loads real extensions).
    pub fetcher: SourceBackend,
    pub manga: MangaService,
    pub chapter: ChapterService,
    pub category: CategoryService,
    pub category_manga: CategoryMangaService,
    pub library: LibraryService,
    pub manga_list: MangaListService,
    pub page: PageService,
    /// Library updater — `/api/v1/update/*` 读它的状态并启停任务。
    pub update: UpdateManager,
    /// 追踪器（登录态、搜索、绑定、推送），与 GraphQL 侧共用同一个句柄。
    pub tracker: TrackerManager,
    /// Chapter download manager (queue + worker + event bus) — 装配层建好后注入，
    /// 与 GraphQL 侧是**同一个句柄**：队列与 worker 都藏在它内部，各自 new 会
    /// 变成两条互不可见的队列（在一个入口排的队，另一个入口既看不到也没人跑）。
    pub download: DownloadManager,
    /// Extension store: repo refresh + online install.
    pub extension_store: ExtensionStoreService,
    /// 扩展宿主的 base url（JVM 沙盒 / Android 宿主），没接扩展时为 None。
    /// 图标路由要用它去取「扩展 APK 里那张图」。
    pub sandbox_base: Option<String>,
    /// Bundled WebUI static directory (SPA hosting; empty = disabled).
    pub webui_dir: std::path::PathBuf,
    /// 进程内各根目录（数据 / 缓存 / 下载 / 本地图源），与 GraphQL 侧共享同一句柄。
    pub paths: AppPaths,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Db,
        config: impl Into<RuntimeConfig>,
        auth: Arc<AuthContext>,
        fetcher: SourceBackend,
        update: UpdateManager,
        tracker: TrackerManager,
        download: DownloadManager,
        sandbox_base: Option<String>,
        webui_dir: std::path::PathBuf,
        paths: AppPaths,
    ) -> Self {
        let manga = MangaService::new(db.clone(), fetcher.clone());
        let chapter = ChapterService::new(db.clone(), fetcher.clone()).with_tracker(tracker.clone());
        let category = CategoryService::new(db.clone());
        let category_manga = CategoryMangaService::new(db.clone());
        let library = LibraryService::new(db.clone(), manga.clone());
        let manga_list = MangaListService::new(db.clone(), fetcher.clone());
        let page = PageService::new(db.clone());
        let extension_store = ExtensionStoreService::new(
            db.clone(),
            sandbox_base.clone(),
            paths.extensions(),
            paths.extensions_bin(),
            paths.cache(),
        );
        Self {
            db,
            config: config.into(),
            auth,
            fetcher,
            manga,
            chapter,
            category,
            category_manga,
            library,
            manga_list,
            page,
            update,
            tracker,
            download,
            extension_store,
            sandbox_base,
            webui_dir,
            paths,
        }
    }
}
