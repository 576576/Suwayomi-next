# 从 Kotlin 版迁移到 Suwayomi-next

Kotlin 原版（Suwayomi/Tachidesk）使用 H2 数据库文件（JVM 专有格式，Rust 无法直接读取），
数据目录内通常有 `tachidesk.mv.db`（或自定义文件名）。Rust 版提供两种迁移路径，按需选择。

## 路径 A：`--migrate`（推荐，完整保留）

自动定位 H2 文件 → `tools/h2-dump` 导出为 PostgreSQL 脚本 → 导入当前后端 → 退出。
库 / 章节 / 阅读进度 / 分类 / meta 全量保留。

```bash
# 1) 构建迁移工具（JDK 17+，首次需要）
gradle -p tools/h2-dump build

# 2) 执行迁移（数据写到嵌入式 PGlite，即 ./pglite-data）
suwayomi-server --migrate <kotlin-data-dir>
# 或指定 h2-dump jar 路径：
suwayomi-server --migrate <kotlin-data-dir> --h2-dump-jar <path>

# 3) 正常启动即可
suwayomi-server
```

- 迁移目标后端由 `SUWAYOMI_DATABASE_URL` 决定：不设 → 嵌入式 PGlite；设置 →
  外部 PostgreSQL（`postgres://user:pass@host:5432/db`）。
- 流程：定位 `<dir>/*.mv.db` → h2-dump 导出（按外键依赖序）→ 逐条导入 → 退出。
- h2-dump 导入脚本幂等（先 DELETE 再 INSERT），重复执行安全。

## 路径 B：备份导入（Mihon .proto 备份）

Kotlin 版导出的 Mihon `.proto` 备份（gzip 体）可直接导入，适合只需要内容
（库 + 进度）而不需要全部元数据的场景：

```bash
# 校验（不落库）：
curl -X POST http://localhost:4567/api/v1/backup/validate --data-binary @backup.tachibk
# 导入：
curl -X POST http://localhost:4567/api/v1/backup/import --data-binary @backup.tachibk
```

导出（用于反向迁移或日常备份）：`GET /api/v1/backup/export`。

## 相关环境变量

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `SUWAYOMI_H2_DUMP_JAR` | `tools/h2-dump/build/libs/h2-dump.jar` | `--migrate` 用的导出工具 jar |
| `SUWAYOMI_DATABASE_URL` | 嵌入式 PGlite | 迁移目标数据库连接串 |

## 更多迁移背景

- `MIGRATION_PLAN.md` — 分阶段迁移计划与决策记录（R1–R8）
- `MIGRATION_STATUS.md` — 逐文件迁移追踪
- `rust-migrate-guide-0.md` — 迁移初始盘点（源仓库目录对照）
