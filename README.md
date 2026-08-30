# Suwayomi-next

Suwayomi（Kotlin/JVM）的 Rust 重写版。目标：保持既有数据、GraphQL/REST/OPDS 接口、Mihon 扩展体系完全兼容。

## 状态

| Phase | 内容 | 状态 |
| --- | --- | --- |
| 0 | 工作区骨架与兼容基线 | 🟢 完成 |
| 1 | 核心数据模型与数据库层 | 🟢 完成 |
| 2 | 核心业务逻辑（domain） | 🟢 完成 |
| 3 | REST API v1 | 🟢 完成 |
| 4 | GraphQL API | 🟢 完成 |
| 5 | 扩展桥接层（JVM 沙盒） | 🟢 完成 |
| 6 | 外围功能（下载/更新/备份/Tracker/OPDS/同步/Tauri 壳） | 🟢 完成 |
| 7 | 数据迁移工具与发布 | 🟢 h2-dump/--migrate/备份导入/Docker/文档完成 |

## 快速开始

```bash
# 构建 + 启动（默认端口 8090，内置嵌入式 PGlite，零外部依赖；Windows 上 4501-4900 可能被 Hyper-V 保留，启动失败时自动顺延端口）
cargo run --release -p suwayomi-server
```

- WebUI：`http://localhost:8090`
- GraphQL：`/api/graphql` ｜ REST：`/api/v1` ｜ OPDS：`/api/opds/v1.2`（KOReader 可用）
- 完整配置与迁移说明见 **`docs/user-guide.md`**

## 仓库结构

```
crates/
  suwayomi-core/     领域模型 + 数据表 + 数据库层（← kotlin manga/model + server/database）
  suwayomi-domain/   业务逻辑（← kotlin manga/impl）
  suwayomi-rest/     REST API v1（← kotlin manga/controller + MangaAPI/GlobalAPI）
  suwayomi-graphql/  GraphQL API（← kotlin graphql）
  suwayomi-opds/     OPDS（← kotlin opds）
  suwayomi-server/   服务端入口（← kotlin Main.kt + server/）
jvm-sandbox/         扩展沙盒（Kotlin，AndroidCompat + dex2jar + ChildFirstClassLoader）
tools/h2-dump/       H2 → PostgreSQL 迁移工具（Kotlin，Phase 7）
migrations/          SQL 迁移（PostgreSQL）
docs/                基线文档（REST 端点 / GraphQL schema / 迁移说明 / 用户指南）
```

## 数据库后端

- **默认**：嵌入式 PGlite（PostgreSQL 17 引擎，WASM 打包，数据目录 `./pglite-data`）——零安装即用
- **备选**：外部 PostgreSQL（设 `SUWAYOMI_DATABASE_URL`，如 `postgres://user:pass@host:5432/db`）

## 从 Kotlin 版迁移

```bash
gradle -p tools/h2-dump build
suwayomi-server --migrate <kotlin-data-dir>   # H2 → 当前后端，完成后退出
suwayomi-server                                # 正常启动即可
```

备份导入/导出：`GET /api/v1/backup/export`、`POST /api/v1/backup/import`（详见用户指南）。

## 真实扩展（JVM sandbox）

server 可启动一个 JVM 沙盒进程，通过 HTTP 契约驱动真实 Mihon/Tachiyomi 扩展（APK → dex2jar → ChildFirst 类加载 + 反射，字节码修复 R8 产物）：

```bash
# 1) 构建 sandbox（JDK 17+，产物 build/libs/suwayomi-jvm-sandbox.jar）
cd jvm-sandbox
gradle build          # 需要 jvm-sandbox/libs/AndroidCompat-1.0.jar（从 Suwayomi-Server
                      #   AndroidCompat 模块构建后复制，或自行替换为等价 Android stub）
cd ..

# 2) 把扩展 APK 放入目录（默认 ./extensions，或用 SUWAYOMI_EXTENSIONS_DIR 指定）
# 3) 启动 server 并启用 sandbox（需要 Java 25+，AndroidCompat 以 JDK 21 编译）
SUWAYOMI_SANDBOX_JAR=jvm-sandbox/build/libs/suwayomi-jvm-sandbox.jar \
SUWAYOMI_SANDBOX_PORT=8091 \
SUWAYOMI_EXTENSIONS_DIR=E:/path/to/extensions \
SUWAYOMI_SANDBOX_PROXY=127.0.0.1:7890 \   # 可选：出境代理（访问被墙源）
./target/debug/suwayomi
```

环境变量：`SUWAYOMI_SANDBOX_JAR`（启用沙盒）、`SUWAYOMI_SANDBOX_PORT`（默认 8091，避开 Windows 动态端口保留区 4501–4900）、`SUWAYOMI_EXTENSIONS_DIR`（默认 ./extensions）、`SUWAYOMI_SANDBOX_PROXY`（可选 HTTP 代理）。未配置时回退内置 `StubFetcher`。

## 关键文档

- `docs/migration/MIGRATION_PLAN.md` — 分阶段迁移计划（含决策记录 R1–R8）
- `docs/migration/MIGRATION_STATUS.md` — 逐文件迁移追踪
- `docs/user-guide.md` — 用户指南（配置/迁移/备份/OPDS/Docker）
- `docs/api/rest-endpoints-baseline.md` — REST v1 端点兼容基线
- `docs/graphql/README.md` — GraphQL schema 基线说明

## 构建

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## 同步（Phase 6）

- **KOReader**：GraphQL `connectKoSyncAccount` / `pushKoSyncProgress` / `pullKoSyncProgress` / `koSyncStatus`。凭据存 `global_meta`；章节 `koreader_hash` 为 `md5("<manga title> - <chapter name>")`（FILENAME 校验和）。
- **SyncYomi**：GraphQL `startSync` / `lastSyncStatus`。配置见 ServerConfig：`syncYomiEnabled` / `syncYomiHost` / `syncYomiApiKey`（另有 6 项 `syncData*` 数据范围与 `syncInterval`）。同步以 Mihon Backup protobuf + ETag（If-None-Match/If-Match）在 `{host}/api/sync/content` 上 pull → restore → push。
- **version 触发器**：`migrations/pg-only/0002_*` 是 `SyncYomiTriggers.kt` 的 PostgreSQL 移植（manga/chapter/category 变更自动 bump version，`is_syncing` 豁免）。嵌入式 pglite 不支持 PL/pgSQL，仅外部 PostgreSQL 应用（`Db::migrate` 自动分流）。

## 扩展安装与源管理（Phase 6）

扩展从**仓库索引**在线安装，装完自动把源注册进数据库，前后端通用：

- **仓库**：`extension_store` 表存 `index_url`（支持 v1 数组与 keiyoushi v2 对象格式）。`POST /api/v1/extension/refresh`（或 GraphQL `fetchExtensions`）拉取索引并 upsert `extension` 表（apkUrl/版本/NSFW 等）。
- **安装/更新/卸载**：`GET /api/v1/extension/install/{pkgName}`、`/update/{pkgName}`、`/uninstall/{pkgName}`（GraphQL 对应 `updateExtension`/`updateExtensions` patch）。安装下载 APK 到 `SUWAYOMI_EXTENSIONS_DIR`（缺省 `./extensions`，命名 `tachiyomi-{lang}.{pkg}-v{ver}.apk`），触发 JVM sandbox 热加载（`/reload`），随后把 `/sources` 的稳定源 id（扩展 `Source.getId()`）upsert 进 `source` 表。
- **外部 APK**：GraphQL `installExternalExtension`（multipart 上传）走 sandbox `/inspect` 解析元数据后安装。
- **代理**：仓库/APK 下载复用 `SUWAYOMI_SANDBOX_PROXY` 出境代理。
- 实测（keiyoushi 仓库）：刷新 **1381** 个扩展；安装 nhentai → sandbox 热加载 **22 源** → DB 注册 → popular **18 部真实漫画**；卸载后 sandbox 0 源、DB 清空。

## 桌面壳与发布布局（Tauri，tauri-app/）

无头 `suwayomi-server` 服务端 + Tauri 2 桌面壳（独立工程，不进 workspace）。发布布局（zip 内）：

```
suwayomi.exe          桌面壳：托盘 + 设置窗口（自动拉起无头服务器）
bin/
  ├─ suwayomi-server.exe   无头服务器（自带 WebUI 静态托管；单实例）
  └─ jvm-sandbox.jar       扩展沙盒（JVM，server 自动查找）
webui/                Suwayomi-WebUI 构建产物（CI 自动捆绑 fork 最新 release）
data/                 工作数据目录（不存在时自动创建）
  ├─ autobackup/  downloads/  local/
pglite-data/          嵌入式数据库（server 自动创建于发布根目录）
logs/                 server.log / tray.log / sandbox.log（运行时日志）
```

- **server 静态托管**：`SUWAYOMI_WEBUI_DIR` env 或 exe 同级 `webui/`（index.html 存在即启用 SPA 托管，根路径/静态资源/前端路由全通）
- **扩展沙盒**：`SUWAYOMI_SANDBOX_JAR` env 或 exe 同级 `jvm-sandbox.jar` / `bin/jvm-sandbox.jar`（自动查找；安装扩展需 jar，需 JDK）
- **托盘菜单**：启动 Suwayomi / 打开 WebUI / 打开数据目录 / 设置 / 退出（退出结束 server 子进程）
- **设置窗口**：端口（保存即重启 server）、打开数据目录、WebUI 地址
- 构建：`cd tauri-app && cargo build --release`；`suwayomi -v` 显示版本与仓库

## Docker

```bash
docker build -t suwayomi-next .
docker run -p 8090:4567 -v suwayomi-data:/data suwayomi-next   # 容器内 4567，映射到宿主 8090
```

## License

MPL-2.0（与上游一致）
