# Suwayomi-next

[Suwayomi](https://github.com/Suwayomi/Suwayomi-Server/) （下称Kotlin/Java版）的 Rust 实现。保持既有 Tachiyomi 数据结构、GraphQL/REST/OPDS 接口、Mihon 扩展体系完全兼容，除扩展运行层外均由Rust实现。

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
| 7 | 数据迁移工具与发布 | 🟢 完成 |

**API 兼容性状态**：REST v1 端点基线见 `docs/api/rest-endpoints-baseline.md`，GraphQL
schema 基线见 `docs/graphql/README.md`，与 Suwayomi 原版保持行为兼容（详见
`docs/migration/MIGRATION_PLAN.md` 的决策记录 R1–R8）。

## 快速开始（源码构建）

```bash
# 构建 + 启动（默认端口 8090，内置嵌入式 Oliphaunt PostgreSQL 18，零外部依赖；启动失败时自动顺延端口）
cargo run --release -p suwayomi-server
```

- WebUI：`http://localhost:8090`
- GraphQL：`/api/graphql` ｜ REST：`/api/v1` ｜ OPDS：`/api/opds/v1.2`（KOReader 可用）
- 完整配置说明见 **`docs/user-guide.md`**

## Release 目录结构与用法

GitHub Release 提供 `Suwayomi-{version}-{platform}-{arch}.zip` 等平台压缩包，解压后即开即用：

> 若发布没有你想要的平台构建，可Fork该仓库后手动运行Release构建，构建参数支持一键配置

```
suwayomi.exe          桌面壳（Tauri 托盘）：启动/重启/WebUI/数据目录/设置/退出
bin/
  ├─ suwayomi-server.exe   无头服务器（核心 Suwayomi-server, 单实例）
  ├─ jvm-sandbox.jar       扩展沙盒（JVM-server）
  └─ extensions/*.jar      已安装扩展（dex2jar 输出，自动生成）
jre/                  捆绑 Temurin JRE 25（Windows x64）——sandbox 运行时，
                      server 优先使用 jre/bin/java.exe，无需系统 JDK；
                      未捆绑时回退 SUWAYOMI_JAVA → JAVA_HOME → PATH 的 java
webui/                Suwayomi-WebUI 构建产物（CI 自动捆绑 fork 最新 release）
                      └─ revision  当前部署的 WebUI 版本（r3487 等，关于页/更新检查读取）
extensions/           扩展下载目录：仅存放扩展安装包 APK（tachiyomi-*.apk）
cache/                统一磁盘缓存根（SUWAYOMI_CACHE_DIR 可覆盖，默认发布根下）
  ├─ extensions/icons/  扩展图标缓存（按内容类型存 .png/.jpg/.webp）
  ├─ extensions/index/  仓库索引本地缓存（{repo}/index.pb|json）
  └─ trackers/          追踪源图标缓存（MAL/Anilist/Bangumi logo 等）
data/                 默认数据目录（Tachiyomi 兼容形式）
  └─ autobackup/  downloads/  local/
pglite-data/          嵌入式数据库（server 自动创建于发布根目录）
logs/                 运行时日志
```

带 Electron 桌面壳的产物为 `Suwayomi-{version}-{platform}-{arch}_wElectron.zip`——在标准版
基础上多一个 `electron/` 目录（electron v44.1.0 win32-x64 运行时 + 应用入口）。托盘
设置「有 Electron 时优先使用」（默认开）开启后，打开 WebUI 将启动 Electron 窗口
（`electron/electron.exe --url=http://127.0.0.1:{port}`）而非系统浏览器；托盘退出时
会一并关闭 Electron 进程。

### 使用方法

1. **启动**：双击 `suwayomi.exe`（静默托盘，无终端窗口），托盘菜单：
   - 启动 / 重启 Suwayomi — 未运行时拉起；运行中显示「重启」（优雅关闭后重新拉起，含嵌入式数据库）
   - 打开 WebUI — 浏览器打开 `http://127.0.0.1:{port}`
   - 打开数据目录 / 设置（端口、数据目录、WebUI 地址，保存即重启 server）
   - 退出 — 结束托盘与 server 子进程（嵌入式 postgres 一并关停）
2. **命令行方式**：直接运行 `bin/suwayomi-server.exe`（`-v` 显示版本与仓库地址）。
3. **添加扩展仓库**：WebUI 扩展页添加仓库索引 URL（支持 Mihon `index.pb` 与
   Tachiyomi `index.json`，如 keiyoushi），刷新后在线安装扩展。
4. **扩展安装**：APK 下载到 `extensions/`，由 JVM 沙盒 dex2jar 转换并加载，
   转换 jar 落在 `bin/extensions/`，源自动注册进数据库。卸载时两者一并清理。
5. **端口**：默认 8090；启动时若被占用自动顺延。与桌面壳同用时以托盘设置为准。
6. **日志**：`logs/` 下 server/tray/sandbox 三个日志文件，排查问题优先看这里。

> 版本命名：自动构建产物版本为 `r{versionCode}`（versionCode = commit 数 + 3000）；
> 手动 beta/release 版本为 `3.y.z`（versionCode 的前三位拆分）。

## 从 Java 版迁移

迁移操作（H2 → 当前后端）见 **`docs/migration/MIGRATE.md`**：推荐 `suwayomi-server
--migrate <kotlin-data-dir>`（h2-dump 全量导入），或导入 Mihon `.proto` 备份。
接口/数据/协议兼容性状态见上文状态表与 `docs/api` 基线。

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
tauri-app/           桌面壳（Tauri 2，独立 workspace，不进主 workspace）
tools/h2-dump/       H2 → PostgreSQL 迁移工具（Kotlin，Phase 7）
migrations/          SQL 迁移（PostgreSQL）
docs/                基线文档（REST 端点 / GraphQL schema / 迁移说明 / 用户指南）
```

## 数据库后端

- **默认**：嵌入式 Oliphaunt（PostgreSQL 18 native，数据目录 `./pglite-data`）——零安装即用，支持多连接池
- **备选**：外部 PostgreSQL（设 `SUWAYOMI_DATABASE_URL`，如 `postgres://user:pass@host:5432/db`）

## 真实扩展（JVM sandbox）

server 可启动一个 JVM 沙盒进程，通过 HTTP 契约驱动真实 Mihon/Tachiyomi 扩展（APK → dex2jar → ChildFirst 类加载 + 反射，字节码修复 R8 产物）：

```bash
# 1) 构建 sandbox（JDK 21+，产物 build/libs/suwayomi-jvm-sandbox.jar）
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

环境变量：`SUWAYOMI_SANDBOX_JAR`（启用沙盒）、`SUWAYOMI_SANDBOX_PORT`（默认 8091）、`SUWAYOMI_EXTENSIONS_DIR`（默认 ./extensions）、`SUWAYOMI_JAR_DIR`（转换 jar 目录，默认 `<extensions>/../bin/extensions`）、`SUWAYOMI_SANDBOX_PROXY`（可选 HTTP 代理）。未配置时回退内置 `StubFetcher`。

## 扩展安装与源管理（Phase 6）

扩展从**仓库索引**在线安装，装完自动把源注册进数据库，前后端通用：

- **仓库**：`extension_store` 表存 `index_url`（支持 v1 数组与 keiyoushi v2 对象格式）。`POST /api/v1/extension/refresh`（或 GraphQL `fetchExtensions`）拉取索引并 upsert `extension` 表（apkUrl/版本/NSFW 等）。索引下载后写本地缓存 `extensions/index/{repo}/index.pb|json`，仓库不可达时自动回退缓存。
- **安装/更新/卸载**：`GET /api/v1/extension/install/{pkgName}`、`/update/{pkgName}`、`/uninstall/{pkgName}`（GraphQL 对应 `updateExtension`/`updateExtensions` patch）。安装下载 APK 到 `SUWAYOMI_EXTENSIONS_DIR`（缺省 `./extensions`，命名 `tachiyomi-{lang}.{pkg}-v{ver}.apk`），触发 JVM sandbox 热加载（`/reload`），随后把 `/sources` 的稳定源 id（扩展 `Source.getId()`）upsert 进 `source` 表。
- **外部 APK**：GraphQL `installExternalExtension`（multipart 上传）走 sandbox `/inspect` 解析元数据后安装。
- **代理**：仓库/APK 下载复用 `SUWAYOMI_SANDBOX_PROXY` 代理设置。
- 实测（keiyoushi 仓库）：刷新 **1381** 个扩展；安装 nhentai → sandbox 热加载 **22 源** → DB 注册 → popular **18 部真实漫画**；卸载后 sandbox 0 源、DB 清空。

## 同步（Phase 6）

- **KOReader**：GraphQL `connectKoSyncAccount` / `pushKoSyncProgress` / `pullKoSyncProgress` / `koSyncStatus`。凭据存 `global_meta`；章节 `koreader_hash` 为 `md5("<manga title> - <chapter name>")`（FILENAME 校验和）。
- **SyncYomi**：GraphQL `startSync` / `lastSyncStatus`。配置见 ServerConfig：`syncYomiEnabled` / `syncYomiHost` / `syncYomiApiKey`（另有 6 项 `syncData*` 数据范围与 `syncInterval`）。同步以 Mihon Backup protobuf + ETag（If-None-Match/If-Match）在 `{host}/api/sync/content` 上 pull → restore → push。
- **version 触发器**：`migrations/pg-only/0002_*` 是 `SyncYomiTriggers.kt` 的 PostgreSQL 移植（manga/chapter/category 变更自动 bump version，`is_syncing` 豁免）。嵌入式（oliphaunt 真实 PG）与外部 PostgreSQL 均自动应用（`Db::migrate` 统一执行）。

## 构建

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Windows 手动构建 release 产物（`suwayomi-server.exe` + 托盘 `suwayomi.exe`）：
双击仓库根的 **`build.bat`**（或 `cmd /c build.bat`）。

## 关键文档

- `docs/migration/MIGRATE.md` — 从 Kotlin 版迁移操作指南（h2-dump / 备份导入）
- `docs/migration/MIGRATION_PLAN.md` — 分阶段迁移计划（含决策记录 R1–R8）
- `docs/user-guide.md` — 用户指南（配置/迁移/备份/OPDS/Docker）
- `docs/api/rest-endpoints-baseline.md` — REST v1 端点兼容基线
- `docs/graphql/README.md` — GraphQL schema 基线说明

## Docker

```bash
docker build -t suwayomi-next .
docker run -p 8090:8090 -v suwayomi-data:/data suwayomi-next   # 容器内与宿主均 8090
```

## License

MPL-2.0（与上游一致）
