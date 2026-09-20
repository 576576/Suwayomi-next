-- 源的两个能力位，取自沙盒（扩展自己的 Source 实现）：
--   supports_latest —— 源提供"最近更新"列表
--   is_configurable —— 源实现 ConfigurableSource，有设置界面
--
-- 此前 GraphQL 侧把它们硬编码成 false，表现为"图源页没有最近更新按钮"
-- "可配置的源也看不到设置入口"。
ALTER TABLE suwayomi.source ADD COLUMN IF NOT EXISTS supports_latest BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE suwayomi.source ADD COLUMN IF NOT EXISTS is_configurable BOOLEAN NOT NULL DEFAULT FALSE;
