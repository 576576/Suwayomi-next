//! HTTP client for the JVM extension sandbox (`ext-runtime.jar`).
//!
//! `HttpSandboxFetcher` implements `SourceFetcher` by calling the sandbox
//! process over its stable HTTP/JSON contract. The sandbox is started and
//! supervised by the server; this type only needs its base URL.

use async_trait::async_trait;
use serde::Deserialize;

use suwayomi_core::backup::{BackupPreference, PreferenceValue, preference_value};
use suwayomi_core::source::{MangasPage, SChapter, SManga};

use crate::error::{DomainError, Result};
use crate::source::SourceFetcher;
use suwayomi_core::text::urlencode;

/// 沙盒 HTTP 端口在 `SUWAYOMI_SANDBOX_PORT` 缺失时的落点：紧邻 server 默认端口
/// （4567）的下一格。桌面壳会给它传一个与 server 不同的空闲端口；裸跑 server 时
/// 这个值被占用（含 server 自身顺延过来）才由 `available_port` 再顺延。
pub const DEFAULT_SANDBOX_PORT: u16 = 4568;

/// 沙盒报上来的一个偏好项（`/source/{id}/preferences/raw`）。
///
/// 值带类型：`SharedPreferences` 把类型一起存下来，只传字符串的话整型/布尔读回来
/// 会落到默认值。`value` 保持 `serde_json::Value`，由 [`Self::to_backup_preference`]
/// 按 `value_type` 解释。
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct SandboxPreference {
    pub key: String,
    #[serde(rename = "type")]
    pub value_type: String,
    pub value: serde_json::Value,
}

impl SandboxPreference {
    /// → Mihon 的 `BackupPreference`（105 号段里的形态）。类型不认识时 `None`。
    ///
    /// `Float` 这里收窄成 `f32`：JSON 只有 f64，而 Mihon 的 `BackupPreference.Float`
    /// 就是 `Float`（32 位），位数对不上是协议本身的形状。
    pub fn to_backup_preference(&self) -> Option<BackupPreference> {
        let value = match self.value_type.as_str() {
            "Int" => preference_value::Value::Int(i32::try_from(self.value.as_i64()?).ok()?),
            "Long" => preference_value::Value::Long(self.value.as_i64()?),
            "Float" => preference_value::Value::Float(self.value.as_f64()? as f32),
            "String" => preference_value::Value::Text(self.value.as_str()?.to_string()),
            "Boolean" => preference_value::Value::Flag(self.value.as_bool()?),
            "StringSet" => preference_value::Value::StringSet(preference_value::StringSetValue {
                value: self.value.as_array()?.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            }),
            _ => return None,
        };
        Some(BackupPreference { key: self.key.clone(), value: Some(PreferenceValue { value: Some(value) }) })
    }

    /// [`Self::to_backup_preference`] 的逆操作。
    pub fn from_backup_preference(pref: &BackupPreference) -> Option<Self> {
        let (value_type, value) = match pref.value.as_ref()?.value.as_ref()? {
            preference_value::Value::Int(v) => ("Int", serde_json::json!(v)),
            preference_value::Value::Long(v) => ("Long", serde_json::json!(v)),
            preference_value::Value::Float(v) => ("Float", serde_json::json!(v)),
            preference_value::Value::Text(v) => ("String", serde_json::json!(v)),
            preference_value::Value::Flag(v) => ("Boolean", serde_json::json!(v)),
            preference_value::Value::StringSet(v) => ("StringSet", serde_json::json!(v.value)),
        };
        Some(Self { key: pref.key.clone(), value_type: value_type.to_string(), value })
    }
}

/// A source described by the sandbox (used for registration/debug).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxSourceInfo {
    pub id: i64,
    pub name: String,
    pub lang: String,
    pub extension: i32,
    /// 老沙盒不报这两个字段，缺省按"不支持 / 不可配置"处理。
    #[serde(default)]
    pub supports_latest: bool,
    #[serde(default)]
    pub is_configurable: bool,
    /// 源的主页地址（`HttpSource.getBaseUrl()` / `getHomeUrl()`）；老沙盒不报。
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub home_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxManga {
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub status: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxMangasPage {
    #[serde(default)]
    pub mangas: Vec<SandboxManga>,
    #[serde(default)]
    pub has_next_page: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxChapter {
    pub url: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub date_upload: i64,
    #[serde(default)]
    pub chapter_number: f32,
    #[serde(default)]
    pub scanlator: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxChapters {
    #[serde(default)]
    pub chapters: Vec<SandboxChapter>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxPage {
    #[serde(default)]
    pub index: i32,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxPages {
    #[serde(default)]
    pub pages: Vec<SandboxPage>,
}

/// Mirrors the sandbox /extensions payload.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxExtension {
    pub pkg_name: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub version_name: String,
    #[serde(default)]
    pub class_name: String,
    /// 沙盒侧解析出的 versionCode（Android 来自 `PackageInfo.longVersionCode`，
    /// 桌面来自 APK manifest）；老沙盒不报该字段时为 0。
    #[serde(default)]
    pub version_code: i64,
    /// 0 = Safe，1 = NSFW。来源是 APK manifest 的 `tachiyomi.extension.nsfw`。
    #[serde(default)]
    pub content_warning: i32,
    /// Sources this extension provides (id/name/lang) — links a sandbox
    /// source back to the extension package for registration.
    #[serde(default)]
    pub sources: Vec<SandboxSourceRef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxSourceRef {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub supports_latest: bool,
    #[serde(default)]
    pub is_configurable: bool,
    /// 源的主页地址（`HttpSource.getBaseUrl()` / `getHomeUrl()`）；老沙盒不报。
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub home_url: Option<String>,
}

/// Fetches manga/chapter data from the JVM sandbox over HTTP.
#[derive(Clone)]
pub struct HttpSandboxFetcher {
    base_url: String,
    client: reqwest::Client,
}

impl HttpSandboxFetcher {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: crate::http::build_client(
                reqwest::Client::builder()
                    .connect_timeout(std::time::Duration::from_secs(5))
                    // 本地回环绝不走代理：reqwest 默认读取 HTTP_PROXY/HTTPS_PROXY 等
                    // 环境变量（Clash 常设置），会把 127.0.0.1:4568 也转发到代理，
                    // 代理无法连接该端口返回 502 Bad Gateway（install reload 失败）。
                    .no_proxy(),
            ),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Health check — used by the server's sandbox lifecycle supervisor.
    pub async fn health(&self) -> bool {
        self.client
            .get(format!("{}/health", self.base_url))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    }

    /// Lists extensions known to the sandbox.
    pub async fn list_extensions(&self) -> Result<Vec<SandboxExtension>> {
        let r = self.client.get(format!("{}/extensions", self.base_url)).send().await.map_err(DomainError::from)?;
        r.json::<Vec<SandboxExtension>>().await.map_err(DomainError::from)
    }

    /// Lists sources known to the sandbox.
    pub async fn list_sources(&self) -> Result<Vec<SandboxSourceInfo>> {
        let r = self.client.get(format!("{}/sources", self.base_url)).send().await.map_err(DomainError::from)?;
        r.json::<Vec<SandboxSourceInfo>>().await.map_err(DomainError::from)
    }

    /// Asks the sandbox to rescan its extensions directory (hot reload).
    pub async fn reload(&self) -> Result<()> {
        let r = self.client.post(format!("{}/reload", self.base_url)).send().await.map_err(DomainError::from)?;
        if !r.status().is_success() {
            return Err(DomainError::Sandbox(format!("sandbox reload failed: {}", r.status())));
        }
        Ok(())
    }

    /// 源设置界面的 JSON 数组，由沙盒把 `PreferenceScreen` 序列化好。
    ///
    /// `None` = 该源没有设置界面（沙盒回 404）。调用方不要把它当错误：
    /// 绝大多数源本来就没有设置项。
    pub async fn source_preferences(&self, source_id: i64) -> Result<Option<String>> {
        let r = self
            .client
            .get(format!("{}/source/{source_id}/preferences", self.base_url))
            .send()
            .await
            .map_err(DomainError::from)?;
        Self::preferences_body(r).await
    }

    /// 按位置写回一个设置值，返回写回后的设置界面 JSON。
    pub async fn set_source_preference(&self, source_id: i64, position: i32, value: &str) -> Result<Option<String>> {
        let r = self
            .client
            .post(format!("{}/source/{source_id}/preferences", self.base_url))
            .json(&serde_json::json!({ "position": position, "value": value }))
            .send()
            .await
            .map_err(DomainError::from)?;
        Self::preferences_body(r).await
    }

    async fn preferences_body(r: reqwest::Response) -> Result<Option<String>> {
        if r.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !r.status().is_success() {
            return Err(sandbox_error(r).await);
        }
        let v: serde_json::Value = r.json().await.map_err(DomainError::from)?;
        Ok(Some(v.get("preferences").map_or_else(|| "[]".to_string(), std::string::ToString::to_string)))
    }

    /// 源的扁平 key/value（`GET /source/{id}/preferences/raw`），备份 105 号段用。
    ///
    /// 与 [`Self::source_preferences`] 的区别：那个给的是「设置界面」（标题、选项、
    /// 可见性），这个给的是「扩展自己存了什么」——文件格式要的是后者。
    ///
    /// `None` = 该源没有设置界面（沙盒回 404），不是错误。
    pub async fn source_preference_values(&self, source_id: i64) -> Result<Option<Vec<SandboxPreference>>> {
        let r = self
            .client
            .get(format!("{}/source/{source_id}/preferences/raw", self.base_url))
            .send()
            .await
            .map_err(DomainError::from)?;
        Self::raw_preferences_body(r).await
    }

    /// 把扁平 key/value 写回扩展自己的存储，返回写回后的结果。
    pub async fn write_source_preference_values(
        &self,
        source_id: i64,
        prefs: &[SandboxPreference],
    ) -> Result<Option<Vec<SandboxPreference>>> {
        let r = self
            .client
            .post(format!("{}/source/{source_id}/preferences/raw", self.base_url))
            .json(&serde_json::json!({ "preferences": prefs }))
            .send()
            .await
            .map_err(DomainError::from)?;
        Self::raw_preferences_body(r).await
    }

    async fn raw_preferences_body(r: reqwest::Response) -> Result<Option<Vec<SandboxPreference>>> {
        #[derive(Deserialize)]
        struct RawPreferences {
            #[serde(default)]
            preferences: Vec<SandboxPreference>,
        }

        if r.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !r.status().is_success() {
            return Err(sandbox_error(r).await);
        }

        let v: RawPreferences = r.json().await.map_err(DomainError::from)?;
        Ok(Some(v.preferences))
    }

    /// Parses an uploaded APK (raw bytes) and returns its extension metadata.
    pub async fn inspect(&self, apk: &[u8]) -> Result<SandboxExtension> {
        let r = self
            .client
            .post(format!("{}/inspect", self.base_url))
            .body(apk.to_vec())
            .send()
            .await
            .map_err(DomainError::from)?;
        if !r.status().is_success() {
            return Err(DomainError::Sandbox(format!("sandbox inspect failed: {}", r.status())));
        }
        r.json::<SandboxExtension>().await.map_err(DomainError::from)
    }

    /// 扩展 **APK 里**那张图标（PNG 字节）。
    ///
    /// 端不认这个路由（旧沙盒）、包没装、取不出来 —— 一律 `None`，调用方自己兜底。
    pub async fn icon(&self, pkg_name: &str) -> Option<Vec<u8>> {
        use base64::Engine as _;

        #[derive(serde::Deserialize)]
        struct SandboxIcon {
            #[serde(default)]
            data: String,
        }

        let r = self.client.get(format!("{}/icon/{pkg_name}", self.base_url)).send().await.ok()?;
        if !r.status().is_success() {
            return None;
        }
        let payload = r.json::<SandboxIcon>().await.ok()?;
        if payload.data.is_empty() {
            return None;
        }

        let bytes = base64::engine::general_purpose::STANDARD.decode(payload.data).ok()?;
        // 回环另一端回 200 却带着一段正文（比如参考实现 404 页面）并不罕见，认一下魔数。
        is_image(&bytes).then_some(bytes)
    }

    async fn fetch_mangas_page(&self, source_id: i64, params: &[(&str, String)]) -> Result<MangasPage> {
        let url = format!("{}/source/{source_id}/manga", self.base_url);
        let resp = self.client.get(&url).query(params).send().await.map_err(DomainError::from)?;
        if !resp.status().is_success() {
            return Err(sandbox_error(resp).await);
        }
        let page: SandboxMangasPage = resp.json().await.map_err(DomainError::from)?;
        let mangas = page
            .mangas
            .into_iter()
            .map(|m| SManga {
                url: m.url,
                title: m.title,
                thumbnail_url: m.thumbnail_url,
                artist: m.artist,
                author: m.author,
                status: m.status,
                description: m.description,
                genre: m.genre,
                alt_titles: Vec::new(),
                update_strategy: suwayomi_core::models::UpdateStrategy::AlwaysUpdate,
                initialized: false,
                memo: serde_json::Value::Null,
            })
            .collect();
        Ok(MangasPage { mangas, has_next_page: page.has_next_page })
    }
}

#[async_trait]
impl SourceFetcher for HttpSandboxFetcher {
    async fn fetch_manga_update(
        &self,
        source_id: i64,
        manga: &SManga,
        _chapters: &[SChapter],
        fetch_details: bool,
        fetch_chapters: bool,
    ) -> Result<(SManga, Vec<SChapter>)> {
        let url = format!("{}/source/{source_id}/manga/{}", self.base_url, urlencode(&manga.url));
        let mut updated = manga.clone();
        if fetch_details {
            let resp = self.client.get(&url).send().await.map_err(DomainError::from)?;
            if resp.status().is_success()
                && let Ok(m) = resp.json::<SandboxManga>().await
            {
                updated.title = if m.title.is_empty() { manga.title.clone() } else { m.title };
                updated.thumbnail_url = m.thumbnail_url.or_else(|| manga.thumbnail_url.clone());
                updated.author = m.author.or_else(|| manga.author.clone());
                updated.artist = m.artist.or_else(|| manga.artist.clone());
                updated.description = m.description.or_else(|| manga.description.clone());
                if m.status != 0 {
                    updated.status = m.status;
                }
                // The sandbox returns `genre` as a ", "-joined string
                // (mangaToMap). Without this copy, every online manga
                // ended up with an empty genre column in the DB even
                // after the detail page re-fetched the source.
                if let Some(g) = m.genre
                    && !g.is_empty()
                {
                    updated.genre = Some(g);
                }
            }
        }
        let mut chapters_out = Vec::new();
        if fetch_chapters {
            let resp = self.client.get(format!("{url}/chapters")).send().await.map_err(DomainError::from)?;
            if resp.status().is_success()
                && let Ok(cs) = resp.json::<SandboxChapters>().await
            {
                chapters_out = cs
                    .chapters
                    .into_iter()
                    .map(|c| SChapter {
                        url: c.url,
                        name: c.name,
                        date_upload: c.date_upload,
                        chapter_number: c.chapter_number,
                        scanlator: c.scanlator,
                        memo: serde_json::Value::Null,
                    })
                    .collect();
            }
        }
        Ok((updated, chapters_out))
    }

    async fn get_popular_manga(&self, source_id: i64, page: u32) -> Result<MangasPage> {
        self.fetch_mangas_page(source_id, &[("page", page.to_string())]).await
    }

    async fn get_latest_updates(&self, source_id: i64, page: u32) -> Result<MangasPage> {
        self.fetch_mangas_page(source_id, &[("page", page.to_string()), ("mode", "latest".into())]).await
    }

    async fn search_manga(&self, source_id: i64, query: &str, page: u32) -> Result<MangasPage> {
        self.fetch_mangas_page(source_id, &[("page", page.to_string()), ("query", query.to_string())]).await
    }

    fn supports_latest(&self, _source_id: i64) -> bool {
        true // extensions support latest unless the source overrides it
    }

    async fn get_filters(&self, source_id: i64) -> Result<serde_json::Value> {
        let r = self
            .client
            .get(format!("{}/source/{source_id}/filters", self.base_url))
            .send()
            .await
            .map_err(DomainError::from)?;
        if !r.status().is_success() {
            return Err(sandbox_error(r).await);
        }
        let json: serde_json::Value = r.json().await.map_err(DomainError::from)?;
        // /source/{id}/filters 返回 {"filters":[...]}；兼容纯数组
        Ok(json.get("filters").cloned().unwrap_or_else(|| if json.is_array() { json } else { serde_json::json!([]) }))
    }

    async fn fetch_pages(
        &self,
        source_id: i64,
        manga_url: &str,
        chapter_url: &str,
    ) -> Result<Vec<suwayomi_core::source::SourcePage>> {
        let cenc = urlencode(chapter_url);
        let menc = urlencode(manga_url);
        let url = format!("{}/source/{source_id}/chapter/{cenc}/pages", self.base_url);
        let resp = self.client.get(&url).query(&[("mangaUrl", menc)]).send().await.map_err(DomainError::from)?;
        if !resp.status().is_success() {
            return Err(sandbox_error(resp).await);
        }
        let pages: SandboxPages = resp.json().await.map_err(DomainError::from)?;
        Ok(pages
            .pages
            .into_iter()
            .map(|p| suwayomi_core::source::SourcePage {
                index: p.index,
                url: p.image_url.clone().unwrap_or(p.url),
                image_url: p.image_url,
                uri: None,
            })
            .collect())
    }
}

/// `requested` 上探多少个端口找空位；只用来躲开占用，不做全端口扫描。
const SANDBOX_PORT_TRIES: u16 = 32;

fn port_bindable(port: u16) -> bool {
    std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_ok()
}

/// 沙盒实际可用的端口：先清掉上一次运行残留的沙盒 JVM，清完还是绑不上就向上顺延。
/// 「清完仍绑不上」意味着占用者不是残留沙盒，而是 server 自己 —— 它的监听端口自顺延时
/// 可能正好落到 `SUWAYOMI_SANDBOX_PORT` 上，那种情况下顺延是唯一的出路。
async fn available_port(requested: u16) -> u16 {
    if port_bindable(requested) {
        return requested;
    }
    tracing::warn!(port = requested, "sandbox port already in use; killing stale listener");
    kill_port_listener(requested);
    for _ in 0..20 {
        if port_bindable(requested) {
            return requested;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let fallback =
        (requested.saturating_add(1)..=requested.saturating_add(SANDBOX_PORT_TRIES)).find(|p| port_bindable(*p));
    // 一个空位都没有：原样返回，交给 spawn 去失败 —— 健康检查会报出「沙盒没起来」，
    // 好过绑到别人的端口上。
    fallback.map_or(requested, |p| {
        tracing::warn!(requested, port = p, "sandbox port still taken; using another one");
        p
    })
}

/// Best-effort: terminate whatever process is LISTENING on `port`.
/// The single-instance mutex guarantees any listener is our own stale JVM — except
/// when the port is the one our own server ended up on after falling back; killing
/// that would take the server down with it.
#[cfg(windows)]
fn kill_port_listener(port: u16) {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    let out = Command::new("netstat").args(["-ano"]).output().ok();
    if let Some(out) = out {
        let own_pid = std::process::id().to_string();
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let l = line.to_ascii_lowercase();
            if l.contains(&format!(":{port}"))
                && l.contains("listening")
                && let Some(pid) = line.split_whitespace().last()
                && pid != own_pid
            {
                let _ = Command::new("taskkill").args(["/F", "/PID", pid]).creation_flags(0x0800_0000).output();
            }
        }
    }
}

#[cfg(not(windows))]
fn kill_port_listener(_port: u16) {}

/// `java` on Windows is `java.exe` — probe both spellings.
fn java_bin(root: &std::path::Path) -> Option<std::path::PathBuf> {
    let bin = root.join("bin").join("java");
    if bin.is_file() {
        return Some(bin);
    }
    let exe = bin.with_extension("exe");
    if exe.is_file() {
        return Some(exe);
    }
    None
}

/// Resolves the `java` executable for the sandbox, in priority order:
/// 1. `SUWAYOMI_JAVA` env — explicit override (e.g. a specific JDK).
/// 2. Bundled Temurin JRE 25 shipped in the release layout as `<root>/jre/`
///    (next to the executable, or one level up from `bin/` where the sandbox
///    jar lives) — makes the desktop zip fully self-contained, no system JDK.
/// 3. `JAVA_HOME/bin/java`.
/// 4. `java` on PATH.
fn resolve_java(jar_path: &str) -> std::path::PathBuf {
    // 1) explicit env override
    if let Ok(java) = std::env::var("SUWAYOMI_JAVA")
        && !java.is_empty()
    {
        let p = std::path::PathBuf::from(&java);
        if p.is_file() || p.with_extension("exe").is_file() {
            return p;
        }
    }
    // 2) bundled jre — jre/ sits at the release root, i.e. either next to the
    //    executable or one level up from `bin/` (where exe/jar live).
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        roots.push(dir.to_path_buf()); // exe at release root
        if let Some(p) = dir.parent() {
            roots.push(p.to_path_buf()); // exe inside bin/ → root is one up
        }
    }
    let jar = std::path::Path::new(jar_path);
    if let Some(dir) = jar.parent() {
        roots.push(dir.to_path_buf()); // jar at release root
        if let Some(p) = dir.parent() {
            roots.push(p.to_path_buf()); // jar inside bin/ → root is one up
        }
    }
    for r in &roots {
        if let Some(p) = java_bin(&r.join("jre")) {
            return p;
        }
    }
    // 3) JAVA_HOME
    if let Ok(jh) = std::env::var("JAVA_HOME")
        && let Some(p) = java_bin(std::path::Path::new(&jh))
    {
        return p;
    }
    // 4) PATH
    std::path::PathBuf::from("java")
}

/// Builds the `java -jar` command for the sandbox (no-window on Windows,
/// JVM output redirected into `<appdata>/logs/sandbox.log`).
///
/// 子进程只拿一个目录旋钮 `SUWAYOMI_APPDATA_DIR`，扩展目录 / dex2jar 产物目录 /
/// 设置目录由 ext-runtime 自己按同一套子路径派生（见 Main.kt）—— 两边必须落在
/// 同一份根上，否则 WebUI 里配的源偏好在扩展侧读不到。
fn spawn_java(jar_path: &str, port: &str, appdata: &std::path::Path) -> std::io::Result<std::process::Child> {
    let java = resolve_java(jar_path);
    let mut cmd = std::process::Command::new(java);
    // JVM 默认不走代理（java.net.useSystemProxies=false）——用户本地 Clash 等
    // 设置了系统代理时扩展请求仍直连外网而失败。显式开启系统代理；显式
    // SUWAYOMI_SANDBOX_PROXY 仍优先（NetworkHelper 的 builder.proxy 覆盖）。
    cmd.arg("-Djava.net.useSystemProxies=true").arg("-jar").arg(jar_path).env("SUWAYOMI_SANDBOX_PORT", port);
    cmd.env(suwayomi_core::config::APPDATA_DIR_ENV, appdata);
    if let Ok(proxy) = std::env::var("SUWAYOMI_SANDBOX_PROXY") {
        cmd.env("SUWAYOMI_SANDBOX_PROXY", proxy);
    }
    // 扩展通过 `eu.kanade.tachiyomi.AppInfo` 读宿主版本并拼进 User-Agent；沙盒是独立
    // 进程，拿不到 server 的编译期常量，只能在这里传进去（与 build.rs 注入的同名变量
    // 一致，来源是同一个 `suwayomi_core::version`）。
    cmd.env("SUWAYOMI_VERSION_NAME", suwayomi_core::version::VERSION);
    cmd.env("SUWAYOMI_VERSION_CODE", suwayomi_core::version::VERSION_CODE);
    // Windows：server 自身无控制台（windows_subsystem=windows），spawn 的
    // java 是 console 程序，默认会新建一个终端窗口——用 CREATE_NO_WINDOW
    // 静默启动，并把 JVM 输出落到日志目录便于诊断。
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let logs = suwayomi_core::config::appdata_logs(appdata);
    let _ = std::fs::create_dir_all(&logs);
    let stdio = std::fs::OpenOptions::new().create(true).append(true).open(logs.join("sandbox.log")).ok();
    match stdio {
        Some(f) => {
            let clone = f.try_clone().ok();
            cmd.stdout(std::process::Stdio::from(f));
            if let Some(c) = clone {
                cmd.stderr(std::process::Stdio::from(c));
            }
        }
        None => {
            cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        }
    }
    cmd.spawn()
}

/// Owns the sandbox JVM process: spawns `java -jar`, waits for health, and
/// hands out an `HttpSandboxFetcher`. A background monitor restarts the JVM
/// whenever it dies (crash / OOM / kill) — the HTTP fetch base URL stays the
/// same (same port), so existing fetchers keep working after a restart.
/// Dropping the guard stops the child and the monitor.
pub struct SandboxProcess {
    child: std::sync::Arc<std::sync::Mutex<Option<std::process::Child>>>,
    fetcher: HttpSandboxFetcher,
    monitor: tokio::task::JoinHandle<()>,
}

impl SandboxProcess {
    /// `appdata` 与 server 共用同一个根（见 `AppPaths::appdata`）：沙盒据此自行
    /// 派生扩展目录 / dex2jar 产物目录 / 设置目录（沙盒侧不派生 `logs`，那个日志
    /// 文件是本进程重定向它的 stdout/stderr 写出来的）。
    pub async fn start(jar_path: &str, port: &str, appdata: &std::path::Path) -> Result<Self> {
        let port = available_port(port.parse().unwrap_or(DEFAULT_SANDBOX_PORT)).await.to_string();
        let mut child =
            spawn_java(jar_path, &port, appdata).map_err(|e| DomainError::Sandbox(format!("spawn sandbox: {e}")))?;
        let base = format!("http://127.0.0.1:{port}");
        let fetcher = HttpSandboxFetcher::new(base.clone());
        // wait for health with retries (up to ~15s); bail if OUR child died
        // (e.g. bind failed) instead of accepting a stale instance's health.
        let mut healthy = false;
        for _ in 0..50 {
            if fetcher.health().await && child.try_wait().ok().flatten().is_none() {
                healthy = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        if !healthy {
            let _ = fetch_child_kill(&mut Some(child));
            return Err(DomainError::Sandbox(format!("sandbox did not become healthy on {base}")));
        }
        let child = std::sync::Arc::new(std::sync::Mutex::new(Some(child)));
        let monitor =
            Self::spawn_monitor(child.clone(), fetcher.clone(), jar_path.to_string(), port.to_string(), appdata);
        Ok(Self { child, fetcher, monitor })
    }

    /// Background watchdog: health-check every 10s; after 2 consecutive
    /// failures kill the JVM, wait for the port to free, respawn and re-check.
    fn spawn_monitor(
        child: std::sync::Arc<std::sync::Mutex<Option<std::process::Child>>>,
        fetcher: HttpSandboxFetcher,
        jar: String,
        port: String,
        appdata: &std::path::Path,
    ) -> tokio::task::JoinHandle<()> {
        // 重启也要用同一个 appdata 根：子进程环境按原样重建
        let appdata = appdata.to_path_buf();
        tokio::spawn(async move {
            let mut fails: u32 = 0;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                let ok = fetcher.health().await;
                fails = if ok { 0 } else { fails + 1 };
                if fails < 2 {
                    continue;
                }
                tracing::warn!("jvm sandbox lost (health failed {fails} consecutive times); restarting on port {port}");
                // kill the old JVM so the port is released
                {
                    // 锁中毒只是说明上一个持锁线程 panic 过，`Option<Child>` 仍然可读；
                    // 这里要的是"把残留 JVM 杀掉"，中毒不该让监控任务先崩一步。
                    let mut guard = child.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    let _ = fetch_child_kill(&mut guard);
                }
                // wait for the port to free up (bind probe)
                let port_num = port.parse::<u16>().unwrap_or(DEFAULT_SANDBOX_PORT);
                for _ in 0..10 {
                    if std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port_num)).is_ok() {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                // respawn and wait for health
                match spawn_java(&jar, &port, &appdata) {
                    Ok(c) => {
                        let mut ok = false;
                        for _ in 0..50 {
                            if fetcher.health().await {
                                ok = true;
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                        }
                        if ok {
                            *child.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(c);
                            tracing::info!("jvm sandbox restarted on port {port}");
                        } else {
                            tracing::warn!("jvm sandbox restart failed to become healthy");
                            let _ = fetch_child_kill(&mut Some(c));
                        }
                    }
                    Err(e) => tracing::warn!("jvm sandbox respawn failed: {e}"),
                }
                fails = 0;
            }
        })
    }

    pub fn fetcher(&self) -> HttpSandboxFetcher {
        HttpSandboxFetcher::new(self.fetcher.base_url())
    }
}

fn fetch_child_kill(child: &mut Option<std::process::Child>) -> std::io::Result<()> {
    // `take()` 一次拿到所有权并把槽位清空：比"先 as_mut 借用、再回头写槽位"
    // 少一次借用协商，也让 `None` 分支变成 `map_or` 的默认值。
    child.take().map_or(Ok(()), |mut c| {
        let r = c.kill();
        let _ = c.wait();
        r
    })
}

impl Drop for SandboxProcess {
    fn drop(&mut self) {
        self.monitor.abort();
        // `Drop` 里更不能 panic：中毒时也要把子进程收干净，否则 JVM 会变成孤儿进程。
        let mut guard = self.child.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = fetch_child_kill(&mut guard);
    }
}

/// 沙盒回非 2xx 时的错误，取出来就是能给用户看的一句话。
///
/// 沙盒的错误体是 `{"error": <一行>, "stack": <栈>}`（见 ext-runtime 共享源码的
/// `Errors.kt`）：`error` 已经是扩展自己那句文案（如哔咔的
/// `IOException: 请在扩展设置界面输入用户名和密码`），`stack` 有几十行 `at …`。
/// 两者都往界面上塞就没人看，所以这里只取 `error`，栈由沙盒自己写进日志。
/// 取不到 `error`（回环上挂了别的进程、代理插了一页 HTML）就退化成截断的原文。
async fn sandbox_error(resp: reqwest::Response) -> DomainError {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_owned))
        .unwrap_or_else(|| body.trim().chars().take(200).collect());
    if msg.is_empty() { DomainError::Source(format!("sandbox error {status}")) } else { DomainError::Source(msg) }
}

/// 按魔数认 PNG / JPEG / WebP。
fn is_image(bytes: &[u8]) -> bool {
    let png = bytes.starts_with(b"\x89PNG");
    let jpeg = bytes.starts_with(b"\xff\xd8");
    let webp = bytes.starts_with(b"RIFF");
    png || jpeg || webp
}
