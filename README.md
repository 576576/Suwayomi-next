# Suwayomi-next

简体中文 | [English](docs/en/README.md)

![128x128](./assets/images/128x128.png)

[Suwayomi-Server](https://github.com/Suwayomi/Suwayomi-Server/) 的 Rust 实现，并完善了桌面托盘。保持既有 Tachiyomi 数据结构、GraphQL/REST/OPDS 接口、Mihon 扩展体系完全兼容，除扩展运行层外均由Rust实现。本项目是API参考的实现，并非原 [Suwayomi-Server](https://github.com/Suwayomi/Suwayomi-Server) 的分支。

![page-setting](./assets/screenshots/page-setting.png)

## 实现范围

已实现：核心数据模型与数据库层、业务逻辑（domain）、REST API v1、GraphQL API、OPDS、
下载/更新/备份/Tracker/KOReader·SyncYomi 同步、JVM 扩展沙盒、Tauri 桌面壳与发布 CI。
REST v1 与 GraphQL schema 基线见 `docs/api/`、`docs/graphql/`，行为与原版 Suwayomi 兼容。

## 快速开始（源码构建）

```bash
# 构建 + 启动（默认端口 8090，内置嵌入式 Oliphaunt PostgreSQL 18，零外部依赖；启动失败时自动顺延端口）
cargo run --release -p suwayomi-server
```

- WebUI：`http://localhost:8090`
- GraphQL：`/api/graphql` ｜ REST：`/api/v1` ｜ OPDS：`/api/opds/v1.2`（KOReader 可用）
- 完整配置说明见 **`docs/user-guide.md`**

## Release 包结构与用法

GitHub Release 提供解压即用的平台包（具体平台与捆绑范围随手动 Release 勾选的目标而定）。
需要其他平台 / 捆绑组合的产物时：

1. Fork 本仓库
2. 在 Actions 页手动运行 `Release` workflow
3. 勾选目标平台、通道（构建参数支持一键配置）
4. 运行完成后从 Release 页下载对应产物

```
suwayomi             桌面壳（Tauri 托盘）
bin/
  ├─ suwayomi-server   无头服务器（单实例）
  ├─ jvm-sandbox.jar   扩展沙盒
  └─ extensions/       已装扩展的转换 jar（自动生成）
data/                默认数据目录（Tachiyomi 兼容）
webui/               Suwayomi-WebUI 构建产物（随发布捆绑）
jre/   oliphaunt-runtime/   运行时依赖（可选）
```

> 捆绑的 WebUI 来自 [576576/Suwayomi-WebUI](https://github.com/576576/Suwayomi-WebUI)

WebUI 桌面窗口由托盘的**系统 WebView** 打开（Windows WebView2 / Linux WebKitGTK /
macOS WKWebView），不捆绑浏览器运行时；无 WebView 时回退系统浏览器。

### 使用方法

1. **启动**：双击 `suwayomi`（静默托盘，无终端窗口），托盘菜单：
   - 启动 / 重启 Suwayomi — 未运行时拉起；运行中显示「重启」（优雅关闭后重新拉起，含嵌入式数据库）
   - 打开 WebUI — 系统 WebView 窗口打开 `http://127.0.0.1:{port}`（无 WebView 时回退浏览器）
   - 打开数据目录 / 设置（端口、数据目录、WebUI 地址，保存即重启 server）
   - 退出 — 结束托盘与 server 子进程（嵌入式 postgres 一并关停）
2. **命令行方式**：直接运行 `bin/suwayomi-server`（`-v` 显示版本与仓库地址）。
3. **添加扩展仓库**：WebUI 扩展页添加仓库索引 URL（支持 Mihon `index.pb` 与
   Tachiyomi `index.json`，如 keiyoushi），刷新后在线安装扩展。
4. **扩展安装**：APK 下载到 `extensions/`，由 JVM 沙盒 dex2jar 转换并加载，
   转换 jar 落在 `bin/extensions/`，源自动注册进数据库。卸载时两者一并清理。
5. **端口**：默认 8090；启动时若被占用自动顺延。与桌面壳同用时以托盘设置为准。
6. **日志**：`cache/logs/` 下 server/tray/sandbox 三个日志文件，排查问题优先看这里。

## 仓库结构

```
crates/
  suwayomi-core/     领域模型 + 数据表 + 数据库层
  suwayomi-domain/   业务逻辑
  suwayomi-rest/     REST API v1
  suwayomi-graphql/  GraphQL API
  suwayomi-opds/     OPDS
  suwayomi-server/   服务端入口
jvm-sandbox/         扩展沙盒（Kotlin：AndroidCompat + dex2jar + ChildFirstClassLoader）
suwayomi-tray/       桌面壳（Tauri 2，独立 workspace，不进主 workspace；Windows/Linux）
tools/h2-dump/       H2 → PostgreSQL 迁移工具（Kotlin）
migrations/          SQL 迁移（含 pg-only/：SyncYomi 触发器）
scripts/             CI/辅助脚本（resolve-webui.sh / unzip_any.py 等）
assets/              图标与截图（images/、screenshots/）
docs/                文档（api/、graphql/、migration/、en/、release.md、user-guide.md）
```

## 数据库后端

- **默认**：嵌入式 Oliphaunt（PostgreSQL 18 native，数据目录 `./pglite-data`）
- **备选**：外部 PostgreSQL（设 `SUWAYOMI_DATABASE_URL`，如 `postgres://user:pass@host:5432/db`）

## 真实扩展（JVM sandbox）

server 可启动一个 JVM 沙盒进程，通过 HTTP 契约驱动真实 Mihon/Tachiyomi 扩展（APK → dex2jar → ChildFirst 类加载 + 反射）：

```bash
# 1) 构建 sandbox（JDK 25 toolchain；产物 build/libs/suwayomi-jvm-sandbox.jar）
cd jvm-sandbox
gradle build          # 需要 jvm-sandbox/libs/AndroidCompat-1.0.jar（从 Suwayomi-Server
                      #   AndroidCompat 模块构建后复制，或自行替换为等价 Android stub）
cd ..

# 2) 把扩展 APK 放入目录（默认 ./extensions，或用 SUWAYOMI_EXTENSIONS_DIR 指定）
# 3) 启动 server 并启用 sandbox
SUWAYOMI_SANDBOX_JAR=jvm-sandbox/build/libs/suwayomi-jvm-sandbox.jar \
SUWAYOMI_SANDBOX_PORT=8091 \
SUWAYOMI_EXTENSIONS_DIR=/path/to/extensions \
SUWAYOMI_SANDBOX_PROXY=127.0.0.1:7890 \   # 可选：HTTP 代理
./target/release/suwayomi-server
```

环境变量：`SUWAYOMI_SANDBOX_JAR`（启用沙盒）、`SUWAYOMI_SANDBOX_PORT`（默认 8091）、`SUWAYOMI_EXTENSIONS_DIR`（默认 ./extensions）、`SUWAYOMI_JAR_DIR`（转换 jar 目录，默认 `<extensions>/../bin/extensions`）、`SUWAYOMI_SANDBOX_PROXY`（可选 HTTP 代理）。未配置时回退内置 `StubFetcher`。

## 扩展安装与源管理

扩展从**仓库索引**在线安装，装完自动把源注册进数据库，前后端通用：

- **仓库**：`extension_store` 表存 `index_url`（支持 v1 数组与 keiyoushi v2 对象格式）。`POST /api/v1/extension/refresh`（或 GraphQL `fetchExtensions`）拉取索引并 upsert `extension` 表（apkUrl/版本/NSFW 等）。索引下载后写本地缓存 `extensions/index/{repo}/index.pb|json`，仓库不可达时自动回退缓存。
- **安装/更新/卸载**：`GET /api/v1/extension/install/{pkgName}`、`/update/{pkgName}`、`/uninstall/{pkgName}`（GraphQL 对应 `updateExtension`/`updateExtensions` patch）。安装下载 APK 到 `SUWAYOMI_EXTENSIONS_DIR`（缺省 `./extensions`，命名 `tachiyomi-{lang}.{pkg}-v{ver}.apk`），触发 JVM sandbox 热加载（`/reload`），随后把 `/sources` 的稳定源 id（扩展 `Source.getId()`）upsert 进 `source` 表。
- **外部 APK**：GraphQL `installExternalExtension`（multipart 上传）走 sandbox `/inspect` 解析元数据后安装。
- **代理**：仓库/APK 下载复用 `SUWAYOMI_SANDBOX_PROXY` 代理设置。

## 同步

- **KOReader**：GraphQL `connectKoSyncAccount` / `pushKoSyncProgress` / `pullKoSyncProgress` / `koSyncStatus`。凭据存 `global_meta`；章节 `koreader_hash` 为 `md5("<manga title> - <chapter name>")`（FILENAME 校验和）。
- **SyncYomi**：GraphQL `startSync` / `lastSyncStatus`。配置见 ServerConfig：`syncYomiEnabled` / `syncYomiHost` / `syncYomiApiKey`（另有 6 项 `syncData*` 数据范围与 `syncInterval`）。同步以 Mihon Backup protobuf + ETag（If-None-Match/If-Match）在 `{host}/api/sync/content` 上 pull → restore → push。
- **version 触发器**：`migrations/pg-only/0002_*` 在 manga/chapter/category 变更时自动 bump 版本（`is_syncing` 豁免），嵌入式与外部 PostgreSQL 均自动应用。

## 构建

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Windows 手动构建 release 产物（`suwayomi-server.exe` + 托盘 `suwayomi.exe`）：
双击仓库根的 **`build.bat`**（或 `cmd /c build.bat`）。

## 关键文档

- `docs/user-guide.md` — 用户指南（配置/备份/OPDS/Docker）
- `docs/release.md` — 发布流程与 CI 约定
- `docs/migration/MIGRATE.md` — 从 Kotlin 版迁移操作指南（h2-dump / 备份导入）
- `docs/api/rest-endpoints-baseline.md` — REST v1 端点兼容基线
- `docs/graphql/README.md` — GraphQL schema 基线说明

## Docker

```bash
docker build -t suwayomi-next .
docker run -p 8090:8090 -v suwayomi-data:/data suwayomi-next   # 容器内与宿主均 8090
```

## 许可证

Mozilla Public License, v.2.0

    Copyright (C) Contributors to the Suwayomi project
    
    This Source Code Form is subject to the terms of the Mozilla Public
    License, v. 2.0. If a copy of the MPL was not distributed with this
    file, You can obtain one at http://mozilla.org/MPL/2.0/.

## 免责声明

本应用的开发者与所提供的内容源 / 内容提供方没有任何关联。
