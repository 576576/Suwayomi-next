-- 源的主页地址，取自沙盒（扩展自己的 `HttpSource.getBaseUrl()` / `getHomeUrl()`）：
-- 图源详情页要显示一个能点开的主页链接，此前 GraphQL 的 `SourceType.baseUrl` /
-- `homeUrl` 恒为 null。
--
-- 与 `0005_add_source_flags` 同一路数：沙盒 `/sources` 上报 → 建源行时写入 →
-- resolver 直接读列。可空：老沙盒不报这两个字段，非 HttpSource 的源也没有。
ALTER TABLE suwayomi.source ADD COLUMN IF NOT EXISTS base_url VARCHAR(2048);
ALTER TABLE suwayomi.source ADD COLUMN IF NOT EXISTS home_url VARCHAR(2048);
