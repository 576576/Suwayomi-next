-- 追踪器凭据（登录后的 token / 用户名 / 密码）。
--
-- 上游把这些值放在客户端侧的 SharedPreferences（`TrackerPreferences.kt`），
-- 服务端没有对应表；这里落库是因为 Rust 侧没有等价的 preference store，而且
-- 重启后要保住登录态。`tracker_id` 用上游 `TrackerManager` 的常量（1 MAL /
-- 2 AniList / 3 Kitsu / 4 Shikimori / 5 Bangumi / 7 MangaUpdates）。
CREATE TABLE IF NOT EXISTS suwayomi.tracker_credential (
    tracker_id    INTEGER PRIMARY KEY,
    username      VARCHAR(512) NOT NULL DEFAULT '',
    password      VARCHAR(2048) NOT NULL DEFAULT '',
    token         VARCHAR(4096) NOT NULL DEFAULT '',
    token_expired BOOLEAN NOT NULL DEFAULT FALSE,
    score_type    VARCHAR(128) NOT NULL DEFAULT '',
    -- MyAnimeList 用 PKCE：`authUrl` 生成 code_verifier，换 token 时要原样回传。
    -- 上游把它放在进程内的静态变量里，重登窗口内重启就丢了；这里落库。
    pkce_verifier VARCHAR(256) NOT NULL DEFAULT ''
);
