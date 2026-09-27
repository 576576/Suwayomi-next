# GraphQL Schema 基线

`schema-baseline.graphql` 是 Rust 版 GraphQL 层的兼容对照物（Phase 4 验收标准：`graphql-inspector diff` 无 breaking change）。

## 当前状态

- ✅ **基线已导出**（2026-08-30）：运行 Kotlin 版 Suwayomi（本机 JDK 25 + Gradle 构建 installDist）introspection 导出，共 **359 个类型定义 / 3033 行 SDL**。
- 自动化脚本：`../../scripts/export-graphql-schema.sh`（重建 + 启动 + introspection + 转 SDL；本机验证可行，注意 Git Bash 命令行参数长度限制——长 introspection 载荷需经脚本文件发送）。

## Rust 侧现状（2026-09-27 实测）

`crates/suwayomi-graphql` 产出的 SDL 与上面的基线对比，由 `schema.rs` 的
`tests::schema_matches_baseline` **在测试里断言**：`include_str!` 把本目录的
`schema-baseline.graphql` 在编译期嵌进测试二进制，所以基线被改坏、或本 fork 侧
实现了/新增了类型而没同步这里，`cargo test` 都会红。

| 口径（含 `union`、不含 `directive`） | 数 |
|---|---:|
| 基线（`/api/v1` 与 GraphQL 由 Kotlin 版导出） | 359 |
| 本 fork 产出（`schema_type_count()`，启动日志同源） | 350 |

差分是稳定且刻意的两部分：

- **未实现 17 个**：`Node` / `Edge` / `NodeList` / `MetaType` 及各自的实现类型 ——
  基线里 6 个 `interface` **全**在这堆里（本 fork 侧一个 `interface` 都没注册）；
  `Settings` / `PartialSettingsType` / `SettingsDownloadConversion(Header)` /
  `UpdateState` / `DownloadUpdate(Type)` / `DownloadEdge` / `DownloadNodeList` /
  5 个 `WebUIUpdate*` / `union Node`。
- **本 fork 自有 8 个**：`RebuildDownloadIndexInput`+`Payload`、
  `RefreshTrackerUserInput`+`Payload`、`ReorderChapterDownloadsPayload`、
  `TrackerOAuthAppType`、`UpdateTrackerOAuthAppInput`+`Payload`。

⚠️ 因此上面那条 Phase 4 验收标准（`graphql-inspector diff` 无 breaking change）
**目前并未满足**：缺的这 17 个里有查询/变更的返回类型（`NodeList`/`Edge` 家族、
`Settings`、`WebUIUpdate*`），对老 WebUI 是破坏性的。补齐还是明确放弃，需要单独决策。

## 源 Schema 构成（实测基线 + Kotlin 源码审计）

- **Queries（15）**：Backup / Category / Chapter / Download / Extension / ExtensionStore / Info / KoreaderSync / Manga / Meta / Settings / Source / Sync / Track / Update
- **Mutations（18）**：Backup / Category / Chapter / Download / Extension / ExtensionStore / Image / Info / KoreaderSync / Manga / Meta / Settings / Sync / Source / Track / Update / User / Webview
- **Subscriptions（4）**：Download / Info / Sync / Update
- **自定义标量（实测名）**：`LongString`（Long→String，JS 精度）、`Duration`（ISO-8601）、`Cursor`、`Upload`（multipart）
- **指令**：`@requireAuth`（自定义）+ 标准 `@defer`/`@stream`/`@skip`/`@include`/`@deprecated`/`@specifiedBy`/`@oneOf`
- **特殊行为**：
  - `NodeList` 分页返回 `nodes/edges/pageInfo/totalCount`，edges 仅首尾两条（`getEdges` 实现）
  - 类型中的 `Long` 字段一律以 String 输出（LongString）
  - **注意**：Kotlin 侧实际注册的标量名是 `LongString`/`Duration`（并非最初假设的 LongAsString/DurationAsString），Phase 4 以本基线为准
