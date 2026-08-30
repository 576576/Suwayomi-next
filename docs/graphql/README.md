# GraphQL Schema 基线

`schema-baseline.graphql` 是 Rust 版 GraphQL 层的兼容对照物（Phase 4 验收标准：`graphql-inspector diff` 无 breaking change）。

## 当前状态

- ✅ **基线已导出**（2026-08-30）：运行 Kotlin 版 Suwayomi（本机 JDK 25 + Gradle 构建 installDist）introspection 导出，共 **359 个类型定义 / 3033 行 SDL**。
- 自动化脚本：`../../scripts/export-graphql-schema.sh`（重建 + 启动 + introspection + 转 SDL；本机验证可行，注意 Git Bash 命令行参数长度限制——长 introspection 载荷需经脚本文件发送）。

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
