# Suwayomi-Server → Suwayomi-next (Rust) 迁移计划

> 依据 `rust-migrate-guide-0.md` 制定，并逐文件核对 `Suwayomi-Server` 仓库（commit `4b2c19ab`，master 分支）源码后细化。
> 目标：在 `Suwayomi-next/` 下完成行为兼容的 Rust 实现，**接口、数据、协议与 Kotlin 原版保持一致**。

---

## 0. 现状盘点（基于源码审计）

### 0.1 源仓库规模

| 维度 | 数量 | 说明 |
| --- | --- | --- |
| Kotlin 源文件（server 模块） | ≈300 | 不含 i18n 资源 |
| 数据表 | 13 张 + 6 张 meta 表 | Exposed ORM 定义 |
| 数据库迁移 | 62 个（M0001–M0062） | de.neonew.exposed.migrations 框架 |
| REST v1 端点 | ≈70 个 | 9 个 controller + GlobalAPI |
| GraphQL | 15 Query / 18 Mutation / 4 Subscription | graphql-kotlin 反射生成 schema |
| OPDS 端点 | ≈10 个 | 供 KOReader 等电子书阅读器 |
| 测试用例 | 20 个 Kotlin 测试文件 | 可提炼为 Rust 测试 |

### 0.2 源仓库目录组织（迁移对照基准）

```
server/src/main/kotlin/
├── eu/kanade/tachiyomi/          # Tachiyomi 继承的扩展 API 抽象（Source/SManga/SChapter/Page/Filter/网络）
├── suwayomi/tachidesk/
│   ├── manga/
│   │   ├── MangaAPI.kt           # REST v1 路由注册
│   │   ├── controller/           # 8 个 REST controller
│   │   ├── impl/                 # 业务逻辑（Manga/Library/Category/Chapter/Page/Search/Source/Extension/Download/Update/Track/Backup）
│   │   ├── model/dataclass/      # DTO
│   │   └── model/table/          # Exposed 表定义
│   ├── graphql/                  # GraphQL 层（queries/mutations/types/subscriptions/dataLoaders/server）
│   ├── opds/                     # OPDS 协议
│   ├── global/                   # 全局设置/元数据/WebView/同步
│   ├── server/                   # 入口、HTTP 服务器、数据库、迁移、认证
│   ├── i18n/                     # 多语言（登录页）
│   └── util/                     # 通用工具
server/src/main/jte/              # JTE 模板（登录页）
server/src/main/resources/        # 图标、GraphQL Playground、tracker logo
AndroidCompat/                    # Android 框架兼容层（运行扩展）
scripts/bundler.sh                # 打包脚本
docs/                             # 用户文档
```

---

## 1. 目标仓库结构（Suwayomi-next）

采用 **Cargo workspace + 多 crate** 组织，与源仓库模块一一映射：

```
Suwayomi-next/
├── Cargo.toml                        # workspace 根
├── crates/
│   ├── suwayomi-core/                # ← eu/kanade/tachiyomi + manga/model/* + server/database/*
│   │   ├── src/models/               # 领域模型（Manga/Chapter/Page/Category/Source/Extension/TrackRecord/TrackSearch/Meta）
│   │   ├── src/schema/               # 数据表定义（对应 manga/model/table/*.kt）
│   │   ├── src/db/                   # 连接池、迁移执行器（对应 server/database/DBManager.kt + migration/）
│   │   └── src/source/               # 扩展 API 抽象（对应 eu/kanade/tachiyomi/source/*）
│   ├── suwayomi-domain/              # ← manga/impl/*（不含 controller）
│   │   ├── src/manga/                # Manga/Library/MangaList
│   │   ├── src/chapter/              # Chapter/Page
│   │   ├── src/category/             # Category/CategoryManga
│   │   ├── src/source/               # Source/Search
│   │   ├── src/download/             # DownloadManager/Downloader
│   │   ├── src/update/               # Updater/UpdateJob
│   │   ├── src/track/                # Tracker 服务
│   │   ├── src/backup/               # protobuf 备份
│   │   └── src/meta/                 # Meta 系统
│   ├── suwayomi-rest/                # ← manga/controller/* + MangaAPI.kt + GlobalAPI.kt
│   ├── suwayomi-graphql/             # ← graphql/*
│   ├── suwayomi-opds/                # ← opds/*
│   └── suwayomi-server/              # ← Main.kt + App.kt + server/JavalinSetup.kt（二进制入口）
├── jvm-sandbox/                      # ← AndroidCompat + 扩展加载逻辑（Kotlin，保持 JVM）
│   ├── sandbox-core/                 # ChildFirstClassLoader + APK→JAR 转换（dex2jar）+ 扩展加载
│   └── sandbox-server/               # HTTP 服务（/extensions /sources /source/.../manga 等）
├── tauri-app/                        # ← 桌面壳（R3 决策：移除 CEF，改用 Tauri）
│   ├── src/                          # Rust 侧：窗口管理、系统托盘、进程编排
│   └── tauri.conf.json               # Tauri 配置（托盘菜单、单实例、开机自启）
├── migrations/                       # SQL 迁移（复刻 M0001–M0062 的净效果）
├── tools/
│   └── h2-dump/                      # Kotlin 小工具：H2 → PostgreSQL 数据导出（R1 决策）
├── tests/                            # 集成测试（HTTP/GraphQL 兼容性测试）
├── docs/                             # 用户文档（迁移自源仓库 docs/）
└── MIGRATION_STATUS.md               # 逐文件迁移追踪表
```

### 1.1 crate 依赖图

```
suwayomi-core ←─────────────┐
      ▲                      │
      │                      │
suwayomi-domain ──────────► suwayomi-rest
      ▲  ▲                  ▲
      │  │                  │
      │  └──── suwayomi-graphql
      │        ▲
      │        │
      └─ suwayomi-opds
      │
      └──► jvm-sandbox（进程外，经 IPC 调用，仅 domain 通过 trait 依赖）
      ▲
suwayomi-server（入口，组合所有 crate）
```

---

## 2. 分阶段迁移计划（Phase 0–7）

### Phase 0：工作区骨架与兼容基线

**目标**：搭好工程骨架，产出三份"兼容基线"，作为后续所有阶段的验收对照物。

**任务**：
- [ ] 初始化 Cargo workspace（6 个 crate + 根 Cargo.toml），配置 rustfmt/clippy/lints
- [ ] 从源仓库导出 **GraphQL schema 基线**（运行 introspection，保存为 `docs/graphql/schema-baseline.graphql`）
- [ ] 从 62 个迁移 + 现有表定义推导 **最终 DDL**，生成 `migrations/*.sql`（见 §3 数据层策略）
- [ ] 盘点 REST v1 全部端点，生成 `docs/api/rest-endpoints-baseline.md`（方法/路径/参数/响应）
- [ ] 建立 `MIGRATION_STATUS.md`（每个 Kotlin 文件 ↔ Rust 实现 ↔ 状态）
- [ ] CI 骨架（GitHub Actions：cargo fmt/clippy/test）

**交付物**：
- `Cargo.toml`（workspace）+ 各 crate 空骨架（可编译）
- `docs/graphql/schema-baseline.graphql`、`docs/api/rest-endpoints-baseline.md`
- `migrations/` 初始 SQL、`MIGRATION_STATUS.md`

**验收标准**：
- `cargo build --workspace` 通过
- schema 基线可从 Kotlin 服务端重复生成且比对一致（工具链就绪）

---

### Phase 1：核心数据模型与数据库层

**目标**：Rust 侧拥有与 Kotlin 完全一致的领域模型与数据库访问层，可直接读写既有数据库。

**迁移范围**（源文件 → Rust 文件）：

| 源（Kotlin） | Rust 目标 |
| --- | --- |
| `manga/model/table/*.kt`（13 表） | `suwayomi-core/src/schema/*.rs` |
| `manga/model/dataclass/*.kt`（≈15 个） | `suwayomi-core/src/models/*.rs` |
| `server/database/DBManager.kt` | `suwayomi-core/src/db/manager.rs` |
| `server/database/H2Migration.kt`、`Migration.kt` | `suwayomi-core/src/db/migrator.rs` |
| `server/database/migration/helpers/*` | 迁移语义并入 migrator |
| `eu/kanade/tachiyomi/source/model/*`（SManga/SChapter/Page/Filter…） | `suwayomi-core/src/source/model.rs` |
| `manga/impl/util/lang/JsonObject.kt` | serde_json 直接使用 |
| `server/settings/*` + `server-config` | `suwayomi-core/src/config/*.rs`（配置模型） |

**要点**：
- 模型字段名、类型、默认值逐字段对齐（如 `MangaDataClass`：`id: Int`、`sourceId: String`、`thumbnailUrl: String?`、`updateStrategy: enum`、`memo: JsonObject`）
- `MangaStatus`/`UpdateStrategy` 等枚举值与 Kotlin 完全一致（`UNKNOWN=0, ONGOING=1, ...`）
- 全模型实现 `Serialize + Deserialize`，JSON 字段名与 Kotlin jackson 输出一致（`thumbnailUrl` 等驼峰字段保留驼峰——注意：**GraphQL 与 JSON DTO 用驼峰，数据库列名用 snake_case**，两套映射分开处理）

**数据库兼容策略**（详见 §3）：
- **仅支持 PostgreSQL**（决策变更 2026-08-30）：全部表在 `suwayomi` schema，连接级 `search_path` 与 Kotlin `defaultSchema` 一致；SQL 统一 `?` 占位符 + 运行时转 `$1..`
- 迁移执行器：兼容既有库的迁移状态表，可从旧版本库原地升级
- **H2 文件无法被 Rust 直接读取** → 数据迁移走 Phase 7 的 `tools/h2-dump`（Kotlin 导出）或备份导入（风险点 R1）

**交付物**：`suwayomi-core` 完整实现 + 单元测试（模型序列化 golden 测试、表结构 DDL 测试）

**验收标准**：
- `cargo test -p suwayomi-core` 通过
- 对既有 PostgreSQL 库（suwayomi schema）执行连接 + 读表成功（集成测试）
- SQLite 全新初始化后 `schema_version` 表 + 全部表结构与基线 DDL 一致

---

### Phase 2：核心业务逻辑（domain 服务）

**目标**：业务逻辑全部迁到 Rust，行为与原 `impl` 一致。

**迁移范围**（按依赖优先级）：

| 优先级 | 源（Kotlin） | Rust 目标 |
| --- | --- | --- |
| P0 | `manga/impl/Manga.kt` | `suwayomi-domain/src/manga/manga.rs` |
| P0 | `manga/impl/Chapter.kt` | `suwayomi-domain/src/chapter/chapter.rs` |
| P0 | `manga/impl/Page.kt` | `suwayomi-domain/src/chapter/page.rs` |
| P0 | `manga/impl/Library.kt`、`MangaList.kt` | `suwayomi-domain/src/manga/library.rs`、`manga_list.rs` |
| P1 | `manga/impl/Category.kt`、`CategoryManga.kt` | `suwayomi-domain/src/category/*` |
| P1 | `manga/impl/Search.kt`、`Source.kt` | `suwayomi-domain/src/source/*` |
| P1 | `manga/impl/extension/*`（Extension 安装/管理） | `suwayomi-domain/src/source/extension.rs` |
| P2 | `manga/impl/track/*`、`update/*`、`download/*`、`backup/*` | 对应模块（Phase 6 细化） |

**要点**：
- Meta 系统（`manga_meta`/`chapter_meta`/`category_meta`/`source_meta`/`global_meta` 表）是本项目核心机制，需完整迁移（`getMetaMap`/`setMeta` 语义一致）
- 缩略图代理（`proxyThumbnailUrl`）逻辑保持一致
- 业务逻辑通过 trait 抽象扩展调用（`SourceFetcher` trait），Phase 5 提供 JVM 沙盒实现，Phase 6 可选提供本地源实现

**交付物**：`suwayomi-domain` 实现 + 单元测试

**验收标准**：
- 移植 Kotlin 侧 `MangaTest`/`PageTest`/`CategoryMangaTest`/`SearchTest`/`PaginatedListTest` 的核心断言为 Rust 测试并全部通过
- 用同一份 SQLite 种子数据在 Kotlin 版与 Rust 版执行相同操作，结果一致

---

### Phase 3：REST API v1 迁移

**目标**：`/api/v1/**` 全部端点行为兼容。

**迁移范围**：`MangaAPI.kt` + 8 个 controller + `GlobalAPI.kt` + 认证中间件（`JavalinSetup.kt` 的 `beforeMatched` 逻辑）。

**端点清单（基线，共 ≈70 个）**：

| 分组 | 方法/路径 | Rust 模块 |
| --- | --- | --- |
| extension | `GET list`、`GET install/{pkgName}`、`POST install`、`GET update/{pkgName}`、`GET uninstall/{pkgName}`、`GET icon/{pkgName}` | `suwayomi-rest/src/extension.rs` |
| source | `GET list`、`GET {sourceId}`、`GET {sourceId}/popular/{pageNum}`、`GET latest/{pageNum}`、`GET/POST preferences`、`GET/POST filters`、`GET search`、`POST quick-search` | `suwayomi-rest/src/source.rs` |
| manga | `GET {mangaId}`、`GET {mangaId}/full`、`GET thumbnail`、`GET/PUT/DELETE category…`、`GET/DELETE library`、`PATCH meta`、`GET chapters`、`POST chapter/batch`、`GET/PATCH/PUT/DELETE chapter/{index}`、`PATCH chapter/{index}/meta`、`GET chapter/{index}/page/{pageIndex}` | `suwayomi-rest/src/manga.rs` |
| chapter | `POST batch`、`GET/HEAD {chapterId}/download` | `suwayomi-rest/src/chapter.rs` |
| category | `GET/POST ""`、`PATCH reorder`、`GET/PATCH/DELETE {categoryId}`、`PATCH {categoryId}/meta` | `suwayomi-rest/src/category.rs` |
| backup | `POST import`、`POST import/file`、`POST validate`、`POST validate/file`、`GET export`、`GET export/file` | `suwayomi-rest/src/backup.rs` |
| downloads | `WS ""`、`GET start/stop/clear` | `suwayomi-rest/src/download.rs` |
| download | `GET/DELETE {mangaId}/chapter/{index}`、`PATCH …/reorder/{to}`、`POST/DELETE batch` | 同上 |
| update | `GET recentChapters/{pageNum}`、`POST fetch`、`POST reset`、`GET summary`、`WS ""` | `suwayomi-rest/src/update.rs` |
| track | `GET list`、`POST login/logout/search/bind/update`、`GET {trackerId}/thumbnail` | `suwayomi-rest/src/track.rs` |
| global | `about`、`settings` 等（`GlobalAPI.kt`） | `suwayomi-rest/src/global.rs` |

**要点**：
- Web 框架：Axum；路由、参数名、JSON 字段名（snake_case 或与 Kotlin 输出一致）、HTTP 状态码逐条对齐
- 认证：`AuthMode.SIMPLE_LOGIN`（会话 Cookie + 登录页重定向）与 `BASIC_AUTH`，`/login.html` GET/POST
- WebSocket：`/api/v1/downloads`、`/api/v1/update` 的 ws 端点；GraphQL subscription 走 Apollo 协议
- 异常映射：`NullPointerException/NoSuchElementException → 404`、`IOException → 500`、`IllegalArgumentException → 400`、`Unauthorized → 401`、`Forbidden → 403`

**交付物**：`suwayomi-rest` 实现 + 端点级集成测试

**验收标准**：
- `docs/api/rest-endpoints-baseline.md` 中每个端点有对应集成测试且通过
- 同一请求在 Kotlin 版与 Rust 版返回相同状态码与 JSON 结构（黄金响应比对）

---

### Phase 4：GraphQL API 迁移

**目标**：`/api/graphql` schema 与行为 100% 兼容。

**迁移范围**：`graphql/queries/*`（15）、`graphql/mutations/*`（18）、`graphql/subscriptions/*`（4）、`graphql/types/*`（≈12）、`graphql/server/primitives/*`、`graphql/dataLoaders/*`、`graphql/directives/*`。

**要点**：
- 用 async-graphql 手写 schema，**以 Phase 0 导出的 `schema-baseline.graphql` 为准**，用 `graphql-inspector diff` 逐字段比对
- 自定义标量（`graphql/server/primitives/`）：
  - `LongAsString`：Long 序列化为 String（JS 精度）
  - `DurationAsString`：ISO-8601 时长
  - `Cursor`、`Upload`（multipart 上传）
- DataLoader 等价物：async-graphql 的 `DataLoader`（或自研 batch loader），覆盖 `MangaDataLoader`/`ChapterDataLoader`/`SourceDataLoader`/`CategoryDataLoader` 等 12+ 个 loader 语义
- 分页：`NodeList`/`Edge`/`PageInfo`/`Cursor` 结构（`getEdges` 只返回首尾两边的特殊实现）保持一致
- 订阅：`DownloadSubscription`/`InfoSubscription`/`SyncSubscription`/`UpdateSubscription`，Apollo 订阅协议（`graphql-transport-ws` 或兼容子集）
- `RequireAuth` directive

**交付物**：`suwayomi-graphql` 实现 + schema 比对脚本 + 查询测试集

**验收标准**：
- `graphql-inspector diff schema-baseline.graphql suwayomi-schema.graphql` 输出 **无 breaking change**
- 关键查询（库列表、章节列表、分页、meta）在双端返回一致

---

### Phase 5：扩展桥接层（JVM 沙盒）

**目标**：Rust 服务能运行 Mihon 扩展（关键难点）。
**决策（R2，已确认）**：**保留 Mihon 扩展的 JVM 执行 + APK 转换流程**——即完整保留源仓库的 `AndroidCompat` + dex2jar（APK→JAR）+ `ChildFirstURLClassLoader` 链路，不做 Wasm/原生替代探索。

**架构**（独立进程方案）：

```
Rust 主进程 ── HTTP/JSON IPC ──► JVM 沙盒进程（Kotlin，保留）
  suwayomi-domain                 sandbox-server（/api 端点）
  SourceFetcher trait 实现           ├── AndroidCompat（复用源码仓库 AndroidCompat/）
                                   ├── dex2jar：APK → JAR 转换（复用 PackageTools/BytecodeEditor）
                                   ├── ChildFirstURLClassLoader（已存在于源码）
                                   └── 扩展 JAR
```

**任务**：
- [ ] 从 `Suwayomi-Server` 源码提取扩展加载/执行逻辑到 `jvm-sandbox/`（独立 Gradle 模块），暴露 HTTP 端点：`GET /extensions`、`GET /sources`、`GET /source/{id}/manga?query=&page=`、`GET /source/{id}/manga/{mangaId}`、`GET /source/{id}/manga/{mangaId}/chapters`、`GET /source/{id}/chapter/{cid}/pages`、`GET /source/{id}/filters`、`POST /source/{id}/filters`
- [ ] **APK→JAR 转换链路**（R2）：移植 `PackageTools.kt`（APK 处理）、`AndroidManifestParser.kt`、`BytecodeEditor.kt`、dex2jar 调用逻辑，安装扩展时完成转换
- [ ] Rust 侧 `suwayomi-domain` 实现 `SourceFetcher` trait 的 HTTP 客户端
- [ ] 沙盒进程生命周期管理（启动、健康检查、优雅关闭、崩溃重启）
- [ ] 本地源（`source/local/*`：ZIP/CBZ/EPUB/RAR 解析）在 Rust 原生实现，不经过沙盒

**交付物**：`jvm-sandbox/` 模块 + `suwayomi-domain` 的 IPC 客户端 + 端到端测试

**验收标准**：
- 端到端：Rust 服务 → 沙盒 → 真实扩展 → 返回漫画/章节/页面数据
- 沙盒进程崩溃后自动重启，数据不丢

**人工决策点**：见 §7 R2。

---

### Phase 6：外围功能

| 模块 | 源（Kotlin） | Rust | 验收 |
| --- | --- | --- | --- |
| 下载管理 | `impl/download/*`（Downloader、DownloadManager、fileProvider） | `suwayomi-domain/src/download/` | 队列/状态/进度 WS 一致 |
| 更新 | `impl/update/*`（Updater、UpdateJob、UpdaterSocket） | `suwayomi-domain/src/update/` | recentChapters/fetch/summary 一致 |
| 备份/恢复 | `impl/backup/proto/*`（Mihon protobuf） | `suwayomi-core/src/backup/proto.rs`（**prost** 复刻 schema） | 双端互导互读 `.proto` 备份文件 |
| Tracker | `impl/track/tracker/*`（MAL/AniList/Kitsu/Bangumi/MangaUpdates/Shikimori） | `suwayomi-domain/src/track/` | OAuth + API 集成测试（mock） |
| OPDS | `opds/*`（V1 feeds、XML 生成） | `suwayomi-opds/` | 与 Kotlin 版输出 XML 比对 |
| 同步 | `global/impl/sync/*`、`manga/impl/sync/KoreaderSyncService`、`server/database/trigger/SyncYomiTriggers` | 对应模块 | 触发器语义等价（SQLite 用触发器，PostgreSQL 同） |
| **桌面壳（R3）** | `server/util/CEFManager.kt`、`server/util/SystemTray.kt`、`server/util/Browser.kt` | `tauri-app/` | 见下方「Tauri 桌面壳」 |

**Tauri 桌面壳（R3 决策，已确认：移除 CEF，改用 Tauri）**：
- 服务端（suwayomi-server）以无头模式运行，Tauri 壳负责：窗口（WebUI 前端）、系统托盘、进程编排
- **系统托盘菜单补全**（对齐并超越源仓库的 `SystemTray.kt` 两项）：
  - 打开 WebUI（对应源仓库 "Open Suwayomi"）
  - 打开数据目录（`openInExplorer`）
  - 启动时最小化到托盘（`startMinimizedToTray` 配置项）
  - 退出（对应 "Quit"，优雅停服务端）
- 单实例锁（复用 `server/util/AppMutex.kt` 语义）、开机自启（可选）
- 登录页（`Login.jte`）与认证流程在 Tauri 窗口内保持可用；桌面模式可配置自动登录（放行 localhost）

**交付物**：各模块实现 + 集成测试

**验收标准**：每个子模块的兼容性测试通过（见上表）

---

### Phase 7：数据迁移与发布

**目标**：用户可从 Kotlin 版平滑迁移，Rust 版可发布。

**任务**：
- [ ] `tools/h2-dump`：Kotlin 小工具，读取既有 H2 文件数据库 → **导出为 PostgreSQL 导入脚本（R1 决策）**（保留原库只读，不破坏）
- [ ] 备份导入路径：Rust 版支持导入 Kotlin 版导出的 Mihon `.proto` 备份（R1 补充路径）
- [ ] `--migrate` CLI：指定 Kotlin 数据目录 → 自动迁移（经 h2-dump）并启动
- [ ] **Tauri 打包**：`tauri-app` 构建 Windows/macOS/Linux 安装包；无头服务模式 Docker 镜像（参考 `scripts/bundler.sh`）
- [ ] 用户文档（迁移自 `docs/*.md` 并补充 Rust 版说明）

**交付物**：迁移工具 + Tauri 桌面安装包 + Docker 镜像 + 文档

**验收标准**：
- 用一份真实 Kotlin 版数据目录完成迁移，数据（库/章节/阅读进度/meta/分类）无丢失
- Tauri 应用三平台可打包；托盘四项菜单可用；Docker 镜像可运行

---

## 3. 数据模型与数据库访问层迁移策略（细节）

### 3.1 表清单与类型映射

| 表（Exposed） | 列数 | SQLite | PostgreSQL | 说明 |
| --- | --- | --- | --- | --- |
| `extension` | 12 | INTEGER PK AUTOINCREMENT | INTEGER PK | `icon_url` 有默认值 |
| `source` | 5 | **INTEGER PK（long id）** | BIGINT PK | 自增策略与 Kotlin 一致 |
| `manga` | 19 | INTEGER PK | INTEGER PK | `memo` → JSON TEXT/jsonb |
| `chapter` | 19 | INTEGER PK | INTEGER PK | FK manga CASCADE |
| `page` | 4 | INTEGER PK | INTEGER PK | FK chapter CASCADE；`index` 列 |
| `category` | 4 | INTEGER PK | INTEGER PK | `order` 列 |
| `category_manga` | 3 | INTEGER PK | INTEGER PK | 唯一约束 (category_id, manga_id) |
| `track_record` | 9 | INTEGER PK | INTEGER PK | |
| `track_search` | 5 | INTEGER PK | INTEGER PK | |
| `extension_store` | 9 | INTEGER PK | INTEGER PK | M0058 新增 |
| `global_meta`/`category_meta`/`chapter_meta`/`manga_meta`/`source_meta` | 3 | INTEGER PK | INTEGER PK | `key`/`value` TEXT |
| 迁移状态表（de.neonew 框架） | — | `migration` 兼容表 | 同 | **实现时核对原表名/结构** |

类型映射规则：
- `IntIdTable.id` → `INTEGER PRIMARY KEY AUTOINCREMENT`（SQLite）/ `INTEGER GENERATED BY DEFAULT AS IDENTITY`（PG）
- `bool` → `INTEGER 0/1`（SQLite）/ `BOOLEAN`（PG）
- `float`（chapter_number）→ `REAL`/`FLOAT4`（保持 -1 默认值）
- `jsonObject("memo")` → `TEXT`（JSON 字符串，SQLite）/ `jsonb`（PG）
- `truncatingVarchar(n)` / `unlimitedVarchar` → `TEXT`（SQLite）/ `VARCHAR(n)`/`TEXT`（PG）
- 所有时间戳（`in_library_at`、`last_read_at`…）为 **epoch 秒 Long**

### 3.2 迁移执行策略

- **不复刻 Kotlin 逐条迁移代码**，而是：a) 分析 62 个迁移的净效果 + 当前 `Table` 定义 → 生成基线 DDL；b) 编写 62 个等价的增量 SQL（每条对应 M0001–M0062 的语义），保证**旧库原地升级**与 Kotlin 版行为一致
- 迁移状态表结构与 de.neonew 框架对齐，**可识别 Kotlin 版已执行的迁移，不重复执行**
- `M0054_MovePostgresToSuwayomiSchema`：PostgreSQL 默认 `suwayomi` schema，SQLite 无 schema 概念（空迁移）

### 3.3 H2 兼容（关键风险）

H2 使用 JVM 专有 MVStore 文件格式，**Rust 侧无法直接读取**。兼容路径（三选一/组合）：
1. `tools/h2-dump`（Kotlin 导出工具）→ SQLite/PG 文件（推荐，最平滑）
2. 备份导入（Mihon `.proto`）→ 丢失阅读进度之外的部分次要数据（章节已下载状态等保留在备份内，可接受）
3. 仅支持 PostgreSQL 原地切换（用户原用 PG 的场景）

→ 详见风险 R1。

---

## 4. API 层迁移要求（总则）

1. **路径不变**：REST 保持 `/api/v1/...` 前缀（`ServerSubpath` 支持子路径部署，`/api/` 前缀可配置）；GraphQL 保持 `/api/graphql`（含 `/api/graphql/files/backup/{file}` 文件下载）；OPDS 保持 `/api/opds/...`
2. **请求结构不变**：query/form/multipart/JSON body 字段名与 Kotlin 版一致
3. **响应结构不变**：JSON 字段名、嵌套结构、枚举字符串值、时间戳格式（epoch 秒）、`Long` 的字符串编码（GraphQL LongAsString）一致
4. **错误语义不变**：状态码映射（404/400/401/403/500）+ 错误信息格式一致
5. **认证不变**：`SIMPLE_LOGIN`（Cookie session + `/login.html` 重定向）与 `BASIC_AUTH`
6. **CORS 不变**：`allowCredentials=true` + `reflectClientOrigin=true`
7. **订阅不变**：GraphQL subscription 使用与 Kotlin 版相同的 Apollo 协议（`graphql-transport-ws`），消息格式一致

---

## 5. 可复用资产清单

| 资产 | 位置（源仓库） | 复用方式 |
| --- | --- | --- |
| **前端 WebUI** | 独立仓库 `Suwayomi/suwayomi-webui`（经 `WebInterfaceManager` 下载托管） | Rust 版保留 WebUI 下载/更新/静态托管逻辑，前端代码零改动 |
| **AndroidCompat** | `AndroidCompat/`（含 `getAndroid.sh`/`getAndroid.ps1` 下载脚本） | 完整复用，编入 `jvm-sandbox` |
| **扩展加载器** | `manga/impl/util/ChildFirstURLClassLoader.kt`、`PackageTools.kt`、`AndroidManifestParser.kt`、`BytecodeEditor.kt` | 提取进 `jvm-sandbox`，原样保留 |
| **Mihon 备份格式** | `manga/impl/backup/proto/models/*.kt`（protobuf schema） | 以 prost 复刻 `.proto` schema，二进制兼容 |
| **测试用例** | `server/src/test/kotlin/**`（20 个文件） | 核心断言移植为 Rust 测试（MangaTest、PageTest、CategoryMangaTest、SearchTest、UpdateControllerTest、PaginatedListTest、M0056SyncYomiTest、RequestParserTest、PathTest/SafePathTest→Rust 路径安全测试） |
| **打包脚本** | `scripts/bundler.sh` | 移植为发布脚本 |
| **用户文档** | `docs/*.md`（6 篇） | 复制并补充 Rust 版说明 |
| **静态资源** | `server/src/main/resources/`（favicon、tracker logo、graphql-playground.html） | 直接复制到 Rust 版资源目录 |
| **i18n** | `server/i18n/`（登录页多语言） | 可选复用（低优先级） |
| **示例数据** | 无内置；`docs/The-Data-Directory.md` 描述目录结构 | 测试用种子数据自行构造，目录结构按文档复刻 |

---

## 6. 里程碑与总体验收

| 里程碑 | 阶段 | 验收 | 状态 |
| --- | --- | --- | --- |
| M0 | Phase 0 | workspace 可编译；三份基线产出 | ✅ 2026-08-30：workspace 编译通过；REST 端点基线、DDL 基线（PG+SQLite）、GraphQL schema 基线（从 Kotlin 版实测导出，359 类型）均产出 |
| M1 | Phase 1 | 模型/表/迁移测试通过；可读既有 PG 库 | ✅ 模型/schema/db/source 完成；PG 集成测试（Docker postgres:16）12 项全绿 |
| M2 | Phase 2 | domain 测试通过；与 Kotlin 行为比对一致 | ✅ 2026-08-30：domain 服务 + 10 集成测试（含 Kotlin 移植断言），27 测试全绿 |
| M3 | Phase 3 | REST 端点全量集成测试通过 | 🔄 下一步 |
| M4 | Phase 4 | GraphQL schema diff 无 breaking；查询测试通过 | 🔄 2026-08-30：核心查询 12/33 + Mutation 27/82，类型 172/363 |
| M5 | Phase 5 | 扩展端到端跑通 | ⬜ |
| M6 | Phase 6 | 外围功能测试通过 | ⬜ |
| M7 | Phase 7 | 真实数据迁移无丢失；发布产物可用 | ⬜ |

---

## 7. 人工决策点与技术风险

| 编号 | 风险/决策 | 等级 | 说明 | **决策结果（已确认）** |
| --- | --- | --- | --- | --- |
| **R1** | **H2 数据迁移路径** | 🔴 高 | H2 为 JVM 专有格式，Rust 无法直读，迁移必须经过工具/备份 | ✅ **提供导入工具，迁移到 PostgreSQL**：`tools/h2-dump`（Kotlin）导出 → PostgreSQL 导入脚本；Mihon 备份导入作为补充路径；SQLite 仍为全新部署默认后端 |
| **R2** | **扩展运行方案** | 🔴 高 | Mihon 扩展是 JVM 字节码，必须由 JVM 执行 | ✅ **保留 Mihon 扩展的 JVM 执行 + APK 转换**：jvm-sandbox 完整保留 AndroidCompat + dex2jar（APK→JAR）+ ChildFirstURLClassLoader 链路 |
| **R3** | **桌面端功能裁剪** | 🟡 中 | CEF WebView、系统托盘、浏览器自动打开、App 自更新（`global/impl/*`、`server/util/CEFManager.kt` 等） | ✅ **移除 CEF，改用 Tauri 桌面壳**：`tauri-app` 提供窗口 + 托盘；**补全系统托盘选项**（打开 WebUI / 打开数据目录 / 启动最小化到托盘 / 退出）；App 自更新走 Tauri updater 替代 |
| **R4** | **GraphQL schema 生成差异** | 🟡 中 | Kotlin 反射生成 vs Rust 手写 | 以 introspection 基线 + `graphql-inspector` 自动化比对，杜绝漂移 |
| **R5** | **迁移框架语义对齐** | 🟡 中 | de.neonew.exposed.migrations 的状态表结构/命名需逆向核对 | Phase 1 实现时以 Kotlin 版实际创建的库为准（可用 Docker 跑一次 Kotlin 版生成参考库） |
| **R6** | **SyncYomi 触发器** | 🟡 中 | `server/database/trigger/SyncYomiTriggers.kt` 用 DB 触发器实现同步 | SQLite/PG 均支持触发器，语义可复刻；需验证数据一致性 |
| **R7** | **本地源解析** | 🟡 中 | ZIP/CBZ/EPUB/RAR 解析（`source/local/*`） | Rust 侧用 zip/cbz 纯 Rust 实现；**RAR 需引入 unrar 类库（许可注意）**，EPUB 需 XML 解析；列为 Phase 5 子任务，风险可控 |
| **R8** | **工作量与交付节奏** | 🟡 中 | 全量迁移 ≈ 数周工程量 | 本计划按"可独立验收的阶段"推进，每个 Phase 完成即交付，支持中途调整优先级 |

---

## 8. 执行流程

1. ✅ **计划已确认**（2026-08-30）：R1→PostgreSQL 导入工具；R2→保留 JVM 执行 + APK 转换；R3→Tauri 桌面壳 + 补全托盘
2. 从 **Phase 0** 开始逐阶段执行；每阶段执行前先核对源仓库对应文件；执行中同步更新 `MIGRATION_STATUS.md`
3. 每阶段结束运行：`cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace`
4. 剩余风险点（R4–R8）在执行中持续跟踪，遇到阻塞时暂停询问
