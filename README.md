# Suwayomi-next

Suwayomi-Server（Kotlin/JVM）的 Rust 重写版。目标：保持既有数据、GraphQL/REST/OPDS 接口、Mihon 扩展体系完全兼容。

## 状态

| Phase | 内容 | 状态 |
| --- | --- | --- |
| 0 | 工作区骨架与兼容基线 | 🟢 完成 |
| 1 | 核心数据模型与数据库层 | 待执行 |
| 2 | 核心业务逻辑（domain） | 待执行 |
| 3 | REST API v1 | 待执行 |
| 4 | GraphQL API | 待执行 |
| 5 | 扩展桥接层（JVM 沙盒） | 待执行 |
| 6 | 外围功能（下载/更新/备份/Tracker/OPDS/同步/Tauri 壳） | 待执行 |
| 7 | 数据迁移工具与发布 | 待执行 |

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
tauri-app/           桌面壳（R3：移除 CEF 改用 Tauri，补全系统托盘）
migrations/          SQL 迁移（PG: migrations/，SQLite: migrations/sqlite/）
docs/                基线文档（REST 端点 / GraphQL schema / 迁移说明）
```

## 关键文档

- `MIGRATION_PLAN.md` — 分阶段迁移计划（含决策记录 R1–R8）
- `MIGRATION_STATUS.md` — 逐文件迁移追踪
- `docs/api/rest-endpoints-baseline.md` — REST v1 端点兼容基线
- `docs/graphql/README.md` — GraphQL schema 基线说明

## 构建

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## License

MPL-2.0（与上游一致）
