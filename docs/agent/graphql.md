# GraphQL Schema 基线

`crates/suwayomi-graphql/baseline/schema-baseline.graphql` 是 Rust 版 GraphQL 层的兼容对照物（Phase 4 验收标准：`graphql-inspector diff` 无 breaking change）。

## 当前状态

- ✅ **基线已导出**（2026-08-30）：运行 Kotlin 版 Suwayomi（本机 JDK 25 + Gradle 构建 installDist）introspection 导出，共 **359 个类型定义 / 3033 行 SDL**。
- 自动化脚本：`../../scripts/export-graphql-schema.sh`（重建 + 启动 + introspection + 转 SDL；本机验证可行，注意 Git Bash 命令行参数长度限制——长 introspection 载荷需经脚本文件发送）。

## Rust 侧现状（2026-09-27 实测）

`crates/suwayomi-graphql` 产出的 SDL 与上面的基线对比，由 `schema.rs` 的
`tests::schema_matches_baseline` **在测试里断言**：`include_str!` 把同 crate 下的
`baseline/schema-baseline.graphql` 在编译期嵌进测试二进制，所以基线被改坏、或本 fork
侧实现了/新增了类型而没同步这里，`cargo test` 都会红。

| 口径（含 `union`、不含 `directive`） | 数 |
|---|---:|
| 基线（`/api/v1` 与 GraphQL 由 Kotlin 版导出） | 359 |
| 本 fork 产出（`schema_type_count()`，启动日志同源） | 352 |

差分是稳定且刻意的两部分：

- **未实现 15 个**：`Node` / `Edge` / `NodeList` / `MetaType` 及各自的实现类型 ——
  基线里 6 个 `interface` **全**在这堆里（本 fork 侧一个 `interface` 都没注册）；
  `Settings` / `PartialSettingsType` / `SettingsDownloadConversion(Header)` /
  `UpdateState` / `DownloadEdge` / `DownloadNodeList` /
  5 个 `WebUIUpdate*` / `union Node`。
  （`DownloadUpdate` / `DownloadUpdateType` 原先也在这堆里，只因为它们挂不上
  `DownloadUpdates` 的字段而没被注册；补上 `updates` 后已可达。）
- **固定丢弃、不打算补的**：5 个 `WebUIUpdate*`。WebUI 自更新已交给桌面托盘，
  WebUI 侧对应的 mutation / subscription / query 已整组删除
  （`src/lib/graphql/server/ServerInfo{Mutation,Subscription}.ts`），没有消费者。
- **本 fork 自有 8 个**：`RebuildDownloadIndexInput`+`Payload`、
  `RefreshTrackerUserInput`+`Payload`、`ReorderChapterDownloadsPayload`、
  `TrackerOAuthAppType`、`UpdateTrackerOAuthAppInput`+`Payload`。

### 字段级差异（不影响上面的类型计数，2026-09-30 实测）

本 fork 有几处字段曾比基线**更严**（非空 vs 可空），已按基线放松：

| 字段 | 基线 | 放松前本 fork | 非空会怎么错 |
|---|---|---|---|
| `MangaType.thumbnailUrlLastFetched` | `LongString` | `LongString!` | `0` 是「从未抓取」的哨兵值，不是时间戳 |
| `SettingsDownloadConversionType.callTimeout` / `connectTimeout` | `Duration` | `Duration!` | 只发 `0` 秒超时的假值，客户端的「留空 = 用默认」表达不出来 |
| `SettingsDownloadConversionType.headers` | `[SettingsDownloadConversionHeaderType!]` | `...!` | 空 `headers` 与「没设过」不可区分 |

⚠️ 因此上面那条 Phase 4 验收标准（`graphql-inspector diff` 无 breaking change）
**目前并未满足**：缺的这 15 个里有查询/变更的返回类型（`NodeList`/`Edge` 家族、
`Settings`、`WebUIUpdate*`），对老 WebUI 是破坏性的。补齐还是明确放弃，需要单独决策。

## WebUI 侧：generated 类型的 schema 来源

`Suwayomi-WebUI/src/lib/graphql/generated/`（`graphql-base.types.ts` / `graphql.ts` /
`apollo-helpers.ts`）由**本仓自己的** server 生成：

```sh
pnpm gql:codegen     # schema 取自 .env 的 CODEGEN_SERVER_URL_GQL
```

`CODEGEN_SERVER_URL_GQL` 默认是 `http://localhost:4567/api/graphql`（本仓 server 的默认
端口，见 `crates/suwayomi-core/src/config.rs`）。

三条约束：

- **必须指向本仓的 Rust server**。指错时生成的类型描述的是一个本 fork 不发布的
  schema：前端能编译通过，请求却落在服务端根本没有的类型/字段上（`MetaType`、
  `PartialSettingsType`、`SettingsDownloadConversion(Header)`、`UpdateState`、
  `extension(condition: { storeIndexUrl })` 都属于这一批）。本机实例若不在这个端口上
  （例如 4567 被占、server 自顺延到了 4568），改 `.env` 的 `CODEGEN_SERVER_URL_GQL`
  覆盖即可。
- `gql:codegen` 末尾会跑 `oxfmt` 格式化 generated 目录。漏掉这步 `format:check`
  （PR CI 会执行）必红 —— graphql-codegen 自己的缩进/换行跟仓库的 oxfmt 风格不一致。
- 服务端增删类型或字段后必须重新生成，`pnpm tsc` 会立刻指出前端对不上本仓 schema 的位置。

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
