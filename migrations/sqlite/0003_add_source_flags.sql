-- 同 PostgreSQL 的 `0005_add_source_flags`：源的两个能力位。
--
-- SQLite 的 `ALTER TABLE ADD COLUMN` 没有 `IF NOT EXISTS`，但整份迁移脚本在
-- 一个事务里执行、失败整体回滚（含 DDL），不会留下改了一半的 schema。
ALTER TABLE source ADD COLUMN supports_latest INTEGER NOT NULL DEFAULT 0;
ALTER TABLE source ADD COLUMN is_configurable INTEGER NOT NULL DEFAULT 0;
