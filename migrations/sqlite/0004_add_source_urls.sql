-- 同 PostgreSQL 的 `0006_add_source_urls`：源的主页地址。
--
-- SQLite 的 `ALTER TABLE ADD COLUMN` 没有 `IF NOT EXISTS`，但整份迁移脚本在
-- 一个事务里执行、失败整体回滚（含 DDL），不会留下改了一半的 schema。
ALTER TABLE source ADD COLUMN base_url TEXT;
ALTER TABLE source ADD COLUMN home_url TEXT;
