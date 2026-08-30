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
| 5 | 扩展桥接层（JVM 沙盒） | 🔶 进程桥接完成，真实扩展加载增量 |
| 6 | 外围功能（下载/更新/备份/Tracker/OPDS/同步/Tauri 壳） | 🔶 OPDS/下载器/更新器/备份完成，同步增量 |
| 7 | 数据迁移工具与发布 | 🟢 h2-dump/--migrate/备份导入/Docker/文档完成 |

## 快速开始

```bash
# 构建 + 启动（默认端口 4567，内置嵌入式 PGlite，零外部依赖）
cargo run --release -p suwayomi-server
```

- WebUI：`http://localhost:4567`
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
./target/debug/suwayomi-server
```

环境变量：`SUWAYOMI_SANDBOX_JAR`（启用沙盒）、`SUWAYOMI_SANDBOX_PORT`（默认 8091，避开 Windows 动态端口保留区 4501–4900）、`SUWAYOMI_EXTENSIONS_DIR`（默认 ./extensions）、`SUWAYOMI_SANDBOX_PROXY`（可选 HTTP 代理）。未配置时回退内置 `StubFetcher`。

## 关键文档

- `MIGRATION_PLAN.md` — 分阶段迁移计划（含决策记录 R1–R8）
- `MIGRATION_STATUS.md` — 逐文件迁移追踪
- `docs/user-guide.md` — 用户指南（配置/迁移/备份/OPDS/Docker）
- `docs/api/rest-endpoints-baseline.md` — REST v1 端点兼容基线
- `docs/graphql/README.md` — GraphQL schema 基线说明

## 构建

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Docker

```bash
docker build -t suwayomi-next .
docker run -p 4567:4567 -v suwayomi-data:/data suwayomi-next
```

## License

MPL-2.0（与上游一致）
