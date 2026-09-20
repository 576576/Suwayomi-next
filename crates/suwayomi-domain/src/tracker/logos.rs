//! 各追踪器的图标。上游用 `getResourceAsStream("/static/tracker/*.png")` 从
//! classpath 取，这里用 `include_bytes!` 编进二进制 —— 缩略图路由因此不联网。

pub const MAL: &[u8] = include_bytes!("../../../../assets/images/tracker/mal.png");
pub const ANILIST: &[u8] = include_bytes!("../../../../assets/images/tracker/anilist.png");
pub const KITSU: &[u8] = include_bytes!("../../../../assets/images/tracker/kitsu.png");
pub const SHIKIMORI: &[u8] = include_bytes!("../../../../assets/images/tracker/shikimori.png");
pub const BANGUMI: &[u8] = include_bytes!("../../../../assets/images/tracker/bangumi.png");
pub const MANGA_UPDATES: &[u8] = include_bytes!("../../../../assets/images/tracker/manga_updates.png");
