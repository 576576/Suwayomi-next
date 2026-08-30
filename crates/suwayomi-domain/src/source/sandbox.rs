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

/// Owns the sandbox JVM process: spawns `java -jar`, waits for health,
/// and hands out an `HttpSandboxFetcher`. Dropping the guard stops the child.
pub struct SandboxProcess {
    child: Option<std::process::Child>,
    fetcher: HttpSandboxFetcher,
}

impl SandboxProcess {
    pub async fn start(jar_path: &str, port: &str) -> Result<Self> {
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
        cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let child = cmd.spawn().map_err(|e| DomainError::Sandbox(format!("spawn sandbox: {e}")))?;
        let base = format!("http://127.0.0.1:{port}");
        let fetcher = HttpSandboxFetcher::new(base.clone());
        // wait for health with retries (up to ~15s)
        for _ in 0..50 {
            if fetcher.health().await {
                return Ok(Self { child: Some(child), fetcher });
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        Err(DomainError::Sandbox(format!("sandbox did not become healthy on {base}")))
    }

    pub fn fetcher(&self) -> HttpSandboxFetcher {
        HttpSandboxFetcher::new(self.fetcher.base_url())
    }
}

impl Drop for SandboxProcess {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
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
