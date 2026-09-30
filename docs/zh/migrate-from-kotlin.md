# 从 Kotlin 版迁移到 Suwayomi-next

Kotlin 原版（Suwayomi/Tachidesk）使用 H2 数据库文件（JVM 专有格式，Rust 无法直接读取），
数据目录内通常有 `tachidesk.mv.db`（或自定义文件名）。Rust 版**不读取 H2 文件**，
迁移走 Mihon 备份导入。

## 备份导入（Mihon .proto 备份）

在 Kotlin 版（或 Mihon App）里导出 `.tachibk` 备份，然后导入 Rust 版：

```bash
# 校验（不落库）：
curl -X POST http://localhost:4567/api/v1/backup/validate --data-binary @backup.tachibk
# 导入：
curl -X POST http://localhost:4567/api/v1/backup/import --data-binary @backup.tachibk
```

导出（用于反向迁移或日常备份）：`GET /api/v1/backup/export`。

导入目标后端由 `SUWAYOMI_DB_BACKEND` / `SUWAYOMI_DATABASE_URL` 决定：不设 → 本地
SQLite（`<appdata>/db/suwayomi.db`，appdata 根默认是 exe 上级的 `appdata/`）；`SUWAYOMI_DB_BACKEND=postgres` 或设置
`SUWAYOMI_DATABASE_URL` → 外部 PostgreSQL（`postgres://user:pass@host:5432/db`）。

## 相关环境变量

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `SUWAYOMI_APPDATA_DIR` | exe 上级 `appdata/` | 程序自身状态的唯一可写根，库固定落在其 `db/` 之下 |
| `SUWAYOMI_DB_BACKEND` | `sqlite` | 后端（`sqlite` / `postgres`） |
| `SUWAYOMI_DATABASE_URL` | （空） | PostgreSQL 连接串 |

库的位置没有独立的环境变量：要挪库就挪整个 `appdata/`。目录布局变化是**破坏性的**
—— 从更早的布局（发布根下的 `db/`，或 `<数据目录>/suwayomi.db`）升级时不会自动搬运，
把旧库手动拷到 `<appdata>/db/suwayomi.db` 即可（`-wal` / `-shm` 一并带上）。
