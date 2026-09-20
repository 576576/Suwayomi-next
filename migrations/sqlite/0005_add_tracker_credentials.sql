-- 同 PostgreSQL 的 `0007_add_tracker_credentials`：追踪器凭据。
--
-- SQLite 的 `ALTER TABLE ADD COLUMN` 没有 `IF NOT EXISTS`，但整份迁移脚本在
-- 一个事务里执行、失败整体回滚（含 DDL），不会留下改了一半的 schema。
CREATE TABLE IF NOT EXISTS tracker_credential (
    tracker_id    INTEGER PRIMARY KEY,
    username      TEXT NOT NULL DEFAULT '',
    password      TEXT NOT NULL DEFAULT '',
    token         TEXT NOT NULL DEFAULT '',
    token_expired INTEGER NOT NULL DEFAULT 0,
    score_type    TEXT NOT NULL DEFAULT '',
    pkce_verifier TEXT NOT NULL DEFAULT ''
);
