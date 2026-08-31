//! HTTP client for the JVM extension sandbox (`jvm-sandbox/`).
//!
//! `HttpSandboxFetcher` implements `SourceFetcher` by calling the sandbox
//! process over its stable HTTP/JSON contract. The sandbox is started and
//! supervised by the server; this type only needs its base URL.

use async_trait::async_trait;
use serde::Deserialize;

use suwayomi_core::source::{MangasPage, SChapter, SManga};

use crate::error::{DomainError, Result};
use crate::source::SourceFetcher;

/// A source described by the sandbox (used for registration/debug).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxSourceInfo {
    pub id: i64,
    pub name: String,
    pub lang: String,
    pub extension: i32,
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
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                // 本地回环绝不走代理：reqwest 默认读取 HTTP_PROXY/HTTPS_PROXY 等
                // 环境变量（Clash 常设置），会把 127.0.0.1:8091 也转发到代理，
                // 代理无法连接该端口返回 502 Bad Gateway（install reload 失败）。
                .no_proxy()
                .build()
                .expect("reqwest client"),
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
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Lists extensions known to the sandbox (Phase 5 skeleton).
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

    async fn fetch_mangas_page(&self, source_id: i64, params: &[(&str, String)]) -> Result<MangasPage> {
        let url = format!("{}/source/{source_id}/manga", self.base_url);
        let resp = self.client.get(&url).query(params).send().await.map_err(DomainError::from)?;
        if !resp.status().is_success() {
            return Err(DomainError::Source(format!(
                "sandbox error {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
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
            if resp.status().is_success() {
                if let Ok(m) = resp.json::<SandboxManga>().await {
                    updated.title = if m.title.is_empty() { manga.title.clone() } else { m.title };
                    updated.thumbnail_url = m.thumbnail_url.or_else(|| manga.thumbnail_url.clone());
                    updated.author = m.author.or_else(|| manga.author.clone());
                    updated.artist = m.artist.or_else(|| manga.artist.clone());
                    updated.description = m.description.or_else(|| manga.description.clone());
                    if m.status != 0 {
                        updated.status = m.status;
                    }
                }
            }
        }
        let mut chapters_out = Vec::new();
        if fetch_chapters {
            let resp = self.client.get(format!("{url}/chapters")).send().await.map_err(DomainError::from)?;
            if resp.status().is_success() {
                if let Ok(cs) = resp.json::<SandboxChapters>().await {
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
            return Err(DomainError::Sandbox(format!("sandbox filters failed: {}", r.status())));
        }
        let json: serde_json::Value = r.json().await.map_err(DomainError::from)?;
        // /source/{id}/filters 返回 {"filters":[...]}；兼容纯数组
        Ok(json
            .get("filters")
            .cloned()
            .unwrap_or_else(|| if json.is_array() { json } else { serde_json::json!([]) }))
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
            return Err(DomainError::Source(format!(
                "sandbox error {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
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

/// Builds the `java -jar` command for the sandbox (no-window on Windows,
/// JVM output redirected into `logs/sandbox.log` when `SUWAYOMI_LOGS_DIR` is set).
fn spawn_java(jar_path: &str, port: &str) -> std::io::Result<std::process::Child> {
    let java = match std::env::var("JAVA_HOME") {
        Ok(jh) => {
            let p = std::path::Path::new(&jh).join("bin").join("java");
            if p.exists() {
                p
            } else {
                std::path::PathBuf::from("java")
            }
        }
        Err(_) => std::path::PathBuf::from("java"),
    };
    let mut cmd = std::process::Command::new(java);
    cmd.arg("-jar").arg(jar_path).env("SUWAYOMI_SANDBOX_PORT", port);
    // Pass through the extensions directory (default ./extensions) and an
    // optional outbound proxy (e.g. Clash) for geo-blocked sources.
    if let Ok(dir) = std::env::var("SUWAYOMI_EXTENSIONS_DIR") {
        cmd.env("SUWAYOMI_EXTENSIONS_DIR", dir);
    }
    if let Ok(proxy) = std::env::var("SUWAYOMI_SANDBOX_PROXY") {
        cmd.env("SUWAYOMI_SANDBOX_PROXY", proxy);
    }
    // Windows：server 自身无控制台（windows_subsystem=windows），spawn 的
    // java 是 console 程序，默认会新建一个终端窗口——用 CREATE_NO_WINDOW
    // 静默启动，并把 JVM 输出落到日志目录便于诊断。
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let stdio = match std::env::var("SUWAYOMI_LOGS_DIR") {
        Ok(dir) => {
            let dir = std::path::PathBuf::from(dir);
            let _ = std::fs::create_dir_all(&dir);
            std::fs::OpenOptions::new().create(true).append(true).open(dir.join("sandbox.log")).ok()
        }
        Err(_) => None,
    };
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
    pub async fn start(jar_path: &str, port: &str) -> Result<Self> {
        let child = spawn_java(jar_path, port)
            .map_err(|e| DomainError::Sandbox(format!("spawn sandbox: {e}")))?;
        let base = format!("http://127.0.0.1:{port}");
        let fetcher = HttpSandboxFetcher::new(base.clone());
        // wait for health with retries (up to ~15s)
        let mut healthy = false;
        for _ in 0..50 {
            if fetcher.health().await {
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
        let monitor = Self::spawn_monitor(child.clone(), fetcher.clone(), jar_path.to_string(), port.to_string());
        Ok(Self { child, fetcher, monitor })
    }

    /// Background watchdog: health-check every 10s; after 2 consecutive
    /// failures kill the JVM, wait for the port to free, respawn and re-check.
    fn spawn_monitor(
        child: std::sync::Arc<std::sync::Mutex<Option<std::process::Child>>>,
        fetcher: HttpSandboxFetcher,
        jar: String,
        port: String,
    ) -> tokio::task::JoinHandle<()> {
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
                    let mut guard = child.lock().unwrap();
                    let _ = fetch_child_kill(&mut guard);
                }
                // wait for the port to free up (bind probe)
                let port_num = port.parse::<u16>().unwrap_or(8091);
                for _ in 0..10 {
                    if std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port_num)).is_ok() {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                // respawn and wait for health
                match spawn_java(&jar, &port) {
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
                            *child.lock().unwrap() = Some(c);
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
    if let Some(c) = child.as_mut() {
        let r = c.kill();
        let _ = c.wait();
        *child = None;
        r
    } else {
        Ok(())
    }
}

impl Drop for SandboxProcess {
    fn drop(&mut self) {
        self.monitor.abort();
        let mut guard = self.child.lock().unwrap();
        let _ = fetch_child_kill(&mut guard);
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
