//! 各站点的 OAuth 应用凭据（client id / secret / 回调地址）。
//!
//! 首次启动在 `<settings>/trackers.json` 生成一份等于内置默认值的副本，之后每次
//! 启动读取它。缺键（整个站点、或站点下的某个字段）用内置默认值补齐；文件已存在
//! 就永不重写（只有 [`save`] 会写），读取或解析失败只告警并退回默认。**显式写成
//! 空串**表示「不配置」，用到时报错而不回退默认 —— 否则会把用户的授权静默送到
//! 别人的应用上。
//!
//! 运行期改动（设置页的齿轮）走 [`save`] + [`TrackerOAuthApps::set_app`]：落盘与
//! 生效一起做，下一次登录/刷新就用新值。
//!
//! 用户的 token 不在这个文件里，那些在数据库（`tracker_credential`）。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{DomainError, Result};

use super::{ANILIST, BANGUMI, KITSU, MYANIMELIST, SHIKIMORI};
use super::{anilist, bangumi, kitsu, myanimelist, shikimori};

/// 一个站点的应用凭据。站点不需要的字段留空（如 AniList 只有 client id）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppCredentials {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl AppCredentials {
    fn new(client_id: &str, client_secret: &str, redirect_uri: &str) -> Self {
        Self {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            redirect_uri: redirect_uri.to_string(),
        }
    }

    pub fn client_id(&self, tracker: &str) -> Result<&str> {
        non_empty(&self.client_id, tracker, "clientId")
    }

    pub fn client_secret(&self, tracker: &str) -> Result<&str> {
        non_empty(&self.client_secret, tracker, "clientSecret")
    }

    pub fn redirect_uri(&self, tracker: &str) -> Result<&str> {
        non_empty(&self.redirect_uri, tracker, "redirectUri")
    }
}

fn non_empty<'a>(value: &'a str, tracker: &str, key: &str) -> Result<&'a str> {
    if value.is_empty() {
        return Err(DomainError::tracker(format!("{tracker}：trackers.json 的 {key} 为空")));
    }
    Ok(value)
}

/// 全部站点的应用凭据。[`Default`] 即内置默认值（上游 Suwayomi 注册的应用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerOAuthApps {
    pub anilist: AppCredentials,
    pub bangumi: AppCredentials,
    pub kitsu: AppCredentials,
    pub mal: AppCredentials,
    pub shikimori: AppCredentials,
}

impl Default for TrackerOAuthApps {
    fn default() -> Self {
        Self {
            anilist: AppCredentials::new(anilist::DEFAULT_CLIENT_ID, "", ""),
            bangumi: AppCredentials::new(
                bangumi::DEFAULT_CLIENT_ID,
                bangumi::DEFAULT_CLIENT_SECRET,
                bangumi::DEFAULT_REDIRECT_URL,
            ),
            kitsu: AppCredentials::new(kitsu::DEFAULT_CLIENT_ID, kitsu::DEFAULT_CLIENT_SECRET, ""),
            mal: AppCredentials::new(myanimelist::DEFAULT_CLIENT_ID, "", ""),
            shikimori: AppCredentials::new(
                shikimori::DEFAULT_CLIENT_ID,
                shikimori::DEFAULT_CLIENT_SECRET,
                shikimori::DEFAULT_REDIRECT_URL,
            ),
        }
    }
}

impl TrackerOAuthApps {
    /// 按追踪器 id 取凭据；非 OAuth 站点（MangaUpdates）为 `None`。
    pub fn app(&self, tracker_id: i32) -> Option<&AppCredentials> {
        match tracker_id {
            MYANIMELIST => Some(&self.mal),
            ANILIST => Some(&self.anilist),
            KITSU => Some(&self.kitsu),
            SHIKIMORI => Some(&self.shikimori),
            BANGUMI => Some(&self.bangumi),
            _ => None,
        }
    }

    /// 改某个站点的凭据；非 OAuth 站点返回 `false`（没改）。
    pub fn set_app(&mut self, tracker_id: i32, app: AppCredentials) -> bool {
        let slot = match tracker_id {
            MYANIMELIST => &mut self.mal,
            ANILIST => &mut self.anilist,
            KITSU => &mut self.kitsu,
            SHIKIMORI => &mut self.shikimori,
            BANGUMI => &mut self.bangumi,
            _ => return false,
        };
        *slot = app;
        true
    }
}

/// 读取配置文件；不存在就先按内置默认值写一份，再返回默认值。
pub fn load_or_create(path: &Path) -> TrackerOAuthApps {
    let defaults = TrackerOAuthApps::default();
    if !path.exists() {
        match save(path, &defaults) {
            Ok(()) => tracing::info!("tracker oauth config created: {}", path.display()),
            Err(e) => tracing::warn!("cannot create tracker oauth config {}: {e}", path.display()),
        }
        return defaults;
    }

    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) => {
            tracing::warn!("cannot read tracker oauth config {}: {e}", path.display());
            return defaults;
        }
    };
    let file: FileRoot = match serde_json::from_str(&raw) {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!("cannot parse tracker oauth config {}: {e}", path.display());
            return defaults;
        }
    };
    tracing::info!("tracker oauth config loaded: {}", path.display());
    TrackerOAuthApps {
        anilist: merge(file.anilist.as_ref(), defaults.anilist),
        bangumi: merge(file.bangumi.as_ref(), defaults.bangumi),
        kitsu: merge(file.kitsu.as_ref(), defaults.kitsu),
        mal: merge(file.mal.as_ref(), defaults.mal),
        shikimori: merge(file.shikimori.as_ref(), defaults.shikimori),
    }
}

fn merge(raw: Option<&FileApp>, default: AppCredentials) -> AppCredentials {
    let Some(raw) = raw else {
        return default;
    };
    AppCredentials {
        client_id: raw.client_id.clone().unwrap_or(default.client_id),
        client_secret: raw.client_secret.clone().unwrap_or(default.client_secret),
        redirect_uri: raw.redirect_uri.clone().unwrap_or(default.redirect_uri),
    }
}

/// 把整份凭据写回文件（与首次生成同形状：五个站点全写）。
pub fn save(path: &Path, apps: &TrackerOAuthApps) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let root = FileRoot {
        anilist: Some(FileApp::from(&apps.anilist)),
        bangumi: Some(FileApp::from(&apps.bangumi)),
        kitsu: Some(FileApp::from(&apps.kitsu)),
        mal: Some(FileApp::from(&apps.mal)),
        shikimori: Some(FileApp::from(&apps.shikimori)),
    };
    let mut json = serde_json::to_string_pretty(&root).map_err(std::io::Error::other)?;
    json.push('\n');
    std::fs::write(path, json)
}

/// 只认五个站点；文件里的其他键（早期版本写过的 `_comment`）忽略。
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct FileRoot {
    anilist: Option<FileApp>,
    bangumi: Option<FileApp>,
    kitsu: Option<FileApp>,
    mal: Option<FileApp>,
    shikimori: Option<FileApp>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct FileApp {
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    redirect_uri: Option<String>,
}

impl From<&AppCredentials> for FileApp {
    fn from(app: &AppCredentials) -> Self {
        Self {
            client_id: Some(app.client_id.clone()),
            client_secret: (!app.client_secret.is_empty()).then(|| app.client_secret.clone()),
            redirect_uri: (!app.redirect_uri.is_empty()).then(|| app.redirect_uri.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::{BANGUMI, KITSU};

    fn temp_file() -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("tracker-oauth-test-{}", uuid::Uuid::new_v4()))
            .join("trackers.json")
    }

    #[test]
    fn creates_file_with_builtin_defaults() {
        let path = temp_file();
        let apps = load_or_create(&path);
        assert_eq!(apps, TrackerOAuthApps::default());

        let raw = std::fs::read_to_string(&path).unwrap();
        // 生成的文件只有五个站点，没有别的键
        let root: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let mut keys: Vec<&str> = root.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["anilist", "bangumi", "kitsu", "mal", "shikimori"]);
        assert_eq!(apps.bangumi.client_id, bangumi::DEFAULT_CLIENT_ID);
        assert_eq!(apps.bangumi.redirect_uri, bangumi::DEFAULT_REDIRECT_URL);
        assert_eq!(apps.mal.client_id, myanimelist::DEFAULT_CLIENT_ID);

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn file_with_extra_keys_still_loads() {
        let path = temp_file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // 早期版本在文件里写过说明行，读的时候要无视它
        std::fs::write(&path, r#"{"_comment": "whatever", "bangumi": {"clientId": "bgmCustom"}}"#).unwrap();

        let apps = load_or_create(&path);
        assert_eq!(apps.bangumi.client_id, "bgmCustom");
        assert_eq!(apps.bangumi.client_secret, bangumi::DEFAULT_CLIENT_SECRET);

        // 再保存一次就不再写出那个键
        save(&path, &apps).unwrap();
        let root: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(root.get("_comment").is_none());

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn missing_keys_fall_back_to_default() {
        let path = temp_file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"bangumi": {"clientId": "bgmCustom"}}"#).unwrap();

        let apps = load_or_create(&path);
        assert_eq!(apps.bangumi.client_id, "bgmCustom");
        assert_eq!(apps.bangumi.client_secret, bangumi::DEFAULT_CLIENT_SECRET);
        assert_eq!(apps.bangumi.redirect_uri, bangumi::DEFAULT_REDIRECT_URL);
        assert_eq!(apps.kitsu, TrackerOAuthApps::default().kitsu);

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn broken_file_is_not_rewritten() {
        let path = temp_file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();

        assert_eq!(load_or_create(&path), TrackerOAuthApps::default());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn explicit_empty_does_not_fall_back() {
        let path = temp_file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"bangumi": {"clientId": ""}}"#).unwrap();

        let apps = load_or_create(&path);
        assert_eq!(apps.bangumi.client_id, "");
        assert!(apps.bangumi.client_id("Bangumi").is_err());
        assert!(apps.bangumi.client_secret("Bangumi").is_ok());

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn saved_values_survive_a_reload() {
        let path = temp_file();
        let mut apps = load_or_create(&path);
        let mut custom = apps.app(BANGUMI).unwrap().clone();
        custom.client_id = "bgmFromUi".to_string();
        assert!(apps.set_app(BANGUMI, custom));
        save(&path, &apps).unwrap();

        let reloaded = load_or_create(&path);
        assert_eq!(reloaded.bangumi.client_id, "bgmFromUi");
        assert_eq!(reloaded.kitsu, TrackerOAuthApps::default().kitsu);

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn unknown_tracker_has_no_credentials() {
        let mut apps = TrackerOAuthApps::default();
        assert!(apps.app(999).is_none());
        assert!(!apps.set_app(999, AppCredentials::default()));
        assert!(apps.app(KITSU).is_some());
    }
}
