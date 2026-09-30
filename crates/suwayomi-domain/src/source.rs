//! Source abstraction for the domain layer.
//!
//! Mirrors `eu.kanade.tachiyomi.source.Source` + the `GetSource` helpers.
//! `SourceFetcher` is the seam where the JVM sandbox plugs in.
//! `LocalSource` semantics (ID, file handling) are reserved here.

pub mod local;
pub mod sandbox;

use async_trait::async_trait;
use suwayomi_core::source::{MangasPage, SChapter, SManga};

/// The local source id, aligned with the WebUI client constant
/// `Sources.LOCAL_SOURCE_ID = '0'` (r3474). Older Tachiyomi builds used -1;
/// the modern Suwayomi WebUI sends `'0'` for every local-source call
/// (`fetchSourceManga(source: '0')`, `fetchMangaAndChapters(sourceId: '0')`).
pub const LOCAL_SOURCE_ID: i64 = 0;

/// Mirrors `Source` / `HttpSource` capabilities needed by the domain layer.
#[async_trait]
pub trait SourceFetcher: Send + Sync {
    /// fetchMangaAndChapters: fetch details (and/or chapters) from the source
    async fn fetch_manga_update(
        &self,
        source_id: i64,
        manga: &SManga,
        chapters: &[SChapter],
        fetch_details: bool,
        fetch_chapters: bool,
    ) -> crate::error::Result<(SManga, Vec<SChapter>)>;

    /// Popular / latest pagination (used by MangaList & Search)
    async fn get_popular_manga(&self, source_id: i64, page: u32) -> crate::error::Result<MangasPage>;
    async fn get_latest_updates(&self, source_id: i64, page: u32) -> crate::error::Result<MangasPage>;
    async fn search_manga(&self, source_id: i64, query: &str, page: u32) -> crate::error::Result<MangasPage>;

    /// Page list for a chapter (used by the download manager). The default
    /// returns an error; the sandbox backend overrides it.
    async fn fetch_pages(
        &self,
        _source_id: i64,
        _manga_url: &str,
        _chapter_url: &str,
    ) -> crate::error::Result<Vec<suwayomi_core::source::SourcePage>> {
        Err(crate::error::DomainError::Source("fetch_pages not implemented".into()))
    }

    /// Whether the source provides a latest listing
    fn supports_latest(&self, source_id: i64) -> bool;

    /// Search filters for a source. Defaults to an empty list;
    /// the JVM sandbox backend forwards `/source/{id}/filters`.
    async fn get_filters(&self, _source_id: i64) -> crate::error::Result<serde_json::Value> {
        Ok(serde_json::json!([]))
    }
}

/// Default stub used when no sandbox backend is installed.
/// Matches `StubSource` behavior: fetching fails with a descriptive error.
#[derive(Default)]
pub struct StubFetcher;

#[async_trait]
impl SourceFetcher for StubFetcher {
    async fn fetch_manga_update(
        &self,
        _source_id: i64,
        _manga: &SManga,
        _chapters: &[SChapter],
        _fetch_details: bool,
        _fetch_chapters: bool,
    ) -> crate::error::Result<(SManga, Vec<SChapter>)> {
        Err(crate::error::DomainError::Source("source unavailable: extension sandbox not connected".into()))
    }

    async fn get_popular_manga(&self, _source_id: i64, _page: u32) -> crate::error::Result<MangasPage> {
        Err(crate::error::DomainError::Source("source unavailable: extension sandbox not connected".into()))
    }

    async fn get_latest_updates(&self, _source_id: i64, _page: u32) -> crate::error::Result<MangasPage> {
        Err(crate::error::DomainError::Source("source unavailable: extension sandbox not connected".into()))
    }

    async fn search_manga(&self, _source_id: i64, _query: &str, _page: u32) -> crate::error::Result<MangasPage> {
        Err(crate::error::DomainError::Source("source unavailable: extension sandbox not connected".into()))
    }

    fn supports_latest(&self, _source_id: i64) -> bool {
        false
    }
}

/// 生产环境的源抓取后端。
///
/// `SourceFetcher` 是**契约**（各方法的语义在这里定义），但生产代码不再用它做
/// 动态派发：全树只有两种后端——没有沙箱时的 stub、以及 JVM 沙箱——把它们写成
/// 一个封闭枚举之后，(a) 调用点不再为"以后可能出现的第三种后端"付 `dyn` 的
/// 间接跳转，(b) 编译器能把 `match` 展开、把热路径（更新器每章一次抓取、
/// 下载器每页一次抓取）内联，(c) `match` 的穷尽性检查会拦住漏掉的分支。
///
/// `HttpSandboxFetcher` 本身是 `Clone` 且内部只有 `String` + `reqwest::Client`
/// （后者内部已是 `Arc`），所以这个枚举可以按值到处传，不必再包一层 `Arc`。
#[derive(Clone, Default)]
pub enum SourceBackend {
    /// 没接扩展：一切抓取都以描述性错误失败（`StubSource` 的语义）。
    #[default]
    Stub,
    /// ext-runtime JVM 沙箱（内嵌进程或外部主机）。
    Sandbox(sandbox::HttpSandboxFetcher),
    /// 测试注入点。
    ///
    /// 只在 `cfg(test)` 下存在，因此生产构建里这个枚举仍然是封闭的两态，
    /// 派发也仍然是静态的；集成测试（`tests/*.rs`）走的是 `Stub`。
    #[cfg(test)]
    Test(std::sync::Arc<dyn SourceFetcher>),
}

impl SourceBackend {
    /// 沙箱的 base url，没接沙箱时为 `None`。
    pub fn sandbox_base(&self) -> Option<&str> {
        match self {
            Self::Sandbox(f) => Some(f.base_url()),
            Self::Stub => None,
            #[cfg(test)]
            Self::Test(_) => None,
        }
    }

    /// 沙盒里各源的扁平设置，供备份写进 105 号段。
    ///
    /// 只问「可配置」的源（`/sources` 里的 `isConfigurable`），源不可配置或没接沙盒
    /// 时为空 —— 调用方据此得到空节，而不是一堆空壳条目。某个源读失败只跳过它：
    /// 一个扩展抛异常不该让整份备份失败。
    pub async fn backup_source_preferences(&self) -> Vec<suwayomi_core::backup::BackupSourcePreferences> {
        let Self::Sandbox(fetcher) = self else {
            return Vec::new();
        };
        let Ok(sources) = fetcher.list_sources().await else {
            tracing::warn!("sandbox: cannot list sources for source preferences backup");
            return Vec::new();
        };
        let mut out = Vec::new();
        for source in sources.into_iter().filter(|s| s.is_configurable) {
            match fetcher.source_preference_values(source.id).await {
                Ok(Some(values)) if !values.is_empty() => out.push(suwayomi_core::backup::BackupSourcePreferences {
                    source_key: source_preference_key(source.id),
                    prefs: values.iter().filter_map(sandbox::SandboxPreference::to_backup_preference).collect(),
                }),
                Ok(_) => {}
                // 404 是常态：备份里的扩展这台机器可能没装。
                Err(e) => tracing::debug!(source = source.id, %e, "sandbox: cannot read source preferences"),
            }
        }
        out
    }

    /// 把备份里的图源设置（105 号段）写回沙盒。没接沙盒时什么都不做。
    pub async fn apply_source_preferences(&self, groups: &[suwayomi_core::backup::BackupSourcePreferences]) {
        let Self::Sandbox(fetcher) = self else {
            return;
        };
        for group in groups {
            let Some(source_id) = parse_source_preference_key(&group.source_key) else {
                continue;
            };
            let prefs: Vec<sandbox::SandboxPreference> =
                group.prefs.iter().filter_map(sandbox::SandboxPreference::from_backup_preference).collect();
            if prefs.is_empty() {
                continue;
            }
            match fetcher.write_source_preference_values(source_id, &prefs).await {
                Ok(_) => tracing::debug!(source = source_id, count = prefs.len(), "source preferences restored"),
                // 同样按常态处理：备份来自另一台机器，本机没有这个源就没有地方写。
                Err(e) => tracing::debug!(source = source_id, %e, "sandbox: cannot write source preferences"),
            }
        }
    }
}

/// 105 号段里的 `sourceKey`，与扩展自己的 `ConfigurableSource.preferenceKey()` 一致。
fn source_preference_key(source_id: i64) -> String {
    format!("source_{source_id}")
}

/// `source_<id>` → id；别的写法（别的客户端自定义的 key）一律忽略。
fn parse_source_preference_key(key: &str) -> Option<i64> {
    key.strip_prefix("source_")?.parse().ok()
}

impl From<sandbox::HttpSandboxFetcher> for SourceBackend {
    fn from(fetcher: sandbox::HttpSandboxFetcher) -> Self {
        Self::Sandbox(fetcher)
    }
}

#[async_trait]
impl SourceFetcher for SourceBackend {
    async fn fetch_manga_update(
        &self,
        source_id: i64,
        manga: &SManga,
        chapters: &[SChapter],
        fetch_details: bool,
        fetch_chapters: bool,
    ) -> crate::error::Result<(SManga, Vec<SChapter>)> {
        match self {
            Self::Stub => {
                StubFetcher.fetch_manga_update(source_id, manga, chapters, fetch_details, fetch_chapters).await
            }
            Self::Sandbox(f) => f.fetch_manga_update(source_id, manga, chapters, fetch_details, fetch_chapters).await,
            #[cfg(test)]
            Self::Test(f) => f.fetch_manga_update(source_id, manga, chapters, fetch_details, fetch_chapters).await,
        }
    }

    async fn get_popular_manga(&self, source_id: i64, page: u32) -> crate::error::Result<MangasPage> {
        match self {
            Self::Stub => StubFetcher.get_popular_manga(source_id, page).await,
            Self::Sandbox(f) => f.get_popular_manga(source_id, page).await,
            #[cfg(test)]
            Self::Test(f) => f.get_popular_manga(source_id, page).await,
        }
    }

    async fn get_latest_updates(&self, source_id: i64, page: u32) -> crate::error::Result<MangasPage> {
        match self {
            Self::Stub => StubFetcher.get_latest_updates(source_id, page).await,
            Self::Sandbox(f) => f.get_latest_updates(source_id, page).await,
            #[cfg(test)]
            Self::Test(f) => f.get_latest_updates(source_id, page).await,
        }
    }

    async fn search_manga(&self, source_id: i64, query: &str, page: u32) -> crate::error::Result<MangasPage> {
        match self {
            Self::Stub => StubFetcher.search_manga(source_id, query, page).await,
            Self::Sandbox(f) => f.search_manga(source_id, query, page).await,
            #[cfg(test)]
            Self::Test(f) => f.search_manga(source_id, query, page).await,
        }
    }

    async fn fetch_pages(
        &self,
        source_id: i64,
        manga_url: &str,
        chapter_url: &str,
    ) -> crate::error::Result<Vec<suwayomi_core::source::SourcePage>> {
        match self {
            Self::Stub => StubFetcher.fetch_pages(source_id, manga_url, chapter_url).await,
            Self::Sandbox(f) => f.fetch_pages(source_id, manga_url, chapter_url).await,
            #[cfg(test)]
            Self::Test(f) => f.fetch_pages(source_id, manga_url, chapter_url).await,
        }
    }

    fn supports_latest(&self, source_id: i64) -> bool {
        match self {
            Self::Stub => StubFetcher.supports_latest(source_id),
            Self::Sandbox(f) => f.supports_latest(source_id),
            #[cfg(test)]
            Self::Test(f) => f.supports_latest(source_id),
        }
    }

    async fn get_filters(&self, source_id: i64) -> crate::error::Result<serde_json::Value> {
        match self {
            Self::Stub => StubFetcher.get_filters(source_id).await,
            Self::Sandbox(f) => f.get_filters(source_id).await,
            #[cfg(test)]
            Self::Test(f) => f.get_filters(source_id).await,
        }
    }
}

/// Convert an external http(s) image URL into the server's same-origin
/// proxy path `/api/v1/image/{b64}` (handled by `suwayomi-rest::routes::image`).
/// Non-http values pass through unchanged so already-proxied paths and local
/// paths round-trip safely. The encoding must stay in lockstep with the REST
/// route; keep both here.
pub fn image_proxy_url(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(url);
        format!("/api/v1/image/{b64}")
    } else {
        url.to_string()
    }
}
