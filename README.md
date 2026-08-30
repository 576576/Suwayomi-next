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
