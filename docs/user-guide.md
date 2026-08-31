# Suwayomi (next) 用户指南

Suwayomi (next) 是 Suwayomi（Kotlin/JVM 版）的 Rust 重写：保持既有数据格式、
GraphQL / REST / OPDS 接口与 Mihon 扩展体系兼容，默认**零外部依赖**启动
（内置嵌入式 Oliphaunt 数据库，真实 PostgreSQL 18 引擎）。

## 快速开始

```bash
# 直接运行（默认端口 8090，嵌入式数据库，数据存 ./pglite-data）
cargo run --release -p suwayomi-server
# 或使用已构建二进制
./target/release/suwayomi-server
```

- WebUI：`http://localhost:8090`（托管目录，见下）
- GraphQL：`http://localhost:8090/api/graphql`
- REST：`http://localhost:8090/api/v1`
- OPDS：`http://localhost:8090/api/opds/v1.2`（KOReader 等阅读器）

## 配置（环境变量）

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `SUWAYOMI_PORT` | `8090` | HTTP 端口 |
| `SUWAYOMI_IP` | `0.0.0.0` | 监听地址 |
| `SUWAYOMI_PGLITE_DATA_DIR` | `./pglite-data` | 嵌入式数据库数据目录（空 = 临时） |
| `SUWAYOMI_DATABASE_URL` | （空） | 设置后改用外部 PostgreSQL（如 `postgres://user:pass@host:5432/db`） |
| `SUWAYOMI_AUTH_MODE` | `DISABLED` | 认证模式：`DISABLED` / `SIMPLE_LOGIN` / `BASIC_AUTH` |
| `SUWAYOMI_AUTH_USERNAME` / `SUWAYOMI_AUTH_PASSWORD` | — | 认证凭据 |
| `SUWAYOMI_SANDBOX_JAR` | — | JVM 扩展沙盒 jar 路径（未设置则扩展源不可用） |
| `SUWAYOMI_SANDBOX_PORT` | `8091` | 沙盒 HTTP 端口 |
| `SUWAYOMI_EXTENSIONS_DIR` | `./extensions` | 扩展 APK 目录（只放 APK） |
| `SUWAYOMI_JAR_DIR` | `<extensions>/../bin/extensions` | dex2jar 转换产物 jar 目录 |
| `SUWAYOMI_H2_DUMP_JAR` | `tools/h2-dump/build/libs/h2-dump.jar` | `--migrate` 用的导出工具 jar |

## 从 Kotlin 版迁移（Phase 7）

Kotlin 版使用 H2 数据库文件（JVM 专有格式，Rust 无法直读）。完整迁移操作指南见
**`docs/migration/MIGRATE.md`**，支持两种路径：

- 路径 A：`suwayomi-server --migrate <kotlin-data-dir>`（h2-dump 全量导出导入，推荐）
- 路径 B：Mihon `.proto` 备份导入（`POST /api/v1/backup/import`）

## 备份

- 导出（流式 gzip protobuf）：`GET /api/v1/backup/export`
- 导出文件：`GET /api/v1/backup/export/file`（`org.suwayomi.next_<ts>.tachibk`）
- 导入：`POST /api/v1/backup/import`（body 为 gzip 备份）

## OPDS / KOReader

根目录：`http://localhost:8090/api/opds/v1.2`
（支持：库浏览、来源探索、历史、库更新、系列章节、章节元数据；`?lang=` 切换语言）

## Docker

```bash
docker build -t suwayomi-next .
docker run -p 8090:8090 -v suwayomi-data:/data suwayomi-next
```

数据持久化在 `/data/pglite-data`。要连外部 PostgreSQL：

```bash
docker run -p 8090:8090 -e SUWAYOMI_DATABASE_URL=postgres://user:pass@host:5432/db suwayomi-next
```

## 已知限制

- 嵌入式 Oliphaunt（真实 PostgreSQL）在**任何 SQL 报错时不受影响**（旧
  pglite-oxide 代理会终止会话，已随 2026-08-31 迁移消除）；运行时意外
  SQL 错误由连接池自动重建。
- 真实扩展源（Mihon APK→JAR）依赖 JVM 沙盒（`SUWAYOMI_SANDBOX_JAR`）；
  未配置时来源相关端点返回"source unavailable"。
