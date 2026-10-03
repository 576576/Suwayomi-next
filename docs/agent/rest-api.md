# REST API v1 端点基线（兼容对照）

> 依据 `Suwayomi`（commit 4b2c19ab）`MangaAPI.kt` / `GlobalAPI.kt` / `OpdsAPI.kt` 生成。
> 前缀：所有端点位于 `/api/` 下（`ServerSubpath` 支持子路径部署时前缀可配置）。
> Rust 版须逐条对齐：方法、路径、参数、请求体、响应 JSON、状态码。
> 状态码映射（源 `JavalinSetup.kt`）：NPE/NoSuchElement→404；IOException→500；IllegalArgumentException→400；Unauthorized→401；Forbidden→403。
>
> **有意偏离 Suwayomi-Server 的一处**：`chapterIndex`（`manga/{mangaId}/chapter/{chapterIndex}` 系列、`download/{mangaId}/chapter/{chapterIndex}`、OPDS `series/{id}/chapter/{chapterIndex}`）按 **0-based** 解释，与 Mihon 的 `sourceOrder`、`.tachibk` 备份以及本仓的 `chapter.source_order` 列同一口径；Suwayomi-Server 是 1-based。配套的 Suwayomi-WebUI 已按 0-based 取值（`chapters[len - 1 - sourceOrder]`、`sourceOrder === 0` 即首章）。

## 1. Manga API（`/api/v1/`）——源 `MangaAPI.kt`

### extension

| 方法 | 路径 | 控制器 | 说明 |
| --- | --- | --- | --- |
| GET | `extension/list` | ExtensionController.list | 扩展列表 |
| GET | `extension/install/{pkgName}` | ExtensionController.install | 安装扩展 |
| POST | `extension/install` | ExtensionController.installFile | 上传文件安装 |
| GET | `extension/update/{pkgName}` | ExtensionController.update | 更新扩展 |
| GET | `extension/uninstall/{pkgName}` | ExtensionController.uninstall | 卸载扩展 |
| GET | `extension/icon/{pkgName}` | ExtensionController.icon | 扩展图标 |

### source

| 方法 | 路径 | 控制器 | 说明 |
| --- | --- | --- | --- |
| GET | `source/list` | SourceController.list | 数据源列表 |
| GET | `source/{sourceId}` | SourceController.retrieve | 数据源详情 |
| GET | `source/{sourceId}/popular/{pageNum}` | SourceController.popular | 热门漫画 |
| GET | `source/{sourceId}/latest/{pageNum}` | SourceController.latest | 最新漫画 |
| GET | `source/{sourceId}/preferences` | SourceController.getPreferences | 获取源偏好 |
| POST | `source/{sourceId}/preferences` | SourceController.setPreference | 设置源偏好 |
| GET | `source/{sourceId}/filters` | SourceController.getFilters | 获取过滤器 |
| POST | `source/{sourceId}/filters` | SourceController.setFilters | 设置过滤器 |
| GET | `source/{sourceId}/search` | SourceController.searchSingle | 搜索 |
| POST | `source/{sourceId}/quick-search` | SourceController.quickSearchSingle | 快速搜索 |
| ~~GET~~ | ~~`source/all/search`~~ | （TODO 注释） | 全局搜索（原版未实现） |

### manga

| 方法 | 路径 | 控制器 | 说明 |
| --- | --- | --- | --- |
| GET | `manga/{mangaId}` | MangaController.retrieve | 漫画详情 |
| GET | `manga/{mangaId}/full` | MangaController.retrieveFull | 完整漫画（含计数） |
| GET | `manga/{mangaId}/thumbnail` | MangaController.thumbnail | 封面图 |
| GET | `manga/{mangaId}/category` | MangaController.categoryList | 所属分类 |
| GET | `manga/{mangaId}/category/{categoryId}` | MangaController.addToCategory | 加入分类 |
| DELETE | `manga/{mangaId}/category/{categoryId}` | MangaController.removeFromCategory | 移出分类 |
| GET | `manga/{mangaId}/library` | MangaController.addToLibrary | 加入书库 |
| DELETE | `manga/{mangaId}/library` | MangaController.removeFromLibrary | 移出书库 |
| PATCH | `manga/{mangaId}/meta` | MangaController.meta | 修改 meta |
| GET | `manga/{mangaId}/chapters` | MangaController.chapterList | 章节列表 |
| POST | `manga/{mangaId}/chapter/batch` | MangaController.chapterBatch | 章节批量操作 |
| GET | `manga/{mangaId}/chapter/{chapterIndex}` | MangaController.chapterRetrieve | 章节详情 |
| PATCH | `manga/{mangaId}/chapter/{chapterIndex}` | MangaController.chapterModify | 修改章节 |
| PUT | `manga/{mangaId}/chapter/{chapterIndex}` | MangaController.chapterModify | 修改章节 |
| DELETE | `manga/{mangaId}/chapter/{chapterIndex}` | MangaController.chapterDelete | 删除章节 |
| PATCH | `manga/{mangaId}/chapter/{chapterIndex}/meta` | MangaController.chapterMeta | 修改章节 meta |
| GET | `manga/{mangaId}/chapter/{chapterIndex}/page/{index}` | MangaController.pageRetrieve | 页面详情 |

### chapter

| 方法 | 路径 | 控制器 | 说明 |
| --- | --- | --- | --- |
| POST | `chapter/batch` | MangaController.anyChapterBatch | 任意章节批量操作 |
| GET | `chapter/{chapterId}/download` | MangaController.downloadChapter | 下载章节文件 |
| HEAD | `chapter/{chapterId}/download` | MangaController.downloadChapter | 下载章节文件（HEAD） |

### category

| 方法 | 路径 | 控制器 | 说明 |
| --- | --- | --- | --- |
| GET | `category` | CategoryController.categoryList | 分类列表 |
| POST | `category` | CategoryController.categoryCreate | 创建分类 |
| PATCH | `category/reorder` | CategoryController.categoryReorder | 分类排序（**必须先于 {categoryId} 注册**） |
| GET | `category/{categoryId}` | CategoryController.categoryMangas | 分类漫画 |
| PATCH | `category/{categoryId}` | CategoryController.categoryModify | 修改分类 |
| DELETE | `category/{categoryId}` | CategoryController.categoryDelete | 删除分类 |
| PATCH | `category/{categoryId}/meta` | CategoryController.meta | 修改分类 meta |

### backup

| 方法 | 路径 | 控制器 | 说明 |
| --- | --- | --- | --- |
| POST | `backup/import` | BackupController.protobufImport | 导入备份 |
| POST | `backup/import/file` | BackupController.protobufImportFile | 上传文件导入 |
| POST | `backup/validate` | BackupController.protobufValidate | 校验备份 |
| POST | `backup/validate/file` | BackupController.protobufValidateFile | 校验上传备份 |
| GET | `backup/export` | BackupController.protobufExport | 导出备份 |
| GET | `backup/export/file` | BackupController.protobufExportFile | 导出备份文件 |

### downloads / download

| 方法 | 路径 | 控制器 | 说明 |
| --- | --- | --- | --- |
| WS | `downloads` | DownloadController.downloadsWS | 下载进度推送 |
| GET | `downloads/start` | DownloadController.start | 开始队列 |
| GET | `downloads/stop` | DownloadController.stop | 停止队列 |
| GET | `downloads/clear` | DownloadController.clear | 清空队列 |
| GET | `download/{mangaId}/chapter/{chapterIndex}` | DownloadController.queueChapter | 入队下载 |
| DELETE | `download/{mangaId}/chapter/{chapterIndex}` | DownloadController.unqueueChapter | 取消下载 |
| PATCH | `download/{mangaId}/chapter/{chapterIndex}/reorder/{to}` | DownloadController.reorderChapter | 队列重排 |
| POST | `download/batch` | DownloadController.queueChapters | 批量入队 |
| DELETE | `download/batch` | DownloadController.unqueueChapters | 批量取消 |

### update

| 方法 | 路径 | 控制器 | 说明 |
| --- | --- | --- | --- |
| GET | `update/recentChapters/{pageNum}` | UpdateController.recentChapters | 最近章节 |
| POST | `update/fetch` | UpdateController.categoryUpdate | 触发更新 |
| POST | `update/reset` | UpdateController.reset | 重置更新 |
| GET | `update/summary` | UpdateController.updateSummary | 更新摘要 |
| WS | `update` | UpdateController.categoryUpdateWS | 更新进度推送 |

### track

| 方法 | 路径 | 控制器 | 说明 |
| --- | --- | --- | --- |
| GET | `track/list` | TrackController.list | 追踪列表 |
| POST | `track/login` | TrackController.login | 登录追踪服务 |
| POST | `track/logout` | TrackController.logout | 登出 |
| POST | `track/search` | TrackController.search | 搜索 |
| POST | `track/bind` | TrackController.bind | 绑定 |
| POST | `track/update` | TrackController.update | 更新 |
| GET | `track/{trackerId}/thumbnail` | TrackController.thumbnail | 追踪服务图标 |

## 2. Global API（`/api/v1/`）——源 `GlobalAPI.kt`

| 方法 | 路径 | 控制器 | 说明 |
| --- | --- | --- | --- |
| GET | `meta` | GlobalMetaController.getMeta | 全局 meta |
| PATCH | `meta` | GlobalMetaController.modifyMeta | 修改全局 meta |
| GET | `settings/about` | SettingsController.about | 关于信息 |
| GET | `settings/check-update` | SettingsController.checkUpdate | 检查更新 |
| GET | `webview` | WebViewController.webview | WebView 页面 |
| WS | `webview` | WebViewController.webviewWS | WebView WS |

## 3. OPDS API（`/api/opds/v1.2/`）——源 `OpdsAPI.kt`

| 方法 | 路径 | 控制器 | 说明 |
| --- | --- | --- | --- |
| GET | `opds/v1.2` | OpdsV1Controller.rootFeed | 根目录 |
| GET | `opds/v1.2/search` | OpdsV1Controller.searchFeed | 搜索说明 |
| GET | `opds/v1.2/history` | OpdsV1Controller.historyFeed | 阅读历史 |
| GET | `opds/v1.2/library-updates` | OpdsV1Controller.libraryUpdatesFeed | 库更新 |
| GET | `opds/v1.2/explore` | OpdsV1Controller.exploreSourcesFeed | 在线源列表 |
| GET | `opds/v1.2/explore/source/{sourceId}` | OpdsV1Controller.exploreSourceFeed | 源浏览 |
| GET | `opds/v1.2/library/series` | OpdsV1Controller.seriesFeed | 库漫画/搜索结果 |
| GET | `opds/v1.2/library/sources` | OpdsV1Controller.librarySourcesFeed | 库源导航 |
| GET | `opds/v1.2/library/categories` | OpdsV1Controller.categoriesFeed | 分类导航 |
| GET | `opds/v1.2/library/genres` | OpdsV1Controller.genresFeed | 题材导航 |
| GET | `opds/v1.2/library/statuses` | OpdsV1Controller.statusesFeed | 状态导航 |
| GET | `opds/v1.2/library/languages` | OpdsV1Controller.languagesFeed | 语言导航 |
| GET | `opds/v1.2/source/{sourceId}` | OpdsV1Controller.librarySourceFeed | 源漫画 |
| GET | `opds/v1.2/category/{categoryId}` | OpdsV1Controller.categoryFeed | 分类漫画 |
| GET | `opds/v1.2/genre/{genre}` | OpdsV1Controller.genreFeed | 题材漫画 |
| GET | `opds/v1.2/status/{statusId}` | OpdsV1Controller.statusMangaFeed | 状态漫画 |
| GET | `opds/v1.2/language/{langCode}` | OpdsV1Controller.languageFeed | 语言漫画 |
| GET | `opds/v1.2/series/{seriesId}/chapters` | OpdsV1Controller.seriesChaptersFeed | 章节列表 |
| GET | `opds/v1.2/series/{seriesId}/chapter/{chapterIndex}/metadata` | OpdsV1Controller.chapterMetadataFeed | 章节元数据 |

## 4. GraphQL（`/api/graphql`）——源 `GraphQL.kt`

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| POST | `graphql` | GraphQL 执行 |
| WS | `graphql` | GraphQL WebSocket（订阅，Apollo 协议） |
| GET | `graphql` | GraphQL Playground |
| GET | `graphql/files/backup/{file}` | 备份文件下载 |

## 5. 核心 HTTP（无 /api 前缀）——源 `JavalinSetup.kt`

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/login.html` | 登录页（SIMPLE_LOGIN 模式） |
| POST | `/login.html` | 登录提交（form: user/pass，成功 303 重定向） |
| GET | `/{webui 静态资源}` | WebUI 托管（WebInterfaceManager） |

认证规则（beforeMatched）：`/login.html`、`site.webmanifest`、`manifest.json`、首页图标免认证；OPTIONS 免认证；`SIMPLE_LOGIN` 未登录 → 302 到 login.html（带 redirect 参数）；`BASIC_AUTH` 校验失败 → 401 + WWW-Authenticate: Basic。

## 6. OPDS API v2（`/api/opds/v2/`）——本仓新增，参考实现无

参考实现**没有**这一组端点，所以本节不是兼容对照，而是本仓自己的契约：与 §3 的 1.2 **并列提供**同一份数据的第二种编码 —— OPDS 2.0（feed 媒体类型 `application/opds+json`），章节的正文用 Readium Divina 清单（`application/divina+json`）表达。

路由形状与 1.2 逐一对应：把 §3 表里的 `opds/v1.2` 换成 `opds/v2`，去掉 1.2 的 `search` 与 `series/{id}/chapter/{chapterIndex}/metadata`，再加一条清单，共 **18 条**。

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `opds/v2` | 根目录（导航） |
| GET | `opds/v2/explore` | 在线源列表（导航） |
| GET | `opds/v2/explore/source/{sourceId}` | 源浏览（远程条目，acquisition 指向源页面） |
| GET | `opds/v2/library/series` | 库漫画 / 搜索结果 |
| GET | `opds/v2/library/sources` | 库源导航 |
| GET | `opds/v2/library/categories` | 分类导航 |
| GET | `opds/v2/library/genres` | 题材导航 |
| GET | `opds/v2/library/statuses` | 状态导航 |
| GET | `opds/v2/library/languages` | 语言导航 |
| GET | `opds/v2/history` | 阅读历史 |
| GET | `opds/v2/library-updates` | 库更新 |
| GET | `opds/v2/source/{sourceId}` | 源漫画 |
| GET | `opds/v2/category/{categoryId}` | 分类漫画 |
| GET | `opds/v2/genre/{genre}` | 题材漫画 |
| GET | `opds/v2/status/{statusId}` | 状态漫画 |
| GET | `opds/v2/language/{langCode}` | 语言漫画 |
| GET | `opds/v2/series/{seriesId}/chapters` | 章节列表 |
| GET | `opds/v2/series/{seriesId}/chapter/{chapterId}/manifest` | 章节的 Divina 清单 |

- 公共 query：`lang=`（缺省 `en`）与 `pageNumber=`；`library/series` 另认 `query=` / `author=` / `title=`（与 §3 同规则切搜索分支），章节 feed 另认 `sort=` / `filter=`（缺省由 `opdsChapterSortOrder` 决定，客户端显式给了就听客户端的）。
- **清单路径用的是章节的库内 `id`，不是 1.2 那个 `chapterIndex`（`source_order`）** —— 章节的 acquisition 链接由 feed 自己给出，客户端照抄即可；这个取值口径与 §0 的偏离说明不冲突。
- 路径要写**无尾斜杠**的形式，与 §1 的实测一致（带尾斜杠会落到 WebUI 的 SPA fallback，返回 `200 text/html`）。
- 认证与 1.2 同源，不需要为 v2 单独配：`/api/` 前缀进「匿名 401」，`/api/opds/` 前缀放行 `?token=`（`crates/suwayomi-api/src/auth.rs` 的两处前缀判定）。
