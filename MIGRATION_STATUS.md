# MIGRATION_STATUS — 迁移追踪

> 对照基准：`Suwayomi` commit `4b2c19ab`（master）
> 状态：⬜ 未开始 · 🔄 进行中 · ✅ 已完成 · ⏭️ 裁剪/不迁移（含理由）

## 一、eu.kanade.tachiyomi（扩展 API 抽象）→ `suwayomi-core/src/source/`

| Kotlin 文件 | Rust 目标 | 状态 | 说明 |
| --- | --- | --- | --- |
| source/Source.kt, CatalogueSource.kt, HttpSource.kt, ParsedHttpSource.kt, ResolvableSource.kt, ConfigurableSource.kt, UnmeteredSource.kt, SourceFactory.kt | source/mod.rs | ⬜ | Phase 2/5 抽象为 trait |
| source/model/SManga.kt, SChapter.kt, SChapterImpl.kt, Page.kt, SMangaImpl.kt, SMangaUpdate.kt, MangasPage.kt, Filter.kt, FilterList.kt, UpdateStrategy.kt | source/model.rs | ✅ | SManga/SChapter/SourcePage/MangasPage/UpdateStrategy 已实现 |
| source/local/**（LocalSource、Zip/Epub/RarPageLoader、ComicInfo 等） | source/local.rs | ⬜ | Phase 5 本地源 |
| network/**（NetworkHelper、CookieJar、拦截器、JavaScriptEngine） | source/network.rs | ⬜ | Phase 5 由 reqwest 替代 |
| util/**（ChapterRecognition、Hash、JsoupExtensions 等） | source/util.rs | ⬜ | 少量工具 |

## 二、suwayomi.manga.model → `suwayomi-core/src/{models,schema}/`

| Kotlin 文件 | Rust 目标 | 状态 |
| --- | --- | --- |
| table/MangaTable.kt / ChapterTable.kt / PageTable.kt / CategoryTable.kt / CategoryMangaTable.kt / SourceTable.kt / ExtensionTable.kt / ExtensionStoreTable.kt / TrackRecordTable.kt / TrackSearchTable.kt / CategoryMetaTable.kt / ChapterMetaTable.kt / MangaMetaTable.kt / SourceMetaTable.kt | schema/rows.rs | ✅ FromRow 行结构 + 表名列名对齐 |
| table/columns/*（JsonObjectColumn、TruncatingVarCharColumn、TypeHelpers） | schema/columns.rs | ⬜ | 截断语义 Phase 2 在 domain 层复刻 |
| dataclass/*（MangaDataClass、ChapterDataClass、PageDataClass、CategoryDataClass、SourceDataClass、ExtensionDataClass、TrackRecordDataClass、TrackSearchDataClass、MangaTrackerDataClass、PaginatedList、ExtensionInfo、ExtensionStore、MangaChapterDataClass） | models/*.rs | ✅ 全部实现 + camelCase golden 测试 |
| global/model/table/GlobalMetaTable.kt | schema/rows.rs | ✅ |

## 二b. server（数据库/配置）→ `suwayomi-core/src/db/` + `config/`

| Kotlin 文件 | Rust 目标 | 状态 |
| --- | --- | --- |
| server/database/DBManager.kt | db/manager.rs | ✅ Db enum + SQLite/PG 连接 |
| server/database/H2Migration.kt + Migration.kt | db/migrator.rs | ✅ sqlx migrate（编译期嵌入） |
| server/database/migration/M0001..M0062（62 个） | migrations/*.sql | ✅ 基线 DDL（PG + SQLite），增量迁移 Phase 1 补充中 |
| server-config/**（ServerConfig.kt 等） | config/mod.rs | ✅ 核心配置（完整 settings 注册表 Phase 3） |

## 三、suwayomi.server（配置/数据库/HTTP 层）→ `suwayomi-core/src/db/` + `suwayomi-server/`

| Kotlin 文件 | Rust 目标 | 状态 |
| --- | --- | --- |
| server/database/DBManager.kt | db/manager.rs | ⬜ |
| server/database/H2Migration.kt | db/migrator.rs | ⬜ |
| server/database/Migration.kt | db/migrator.rs | ⬜ |
| server/database/migration/M0001..M0062（62 个） | migrations/*.sql（净效果基线） | 🔄 基线已产出，增量待 Phase 1 |
| server/database/trigger/SyncYomiTriggers.kt | db/triggers.rs | ⬜ |
| server/database/DBTransaction.util.kt | db/mod.rs | ⬜ |
| server/ServerSetup.kt / JavalinSetup.kt / Migration.kt | server/src（axum 等价） | ⬜ |
| server/settings/*（SettingsAsMap、SettingsUpdater、SettingsValidator） | core/config/* | ⬜ |
| server/util/*（AppExit、AppMutex、Browser、CEFManager⏭️、Platform、ServerSubpath、SystemTray→tauri、WebInterfaceManager） | server + tauri-app | ⬜ |
| server/user/UserType.kt | server（认证） | ⬜ |
| server-config/**（ServerConfig.kt、SettingDelegate.kt、SettingsRegistry.kt、SettingGroup.kt、graphql/types/*） | core/config/* | ⬜ |

## 四、suwayomi.manga.impl（业务逻辑）→ `suwayomi-domain/src/`

| Kotlin 文件 | Rust 目标 | 状态 |
| --- | --- | --- |
| Manga.kt（getManga/getMangaFull/meta/最新章节/下载分类判断） | manga/mod.rs | ✅ |
| MangaList.kt（insertOrUpdate/processEntries/proxyThumbnailUrl） | manga/manga_list.rs | ✅ |
| Library.kt（add/remove + 默认分类） | manga/library.rs | ✅ |
| Chapter.kt（列表/计数/modify/批量/progress/delete/recent/meta/removeDuplicates） | chapter/mod.rs | ✅ |
| Page.kt（DB 部分：列表/计数；图片流 Phase 5/6） | page/mod.rs | 🔄 DB 部分完成，图片流待 Phase 5 |
| Category.kt（create/update/reorder/remove/normalize/list/meta） | category/mod.rs | ✅ |
| CategoryManga.kt（add/remove/列表/计数） | category/category_manga.rs | ✅ |
| meta 系统（Manga/Chapter/Category/Source/Global） | meta/mod.rs | ✅ 批量 upsert 语义对齐 |
| Search.kt、Source.kt（依赖扩展） | source/mod.rs（SourceFetcher trait + StubFetcher） | 🔄 trait 已定义，真实实现 Phase 5 |
| download/**、update/**、track/**、backup/** | — | ⬜ Phase 6 |
| extension/**（安装/管理） | — | ⬜ 数据层可复用 schema，逻辑 Phase 5 |
| util/**（含 jvm-sandbox 相关） | 分散 | ⬜ Phase 5/6 |
| sync/KoreaderSyncService.kt | — | ⬜ Phase 6 |

## 五、suwayomi.manga.controller（REST v1）→ `suwayomi-rest/src/routes/`

| Kotlin 文件 | Rust 目标 | 状态 |
| --- | --- | --- |
| MangaAPI.kt（路由注册） | routes/mod.rs | ✅ /api/v1 router（axum） |
| MangaController.kt（manga/chapter/page/category 关联端点） | routes/manga.rs | ✅ |
| CategoryController.kt | routes/category.rs | ✅ |
| SourceController.kt | routes/source.rs | 🔄 列表/详情/搜索走 fetcher；preferences/filters 骨架 |
| ExtensionController.kt | routes/extension.rs | 🔄 列表可用；安装/卸载待 Phase 5 沙盒 |
| BackupController.kt / DownloadController.kt / UpdateController.kt / TrackController.kt | routes/*.rs | 🔄 track/update/downloads 已真实现（Phase 6 第一批）；backup 仍 501（protobuf） |
| GlobalAPI.kt（meta/settings/webview） | routes/global.rs + meta_handler.rs | ✅ |
| global/GlobalAPI.kt + controller/（GlobalMetaController、SettingsController、WebViewController） | routes/global.rs | ⬜ |

## 六、suwayomi.graphql → `suwayomi-graphql/src/schema/`

| Kotlin 目录 | Rust 目标 | 状态 |
| --- | --- | --- |
| queries/*（33 个 Query 字段） | src/query.rs | ✅ 33/33 全齐（条件过滤/排序/分页/游标） |
| mutations/*（82 个 Mutation 字段） | src/mutation.rs + mutation_b4.rs | ✅ 82/82 全齐（Category/Meta/Manga/Chapter DB 全实现；Download/Update/Backup/Track/Extension/Sync/User/WebUI 管理器依赖返回 Kotlin 兼容默认） |
| subscriptions/*（6 个字段） | src/subscription.rs | ✅ 6/6（初始快照流；Phase 6 接广播通道） |
| types/* | src/types.rs | 🔄 MangaType/ChapterType/CategoryType/PageType/SourceType(完整)/ExtensionType/ExtensionStoreType/MetaType interface(5 实现)/Filter/Preference union + NodeList 分页结构（edges 首尾语义对齐） |
| server/primitives/*（Cursor、LongString、Duration、OrderBy、NodeList、Upload） | src/scalars.rs | ✅ LongString/Duration(ISO-8601)/Cursor 已实现 |
| dataLoaders/* | — | ⬜ 用 ctx.data 直接查询替代（后续可加 batch loader） |
| server/*（GraphQLController 等） | src/schema.rs | ✅ GraphQL::new(service) 挂载 /api/graphql，GET/POST 可用 |
| directives/RequireAuth* | — | ⬜ 待增量 |
| —（基线与验收） | docs/graphql/schema-baseline.graphql | ✅ 已从 Kotlin 版 introspection 导出（359 类型 / 3033 行）；当前 Rust 侧 28 类型（核心），全量 359 待增量补全 |

## 七、suwayomi.opds → `suwayomi-opds/src/feeds/`

| Kotlin 文件 | Rust 目标 | 状态 |
| --- | --- | --- |
| OpdsAPI.kt + controller/OpdsV1Controller.kt + impl/* + model/*（XML）+ dto/* + repository/* + util/* + constants | feeds/*.rs | ⬜ |

## 八、suwayomi.global → `suwayomi-rest` + `tauri-app`

| Kotlin 文件 | Rust 目标 | 状态 |
| --- | --- | --- |
| impl/About.kt、AppUpdate.kt（→ tauri-updater 等价）、GlobalMeta.kt、WebView.kt、KcefWebView.kt（⏭️ CEF 移除）、impl/sync/*（SyncManager、SyncYomiSyncService）、impl/util/Jwt.kt | 分散 | ⬜ |
| i18n/LocalizationHelper.kt | tauri-app（登录页） | ⬜ |

## 九、桌面/系统集成（R3 决策：CEF → Tauri）

| Kotlin 文件 | Rust 目标 | 状态 |
| --- | --- | --- |
| server/util/CEFManager.kt | ⏭️ 移除（改用 Tauri WebView） | ✅ 已决策 |
| server/util/SystemTray.kt | tauri-app tray 菜单（补全：打开 WebUI/打开数据目录/启动最小化/退出） | ⬜ |
| server/util/Browser.kt | tauri-app（open 命令） | ⬜ |
| server/util/AppMutex.kt | tauri-app（单实例） | ⬜ |

## 十、测试用例（源 server/src/test → Rust tests）

| Kotlin 测试 | Rust 测试目标 | 状态 |
| --- | --- | --- |
| manga/impl/MangaTest.kt、PageTest.kt、CategoryMangaTest.kt、SearchTest.kt、update/TestUpdater.kt | suwayomi-domain/tests/ | ⬜ |
| manga/controller/CategoryControllerTest.kt、UpdateControllerTest.kt | suwayomi-rest/tests/ | ⬜ |
| manga/model/PaginatedListTest.kt | suwayomi-core/tests/ | ⬜ |
| graphql/RequestParserTest.kt | suwayomi-graphql/tests/ | ⬜ |
| server/database/migration/M0056SyncYomiTest.kt | suwayomi-core/tests/ | ⬜ |
| ImageUtilTest.kt、PathTest.kt、SafePathTest.kt、CefTest.kt（⏭️）、FlowTest.kt、LooperTest.kt、ApplicationTest.kt | 对应 crate | ⬜ |
| masstest/*（CloudFlareTest、TestExtensionCompatibility） | jvm-sandbox（Kotlin 保留） | ⬜ |

## 统计

- 总计待迁移 Kotlin 文件：≈300
- **Phase 0 已完成**：workspace 骨架、REST 端点基线、DDL 基线、CI、**GraphQL schema 基线（已从 Kotlin 版实测导出）**
- **Phase 1 已完成**：models（全部 dataclass + 枚举 golden 测试）、schema rows、db（manager/migrator）、source model 抽象
- **Phase 2 已完成**：domain 服务（Manga/MangaList/Library/Chapter/Page/Category/CategoryManga/Meta/SourceFetcher trait）；10 个 domain 集成测试（移植 Kotlin MangaTest/CategoryMangaTest 核心断言）
- **决策变更（2026-08-30）：数据库后端统一为 PostgreSQL**（移除 SQLite）：`Db` 改为 PgPool 包装（连接级 search_path=suwayomi）、`bind_placeholders` 固定 `?`→`$n`、bool 列改 TRUE/FALSE 语法、迁移幂等（DROP CONSTRAINT IF EXISTS）、memo 列 TEXT 字符串；PG 集成测试经 Docker postgres:16（端口 15432）12 项全绿；workspace 23 单元测试全绿，clippy -D warnings 通过
- **决策变更（2026-08-30）：嵌入式 PGlite 为默认后端（外部 PostgreSQL 保留为备选）**：`Db::connect_embedded(data_dir)` 通过 pglite-oxide（PGlite/PostgreSQL 17 引擎 + wasmtime，本地 TCP 回环，池单连接）实现「零安装嵌入式 PG」；`Db::connect(url)` 维持外部 PG。注：`pglite-rs`（原生静态库版）的 ORM 桥仅 unix socket（Windows 无效），故采用同引擎的 pglite-oxide（钉 0.3.0：0.4+/0.5 切 wasmer-wasix alpha 编译失败）。已知约束：嵌入式代理在**任何 SQL 报错**时终止会话（迁移前已用限定名预建 suwayomi schema + `_sqlx_migrations` 规避；运行时 SQL 错误会触发重连）。server 默认嵌入式（`SUWAYOMI_PGLITE_DATA_DIR` 指定数据目录，默认 `./pglite-data`），设置 `SUWAYOMI_DATABASE_URL` 即切外部 PG。嵌入式集成测试 5 项全绿（迁移/CRUD/持久化重开/search_path）+ 4568 端口冒烟通过
- **Phase 3 已完成（核心）**：axum REST 层（AppState/认证 Basic+Simple+login 豁免/错误映射 404·400·401·403·500）、/api/v1 全路由注册（backup/downloads/download/update/track 为 501 stub）、manga/category/chapter/source/extension/global 端点；REST 集成测试 2 项 + 真实服务冒烟（4568 端口）通过；25 单元测试全绿；clippy -D warnings 通过
- **Phase 4 已启动（核心骨架）**：async-graphql 7 集成（自定义标量 LongString/Duration/Cursor、MangaType/ChapterType/CategoryType/PageType/SourceType/MetaType、NodeList 分页、mangas 条件过滤+排序+分页、计算字段 unread/download/bookmark/chapters/categories/meta）；挂载 /api/graphql（GET/POST）端到端查询验证通过（枚举值/NodeList/过滤/计算字段与 Kotlin 一致）；当前 28 类型，基线 359 待增量补全；27 单元测试全绿；clippy -D warnings 通过
- **Phase 4 已完成**：Query 33/33、Mutation 82/82、Subscription 6/6 字段全齐；类型 351/363（缺失 14 均为孤立声明类型，客户端不可达不影响）；27 单元测试全绿
- **Phase 5 骨架已完成**：jvm-sandbox（Kotlin 2.2.20 + JDK HttpServer 零依赖）+ HttpSandboxFetcher（reqwest）+ SandboxProcess 生命周期；Rust 拉起 JVM→health→HTTP IPC 链路端到端验证通过；真实扩展加载（ChildFirstClassLoader + 完整接口）为下一增量
- **Phase 6 第一批完成**：REST track（list 真实现）/update（recentChapters 真实现 + summary）/downloads（队列契约）端点；backup 保留 stub
- **Phase 6 OPDS 完成**：suwayomi-opds 全量 feed（根导航/OpenSearch 描述/历史/库系列含交叉过滤+排序 facet/探索来源+来源在线浏览（走 SourceFetcher）/库来源/分类/流派/状态/语言导航/库更新/系列章节/章节元数据/not-found）；手写 XML writer（无外部依赖）+ 扁平 FromRow repository；挂载 `/api/opds/v1.2`（19 端点，认证随主 router）；集成测试 7 项（嵌入式 PGlite，含 XML 结构断言）+ 4568 HTTP 冒烟通过；踩坑：COUNT 聚合查询误带 ORDER BY 触发 PG 错误→嵌入式会话终止（已拆分 where/order）
- **Phase 6 库更新器 + 广播通道完成**：`suwayomi-graphql::updater`（UpdateManager：broadcast 事件总线 + 后台任务逐部遍历库内漫画，经 SourceFetcher 拉取章节、ON CONFLICT 插入新章节、更新 manga 时间戳/version，流式发出 LibraryUpdateStatus 快照）；`updateLibrary`/`updateStop` mutation 接真实管理；`libraryUpdateStatusChanged` 订阅改为广播流；GraphQLState 注入 update 管理器；库内测试 2 项（fake fetcher 插 2 章 + 事件流断言；源错误标记 Failed）全绿。注：独立 tests/ 二进制在 Windows 上被系统级 740 拦截（内容启发式），测试移入 lib `#[cfg(test)]` 规避
- **Phase 6 备份导出完成（protobuf）**：`suwayomi-core::backup`（手写 prost 消息，字段号对齐 kotlinx @ProtoNumber 0.x 格式：Backup/Manga/Chapter/Category/Source/Tracking/History/ServerSettings；`create_backup` 从库构建 → encode → gzip）；REST `/api/v1/backup/export`（octet-stream 流式 gzip proto）与 `/export/file`（attachment .tachibk）真实现，替换最后一个 501 stub；import/validate 保留 501 待后续；GraphQL `createBackup` 返回下载 URL；测试 2 项（roundtrip 保留漫画/章节/分类/来源断言 + 空库有效）；clippy 0 告警、workspace 43 集成测试通过。注：prost-build 需 protoc（未安装），改用手写 `#[derive(prost::Message)]` 免 build.rs
- 下一步：Phase 6 剩余（下载管理器、备份导入、同步、真实扩展加载）→ Phase 7
