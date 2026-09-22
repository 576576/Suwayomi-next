//! 扩展仓库管理：refresh_stores（拉各 repo index 并 upsert extension 表）→
//! install/uninstall/install_external（APK 下载 + 沙盒热载 + 注册源）→
//! sync_sources（读沙盒 /sources 稳定 id upsert source 表）。
//! 无沙盒时仅刷新可用，install 报错。

use reqwest::Client;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use suwayomi_core::db::Db;

use crate::error::{DomainError, Result};
use crate::source::sandbox::HttpSandboxFetcher;

/// A repo index can be either the legacy top-level array or the v2 object
/// with an `extensionList` field (e.g. keiyoushi).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RepoIndex {
    V1(Vec<RepoEntry>),
    V2 {
        #[serde(rename = "extensionList")]
        extension_list: RepoV2List,
    },
}

#[derive(Debug, Deserialize)]
struct RepoV2List {
    #[serde(rename = "extensions")]
    extensions: Vec<RepoEntry>,
}

/// One repo entry in either wire format.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RepoEntry {
    V1(RepoIndexEntry),
    V2(RepoIndexEntryV2),
}

impl RepoEntry {
    fn into_v1(self) -> RepoIndexEntry {
        match self {
            RepoEntry::V1(e) => e,
            RepoEntry::V2(e) => RepoIndexEntry {
                name: e.name,
                pkg: e.package_name,
                apk: e.resources.apk_url,
                icon: e.resources.icon_url,
                jar: e.resources.jar_url,
                lang: e.lang,
                version_name: e.version_name,
                version_code: e.version_code.as_i64(),
                nsfw: e.nsfw || e.content_warning.as_ref().is_some_and(RawContentWarning::is_nsfw),
                obsolete: e.obsolete,
                has_readme: false,
                sources: e.sources,
            },
        }
    }
}

impl RepoIndex {
    /// 拆成条目 + 是不是旧版（裸数组）那支 —— 旧版的 icon 要按约定补。
    fn entries(self) -> (Vec<RepoIndexEntry>, bool) {
        match self {
            RepoIndex::V1(v) => (v.into_iter().map(RepoEntry::into_v1).collect(), true),
            RepoIndex::V2 { extension_list } => (
                extension_list.extensions.into_iter().map(RepoEntry::into_v1).collect(),
                false,
            ),
        }
    }
}

/// versionCode appears as a number in legacy repos and as a *string* in v2.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StrOrNum {
    S(String),
    N(i64),
}

impl StrOrNum {
    fn as_i64(&self) -> i64 {
        match self {
            StrOrNum::N(n) => *n,
            StrOrNum::S(s) => s.parse().unwrap_or(0),
        }
    }
}

/// v2 repo entry: `packageName` + nested `resources.apkUrl`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepoIndexEntryV2 {
    name: String,
    package_name: String,
    resources: RepoResources,
    #[serde(default)]
    lang: String,
    version_name: String,
    version_code: StrOrNum,
    #[serde(default, deserialize_with = "de_bool_or_int")]
    nsfw: bool,
    /// keiyoushi 的 v2 索引不给 `nsfw`，给的是 `contentWarning` 枚举。
    #[serde(default)]
    content_warning: Option<RawContentWarning>,
    #[serde(default)]
    obsolete: bool,
    #[serde(default)]
    sources: Vec<RepoSource>,
}

/// v2 的 `contentWarning`：`CONTENT_WARNING_SAFE` / `MIXED` / `NSFW`（也见过纯数字）。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawContentWarning {
    Str(String),
    Int(i64),
}

impl RawContentWarning {
    /// 与 Mihon `index.pb` 那边同一条判据（`ContentWarning >= 2`）：**MIXED 也算 NSFW**。
    fn is_nsfw(&self) -> bool {
        match self {
            RawContentWarning::Str(s) => {
                let s = s.to_ascii_uppercase();
                s.ends_with("MIXED") || s.ends_with("NSFW")
            }
            RawContentWarning::Int(n) => *n >= 2,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepoResources {
    #[serde(default)]
    apk_url: Option<String>,
    #[serde(default)]
    icon_url: Option<String>,
    #[serde(default)]
    jar_url: Option<String>,
}

/// One entry of a tachiyomi repo `index.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoIndexEntry {
    pub name: String,
    pub pkg: String,
    /// 旧版仓库里是**相对 index 的路径**（`xxx.apk`、`apk/xxx.apk`），
    /// 由 [`parse_index`] 拼成绝对 URL。
    #[serde(default)]
    pub apk: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub jar: Option<String>,
    #[serde(default)]
    pub lang: String,
    /// 旧版仓库写 `version`。
    #[serde(alias = "version")]
    pub version_name: String,
    /// 旧版仓库写 `code`。
    #[serde(alias = "code")]
    pub version_code: i64,
    #[serde(default, deserialize_with = "de_bool_or_int")]
    pub nsfw: bool,
    #[serde(default)]
    pub obsolete: bool,
    #[serde(default)]
    pub has_readme: bool,
    #[serde(default)]
    pub sources: Vec<RepoSource>,
}

/// `nsfw` 在旧版仓库里是 `0`/`1`，新版才是布尔。
fn de_bool_or_int<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BoolOrInt {
        Bool(bool),
        Int(i64),
    }
    Ok(match BoolOrInt::deserialize(deserializer)? {
        BoolOrInt::Bool(b) => b,
        BoolOrInt::Int(n) => n != 0,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSource {
    pub name: String,
    /// v1 repos use `lang`, v2 repos use `language`.
    #[serde(alias = "language")]
    pub lang: String,
    pub id: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub version_id: i64,
    #[serde(default)]
    pub obsolete: bool,
}

/// (apk_url, pkg_name, version_name, version_code, lang, apk_name)
type InstallRow = (Option<String>, String, String, i64, String, Option<String>);

/// Extension store service bound to the database + optional sandbox.
#[derive(Clone)]
pub struct ExtensionStoreService {
    db: Db,
    http: Client,
    sandbox: Option<HttpSandboxFetcher>,
    extensions_dir: PathBuf,
    /// Directory for dex2jar-converted jars (release layout: `bin/extensions`).
    jar_dir: PathBuf,
    /// 统一缓存根（`<发布根>/cache`），仓库索引缓存落在其 `extensions/index/` 下。
    /// 单独持有而非每次调 `cache_root()`，测试才能注入独立目录（见 `with_dirs`）。
    cache_dir: PathBuf,
}

impl ExtensionStoreService {
    pub fn new(db: Db, sandbox_base: Option<String>) -> Self {
        let extensions_dir = std::env::var("SUWAYOMI_EXTENSIONS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./extensions"));
        let jar_dir = std::env::var("SUWAYOMI_JAR_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                extensions_dir
                    .parent()
                    .map(|p| p.join("bin").join("extensions"))
                    .unwrap_or_else(|| PathBuf::from("bin/extensions"))
            });
        Self::with_dirs(db, sandbox_base, extensions_dir, jar_dir)
    }

    /// 显式指定扩展目录 / jar 目录 / 缓存根的构造器。
    ///
    /// 测试专用：`new()` 从进程环境变量读取目录，而 `std::env::set_var` 在
    /// Rust 2024 起是 `unsafe`（且多线程下修改进程环境本身就是数据竞争），
    /// 并行测试还会互相覆盖 `SUWAYOMI_EXTENSIONS_DIR` / `SUWAYOMI_CACHE_DIR`。
    /// 改为注入路径后，各测试持有独立临时目录，无需触碰环境变量。
    pub fn with_dirs(
        db: Db,
        sandbox_base: Option<String>,
        extensions_dir: PathBuf,
        jar_dir: PathBuf,
    ) -> Self {
        Self::with_cache_dir(db, sandbox_base, extensions_dir, jar_dir, suwayomi_core::config::cache_root())
    }

    /// 同 [`Self::with_dirs`]，但额外显式指定缓存根（`index_cache_path` 用）。
    pub fn with_cache_dir(
        db: Db,
        sandbox_base: Option<String>,
        extensions_dir: PathBuf,
        jar_dir: PathBuf,
        cache_dir: PathBuf,
    ) -> Self {
        let mut builder = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(30))
            .read_timeout(std::time::Duration::from_secs(60));
        // reuse the sandbox outbound proxy for repo fetches / APK downloads
        if let Ok(proxy) = std::env::var("SUWAYOMI_SANDBOX_PROXY")
            && !proxy.is_empty()
            && let Ok(p) = reqwest::Proxy::all(&proxy)
        {
            builder = builder.proxy(p);
        }
        Self {
            db,
            http: builder.build().expect("reqwest client"),
            sandbox: sandbox_base.map(HttpSandboxFetcher::new),
            extensions_dir,
            jar_dir,
            cache_dir,
        }
    }

    pub fn sandbox_available(&self) -> bool {
        self.sandbox.is_some()
    }

    pub fn extensions_dir(&self) -> &Path {
        &self.extensions_dir
    }

    pub fn jar_dir(&self) -> &Path {
        &self.jar_dir
    }

    // ------------------------------------------------------------------
    // Repo index refresh
    // ------------------------------------------------------------------

    /// Fetches `index.json` from every configured store and upserts the
    /// extension table. Returns the number of extensions now known.
    pub async fn refresh_stores(&self) -> Result<usize> {
        let stores: Vec<(String, String)> = suwayomi_db::query_as(
            "SELECT index_url, name FROM suwayomi.extension_store ORDER BY id",
        )
        .fetch_all(self.db.pool())
        .await?;
        let mut total = 0usize;
        for (index_url, store_name) in stores {
            total += self.refresh_one(&index_url, &store_name).await?;
        }
        Ok(total)
    }

    /// 拉取单个 repo 的 index 并 upsert。index 缓存到
    /// `<cache>/extensions/index/index-<hash>.<ext>`；网络失败/解析失败
    /// 回退本地缓存——仓库暂时宕机不会挂起刷新或清空仓库
    pub async fn refresh_one(&self, index_url: &str, store_name: &str) -> Result<usize> {
        let _ = store_name;
        let url = normalize_index_url(index_url);
        let cache_file = index_cache_path(&self.cache_dir, index_url);
        if let Some(dir) = cache_file.parent() {
            prune_legacy_index_dirs(dir);
        }
        let mut downloaded: Option<Vec<u8>> = None;

        match self.http.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(b) => {
                    let b = decompress_gzip_if_needed(&b);
                    // write-through cache
                    if let Some(dir) = cache_file.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let _ = std::fs::write(&cache_file, &b);
                    downloaded = Some(b);
                }
                Err(e) => tracing::warn!("repo {index_url}: read body: {e}"),
            },
            Ok(resp) => tracing::warn!("repo {index_url} returned {}", resp.status()),
            Err(e) => tracing::warn!("repo fetch {index_url}: {e}"),
        }

        // Prefer the freshly downloaded index; on any failure fall back to
        // the on-disk cache so the extension table keeps its last good state.
        if let Some(bytes) = &downloaded {
            match self.upsert_index(bytes, &url, index_url).await {
                Ok(n) => return Ok(n),
                Err(e) => tracing::warn!("repo {index_url} index parse failed ({e}); falling back to cache"),
            }
        }
        if cache_file.exists() {
            let cached = std::fs::read(&cache_file)
                .map_err(|e| DomainError::Source(format!("read cached index: {e}")))?;
            return self.upsert_index(&cached, &url, index_url).await;
        }
        // nothing usable at all — surface the original error if we had one
        if let Some(bytes) = downloaded {
            return self.upsert_index(&bytes, &url, index_url).await;
        }
        Err(DomainError::Source(format!(
            "repo {index_url} unreachable and no cached index under {}",
            cache_file.display()
        )))
    }

    /// Parses a repo index (Mihon `index.pb` or tachiyomi `index.json`) and
    /// upserts its entries into the `extension` table.
    async fn upsert_index(&self, bytes: &[u8], url: &str, index_url: &str) -> Result<usize> {
        let entries = parse_index(bytes, url)?;
        let pool = self.db.pool();

        // 沙盒实际加载了哪些包。`is_installed_for_local_dir` 只看 extensions/
        // 目录里的文件，而 Android 上那个目录恒空（扩展装在系统里），刷新一次
        // 仓库就会把所有已安装扩展冲成"未安装"—— 把沙盒的加载结果并进来。
        // 沙盒不可用/查询失败时退化为纯文件判定（桌面原有语义）。
        let loaded: std::collections::HashSet<String> = match &self.sandbox {
            Some(f) => f
                .list_extensions()
                .await
                .map(|v| v.into_iter().map(|e| e.pkg_name).collect())
                .unwrap_or_default(),
            None => std::collections::HashSet::new(),
        };

        let mut n = 0usize;
        for e in entries {
            let content_warning = if e.nsfw { 1 } else { 0 };
            suwayomi_db::query(
                "INSERT INTO suwayomi.extension \
                 (apk_name, store_index_url, name, pkg_name, apk_url, icon_url, jar_url, version_name, version_code, lang, content_warning, is_installed, has_update, is_obsolete, class_name) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, FALSE, $13, '') \
                 ON CONFLICT (pkg_name) DO UPDATE SET \
                   apk_name = EXCLUDED.apk_name, store_index_url = EXCLUDED.store_index_url, \
                   name = EXCLUDED.name, apk_url = EXCLUDED.apk_url, icon_url = EXCLUDED.icon_url, \
                   jar_url = EXCLUDED.jar_url, version_name = EXCLUDED.version_name, \
                   version_code = EXCLUDED.version_code, \
                   lang = EXCLUDED.lang, content_warning = EXCLUDED.content_warning, \
                   is_obsolete = EXCLUDED.is_obsolete, \
                   has_update = (EXCLUDED.version_code > suwayomi.extension.version_code AND suwayomi.extension.is_installed)",
            )
            .bind(e.apk.as_deref().map(apk_file_name))
            .bind(index_url)
            .bind(&e.name)
            .bind(&e.pkg)
            .bind(&e.apk)
            // icon_url 是 NOT NULL：旧版仓库条目里压根没有 icon 字段，绑 NULL 会让
            // 整个 upsert 失败（"NOT NULL constraint failed"）。空串在图标路由那边
            // 有明确语义 —— 跳过这个来源，回退沙盒/占位。
            .bind(e.icon.as_deref().unwrap_or(""))
            .bind(&e.jar)
            .bind(&e.version_name)
            .bind(e.version_code)
            .bind(&e.lang)
            .bind(content_warning)
            .bind(e.is_installed_for_local_dir(&self.extensions_dir) || loaded.contains(&e.pkg))
            .bind(e.obsolete)
            .execute(pool)
            .await
            .map_err(|err| DomainError::Source(format!("extension upsert {}: {err}", e.pkg)))?;
            n += 1;
        }
        Ok(n)
    }


    // ------------------------------------------------------------------
    // Install / update / uninstall
    // ------------------------------------------------------------------

    /// Downloads and installs (or updates) the extension identified by pkg.
    pub async fn install(&self, pkg: &str) -> Result<()> {
        let fetcher = self.require_sandbox()?;
        let row: Option<InstallRow> = suwayomi_db::query_as(
            "SELECT apk_url, pkg_name, version_name, version_code::BIGINT, lang::VARCHAR, apk_name FROM suwayomi.extension WHERE pkg_name = $1",
        )
        .bind(pkg)
        .fetch_optional(self.db.pool())
        .await?;
        let (apk_url, pkg_name, version_name, version_code, lang, apk_name) = row
            .ok_or_else(|| DomainError::Source(format!("extension {pkg} not found in store (run refresh first)")))?;
        let apk_url = apk_url.ok_or_else(|| DomainError::Source(format!("extension {pkg} has no apk url")))?;

        std::fs::create_dir_all(&self.extensions_dir)
            .map_err(|e| DomainError::Source(format!("create extensions dir: {e}")))?;
        // The sandbox scans `tachiyomi-{lang}.{pkg}-v{version}.apk`; prefer the
        // store's own filename, fall back to our canonical naming.
        let file_name = match apk_name {
            Some(n) if !n.is_empty() => n,
            _ => format!("tachiyomi-{lang}.{pkg_name}-v{version_name}.apk"),
        };
        let target = self.extensions_dir.join(&file_name);

        // remove any previous versions of the same package first
        remove_matching_apks(&self.extensions_dir, pkg)?;
        remove_matching_jars(&self.jar_dir, pkg)?;

        let bytes = self
            .http
            .get(&apk_url)
            .send()
            .await
            .map_err(|e| DomainError::Source(format!("download {pkg}: {e}")))?
            .error_for_status()
            .map_err(|e| DomainError::Source(format!("download {pkg}: {e}")))?
            .bytes()
            .await
            .map_err(DomainError::from)?;
        std::fs::write(&target, &bytes)
            .map_err(|e| DomainError::Source(format!("write {file_name}: {e}")))?;

        fetcher.reload().await?;
        self.sync_sources().await?;

        // mark installed + clear has_update
        suwayomi_db::query(
            "UPDATE suwayomi.extension SET is_installed = TRUE, has_update = FALSE, apk_name = $1 WHERE pkg_name = $2",
        )
        .bind(&file_name)
        .bind(pkg)
        .execute(self.db.pool())
        .await?;
        let _ = version_code;
        Ok(())
    }

    /// Removes the extension APK, unregisters its sources, hot-reloads.
    pub async fn uninstall(&self, pkg: &str) -> Result<()> {
        let fetcher = self.require_sandbox()?;
        // sources are registered against the extension row; delete them first
        suwayomi_db::query(
            "DELETE FROM suwayomi.source WHERE extension = (SELECT id FROM suwayomi.extension WHERE pkg_name = $1)",
        )
        .bind(pkg)
        .execute(self.db.pool())
        .await?;
        remove_matching_apks(&self.extensions_dir, pkg)?;
        remove_matching_jars(&self.jar_dir, pkg)?;
        fetcher.reload().await?;
        suwayomi_db::query("UPDATE suwayomi.extension SET is_installed = FALSE, has_update = FALSE WHERE pkg_name = $1")
            .bind(pkg)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    /// Installs an externally-uploaded APK (bytes) by asking the sandbox to
    /// parse it, then persisting it under its canonical name.
    pub async fn install_external(&self, apk: &[u8]) -> Result<()> {
        let fetcher = self.require_sandbox()?;
        let meta = fetcher.inspect(apk).await?;
        std::fs::create_dir_all(&self.extensions_dir)
            .map_err(|e| DomainError::Source(format!("create extensions dir: {e}")))?;
        remove_matching_apks(&self.extensions_dir, &meta.pkg_name)?;
        remove_matching_jars(&self.jar_dir, &meta.pkg_name)?;
        let file_name = format!("tachiyomi-{}.{}-v{}.apk", meta.lang, meta.pkg_name, meta.version_name);
        let target = self.extensions_dir.join(&file_name);
        std::fs::write(&target, apk).map_err(|e| DomainError::Source(format!("write {file_name}: {e}")))?;
        fetcher.reload().await?;
        self.sync_sources().await?;
        // 兜底（正常情况 sync_sources 已经建好行）：补上本地 APK 文件名，
        // 后续 uninstall / upsert_index 都按它找文件。
        suwayomi_db::query(
            "INSERT INTO suwayomi.extension \
             (apk_name, name, pkg_name, version_name, version_code, lang, content_warning, is_installed, class_name) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE, $8) \
             ON CONFLICT (pkg_name) DO UPDATE SET apk_name = EXCLUDED.apk_name, name = EXCLUDED.name, \
               version_name = EXCLUDED.version_name, version_code = EXCLUDED.version_code, lang = EXCLUDED.lang, \
               is_installed = TRUE, class_name = EXCLUDED.class_name, \
               content_warning = CASE WHEN suwayomi.extension.store_index_url IS NULL \
                 THEN EXCLUDED.content_warning ELSE suwayomi.extension.content_warning END",
        )
        .bind(&file_name)
        .bind(&meta.name)
        .bind(&meta.pkg_name)
        .bind(&meta.version_name)
        .bind(meta.version_code)
        .bind(&meta.lang)
        .bind(meta.content_warning)
        .bind(&meta.class_name)
        .execute(self.db.pool())
        .await?;
        // 源行跟随扩展行的 content_warning（仓库索引优先，非仓库来源用 APK
        // meta-data —— 见上面的 CASE）
        suwayomi_db::query(
            "UPDATE suwayomi.source AS s SET content_warning = e.content_warning \
             FROM suwayomi.extension AS e WHERE s.extension = e.id",
        )
        .execute(self.db.pool())
        .await?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Source registration
    // ------------------------------------------------------------------

    /// Synchronises the sandbox's loaded sources into the `source` table.
    /// Source ids come from the extension's `Source.getId()` and are stable
    /// across reloads; `extension` links back to the pkg_name row.
    pub async fn sync_sources(&self) -> Result<usize> {
        let fetcher = self.require_sandbox()?;
        let _sources = fetcher.list_sources().await?;
        let exts = fetcher.list_extensions().await?;
        let pool = self.db.pool();

        // ---- 1) 扩展表回写 -------------------------------------------------
        //
        // 这里必须能**建行**：桌面上 `extension` 表是仓库索引的镜像
        // （refresh_stores 写入），手工丢进 extensions/ 的 APK 没有索引行；
        // Android 更彻底 —— 扩展来自系统 PackageManager，压根没有仓库索引。
        // 此前这里直接跳过没有索引行的包（怕 insert 撞 NOT NULL），表现为
        // "沙盒里有源、server 侧扩展列表却是空的"。
        //
        // `store_index_url` 保持 NULL 即"非仓库来源"，用它区分权威来源：仓库
        // 索引里的 nsfw 标记比 APK meta-data 可靠，所以只在非仓库行上用沙盒
        // 报上来的 content_warning 覆盖。
        for e in &exts {
            suwayomi_db::query(
                "INSERT INTO suwayomi.extension \
                 (name, pkg_name, version_name, version_code, lang, content_warning, is_installed, class_name) \
                 VALUES ($1, $2, $3, $4, $5, $6, TRUE, $7) \
                 ON CONFLICT (pkg_name) DO UPDATE SET \
                   name = EXCLUDED.name, version_name = EXCLUDED.version_name, \
                   version_code = EXCLUDED.version_code, lang = EXCLUDED.lang, \
                   is_installed = TRUE, class_name = EXCLUDED.class_name, \
                   content_warning = CASE WHEN suwayomi.extension.store_index_url IS NULL \
                     THEN EXCLUDED.content_warning ELSE suwayomi.extension.content_warning END",
            )
            .bind(&e.name)
            .bind(&e.pkg_name)
            .bind(&e.version_name)
            .bind(e.version_code)
            .bind(&e.lang)
            .bind(e.content_warning)
            .bind(&e.class_name)
            .execute(pool)
            .await?;
        }

        // ---- 2) 已卸载的扩展回写未安装 -------------------------------------
        //
        // 只清"沙盒不再报告 **且** 本地也没有对应 APK 文件"的行：
        //  - Android：扩展装在系统里，extensions/ 恒空，所以系统卸载后这里
        //    是唯一的回写时机（走系统安装器的卸载我们收不到回调）。
        //  - 桌面：文件还在就保持原样 —— upsert_index 按文件判定 is_installed，
        //    在这里用"沙盒没加载"去清会和它来回打架（加载失败但文件仍在）。
        //
        // `list_extensions()` 失败会在这里之前 `?` 返回，所以能走到这一步就说明
        // 列表是沙盒的真话（"空"= 确实一个都没有，不是查询失败）。
        let loaded: std::collections::HashSet<&str> = exts.iter().map(|e| e.pkg_name.as_str()).collect();
        let rows: Vec<(i32, String, Option<String>)> =
            suwayomi_db::query_as("SELECT id, pkg_name, apk_name FROM suwayomi.extension")
                .fetch_all(pool)
                .await?;
        let stale: Vec<i32> = rows
            .into_iter()
            .filter(|(_, pkg, apk)| {
                !loaded.contains(pkg.as_str())
                    && !apk.as_deref().is_some_and(|n| self.extensions_dir.join(n).exists())
            })
            .map(|(id, _, _)| id)
            .collect();
        for id in stale {
            suwayomi_db::query("UPDATE suwayomi.extension SET is_installed = FALSE WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?;
        }

        // ---- 3) 源行注册 ---------------------------------------------------
        //
        // 第 1 步之后每个沙盒扩展都有 extension 行，pkg -> id 必定命中。
        let registered: std::collections::HashMap<String, i32> =
            suwayomi_db::query_as::<(String, i32)>("SELECT pkg_name, id FROM suwayomi.extension")
                .fetch_all(pool)
                .await?
                .into_iter()
                .collect();

        // The sandbox reports each extension together with the sources it
        // provides, so the pkg link is unambiguous here.
        let mut n = 0usize;
        for e in &exts {
            let Some(&ext_id) = registered.get(&e.pkg_name) else { continue };
            for s in &e.sources {
                suwayomi_db::query(
                    "INSERT INTO suwayomi.source \
                       (id, name, lang, extension, supports_latest, is_configurable, base_url, home_url) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, lang = EXCLUDED.lang, \
                       extension = EXCLUDED.extension, supports_latest = EXCLUDED.supports_latest, \
                       is_configurable = EXCLUDED.is_configurable, base_url = EXCLUDED.base_url, \
                       home_url = EXCLUDED.home_url",
                )
                .bind(s.id)
                .bind(&s.name)
                .bind(&s.lang)
                .bind(ext_id)
                .bind(s.supports_latest)
                .bind(s.is_configurable)
                .bind(&s.base_url)
                .bind(&s.home_url)
                .execute(pool)
                .await?;
                n += 1;
            }
        }

        // 源行继承所属扩展的 content_warning（来源：仓库索引，非仓库来源则是
        // APK meta-data）。sync_sources 此前从不写该列，源行恒为 0(Safe)，
        // 导致"图源列表隐藏 NSFW"过滤永远放行。
        suwayomi_db::query(
            "UPDATE suwayomi.source AS s SET content_warning = e.content_warning \
             FROM suwayomi.extension AS e WHERE s.extension = e.id",
        )
        .execute(pool)
        .await?;
        Ok(n)
    }

    // ------------------------------------------------------------------

    fn require_sandbox(&self) -> Result<HttpSandboxFetcher> {
        self.sandbox
            .clone()
            .ok_or_else(|| DomainError::Sandbox("extension install requires the JVM sandbox (SUWAYOMI_SANDBOX_JAR)".into()))
    }
}

impl RepoIndexEntry {
    fn is_installed_for_local_dir(&self, dir: &Path) -> bool {
        if !dir.is_dir() {
            return false;
        }
        let name = self.apk.as_deref().map(apk_file_name);
        if let Some(n) = name
            && dir.join(n).exists()
        {
            return true;
        }
        // fall back: any file named tachiyomi-{lang}.{pkg}* in the dir
        let prefix = format!("{}.{}", self.lang, self.pkg);
        if let Ok(rd) = std::fs::read_dir(dir) {
            for f in rd.flatten() {
                if let Some(fn_) = f.file_name().to_str()
                    && fn_.starts_with(&prefix)
                    && fn_.ends_with(".apk")
                {
                    return true;
                }
            }
        }
        false
    }
}

fn apk_file_name(url: &str) -> String {
    url.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("ext.apk").to_string()
}

fn remove_matching_apks(dir: &Path, pkg: &str) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for f in std::fs::read_dir(dir).map_err(|e| DomainError::Source(format!("read dir: {e}")))? {
        let f = f.map_err(|e| DomainError::Source(format!("read dir entry: {e}")))?;
        let name = f.file_name().to_string_lossy().into_owned();
        // matches tachiyomi-<lang>.<short-pkg>-v<ver>.apk; the store's pkg is
        // the full package name (eu.kanade.tachiyomi.extension.*), while the
        // file uses the short form (all.nhentaicom).
        let short = pkg.strip_prefix("eu.kanade.tachiyomi.extension.").unwrap_or(pkg);
        if name.ends_with(".apk") && name.contains(short) {
            std::fs::remove_file(f.path()).map_err(|e| DomainError::Source(format!("remove {name}: {e}")))?;
        }
    }
    Ok(())
}

/// Removes converted jars of the given package from the jar directory.
/// The jar filename mirrors the apk's (`tachiyomi-<lang>.<short>-v<ver>.jar`).
fn remove_matching_jars(dir: &Path, pkg: &str) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for f in std::fs::read_dir(dir).map_err(|e| DomainError::Source(format!("read dir: {e}")))? {
        let f = f.map_err(|e| DomainError::Source(format!("read dir entry: {e}")))?;
        let name = f.file_name().to_string_lossy().into_owned();
        let short = pkg.strip_prefix("eu.kanade.tachiyomi.extension.").unwrap_or(pkg);
        if name.ends_with(".jar") && name.contains(short) {
            std::fs::remove_file(f.path()).map_err(|e| DomainError::Source(format!("remove {name}: {e}")))?;
        }
    }
    Ok(())
}

/// 用户填的可能是仓库根目录（补成 `/index.json`），也可能直接给了索引文件：
/// `index.json`、Mihon 的 `index.pb`，或旧版仓库那个 `index.min.json`
/// （后者不能被拼成 `index.min.json/index.json`）。
fn normalize_index_url(url: &str) -> String {
    let url = url.trim();
    if url.ends_with(".json") || url.ends_with(".pb") {
        url.to_string()
    } else {
        format!("{}/index.json", url.trim_end_matches('/'))
    }
}

/// 把仓库索引解析成条目列表（Mihon `index.pb` / Tachiyomi `index.json` 分流）。
///
/// 旧版 JSON 仓库里的 `apk` / `icon` / `jar` 是**相对 index 的路径**，直接拿去请求
/// 会报 "relative URL without a base"，这里补成绝对 URL（见 [`resolve_asset_url`]）；
/// 旧版条目还没有 `icon`，按仓库约定补 `<index 目录>/icon/<pkg>.png`。新版 JSON 不给
/// `lang`（见 [`lang_from_apk_name`]）、NSFW 藏在 `contentWarning` 里（见
/// [`RawContentWarning`]），都在这里抹平。
fn parse_index(bytes: &[u8], url: &str) -> Result<Vec<RepoIndexEntry>> {
    let (mut entries, legacy) = if url.ends_with(".pb") {
        (
            parse_mihon_pb_index(bytes).map_err(|e| DomainError::Source(format!("mihon repo index parse: {e}")))?,
            false,
        )
    } else {
        let index: RepoIndex =
            serde_json::from_slice(bytes).map_err(|e| DomainError::Source(format!("repo index parse: {e}")))?;
        index.entries()
    };
    let base = index_base_url(url);
    for entry in &mut entries {
        let apk = entry.apk.as_deref().map(|v| resolve_asset_url(&base, "apk", v));
        let icon = entry.icon.as_deref().map(|v| resolve_asset_url(&base, "icon", v));
        let jar = entry.jar.as_deref().map(|v| resolve_asset_url(&base, "jar", v));
        entry.apk = apk;
        entry.icon = icon;
        entry.jar = jar;
        // 旧版仓库不写 icon 字段，但图标按约定放在 `<index 目录>/icon/<pkg>.png`
        // （keiyoushi 的 index.html 与仓库目录都是这个布局）。
        if entry.icon.is_none() && legacy {
            entry.icon = Some(format!("{base}icon/{}.png", entry.pkg));
        }
        // 新版 JSON 不写 lang（语言散在 `sources[].language`，多语言扩展会列出一堆，
        // 拿第一条当扩展语言是错的）—— 按 Tachiyomi 的 APK 命名约定取语言段。
        if entry.lang.is_empty()
            && let Some(lang) = lang_from_apk_name(entry.apk.as_deref().unwrap_or_default())
        {
            entry.lang = lang;
        }
    }
    Ok(entries)
}

/// `tachiyomi-zh.copymanga-v1.4.53.apk` → `zh`；多语言扩展是 `all`。
fn lang_from_apk_name(apk: &str) -> Option<String> {
    let file = apk.rsplit('/').next()?;
    let lang = file.strip_prefix("tachiyomi-")?.split('.').next()?;
    (!lang.is_empty()).then(|| lang.to_string())
}

/// index 所在目录：`…/repo/index.min.json` → `…/repo/`（拼相对 URL 用）。
fn index_base_url(url: &str) -> String {
    match url.rfind('/') {
        Some(i) => url[..=i].to_string(),
        None => url.to_string(),
    }
}

/// 旧版仓库的资源路径补全。布局是 `<index 目录>/{apk,icon,jar}/<文件名>`：条目里
/// 只写文件名（`tachiyomi-zh.copymanga-v1.4.53.apk`），所以光拼 index 目录会 404
/// —— 实测 `<repo>/apk/xxx.apk` 200、`<repo>/xxx.apk` 404。已经带了目录的相对路径
/// 原样拼，绝对 URL 不动（新版 JSON 与 Mihon `.pb` 给的都是绝对 URL）。
fn resolve_asset_url(base: &str, subdir: &str, value: &str) -> String {
    if value.contains("://") {
        return value.to_string();
    }
    let value = value.trim_start_matches('/');
    if value.contains('/') {
        return format!("{base}{value}");
    }
    format!("{base}{subdir}/{value}")
}

/// 仓库索引的本地缓存路径：`<cache>/extensions/index/index-<hash>.<ext>`，
/// hash 取规范化 index URL 的哈希（跨刷新、跨重启稳定），ext 是 `pb` / `json`。
/// 缓存根在发布根下的统一 `<发布根>/cache`（见 `suwayomi_core::config::cache_root`），
/// 不在扩展目录里。
fn index_cache_path(cache_dir: &Path, index_url: &str) -> PathBuf {
    let url = normalize_index_url(index_url);
    let ext = if url.ends_with("index.pb") { "pb" } else { "json" };
    cache_dir
        .join("extensions")
        .join("index")
        .join(format!("index-{:016x}.{ext}", url_hash(&url)))
}

/// 同一个 URL 跨进程稳定的哈希（缓存文件名用）。
fn url_hash(url: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    hasher.finish()
}

/// 索引缓存目录里只该有 `index-<hash>.<ext>` 文件：老布局留下的 `<repo>/`
/// 目录顺手删掉（纯缓存，下次刷新会重下）。
fn prune_legacy_index_dirs(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn decompress_gzip_if_needed(bytes: &[u8]) -> Vec<u8> {
    // gzip magic 1f 8b
    if bytes.len() > 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        use std::io::Read;
        let mut out = Vec::new();
        let mut decoder = flate2::read::GzDecoder::new(bytes);
        if decoder.read_to_end(&mut out).is_ok() {
            return out;
        }
    }
    bytes.to_vec()
}
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// These tests exercise the extension index/install path against external
    /// PostgreSQL via DATABASE_URL; skipped otherwise (same convention as the
    /// version-bump trigger test).
    async fn setup_db() -> Option<Db> {
        let url = std::env::var("DATABASE_URL").ok().filter(|u| !u.is_empty())?;
        let db = suwayomi_core::db::Db::postgres(&url).await.expect("db");
        db.migrate().await.expect("migrate");
        // clear extension-related tables so tests are repeatable
        let _ = suwayomi_db::query("TRUNCATE suwayomi.source, suwayomi.extension, suwayomi.extension_store CASCADE")
            .execute(db.pool()).await;
        Some(db)
    }

    /// 建一个独立临时根目录（不改进程环境变量，测试可并行）。
    /// 调用方负责在结尾 `remove_dir_all`。
    fn tmp_root() -> PathBuf {
        let tmp = std::env::temp_dir().join(format!("ext-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        tmp
    }

    /// 构造指向独立临时目录的 service（不改进程环境变量，测试可并行）。
    fn service_in(tmp: &Path, db: Db, sandbox_base: Option<String>) -> ExtensionStoreService {
        let extensions = tmp.join("extensions");
        std::fs::create_dir_all(&extensions).unwrap();
        ExtensionStoreService::with_cache_dir(
            db,
            sandbox_base,
            extensions,
            tmp.join("bin").join("extensions"),
            tmp.join("cache"),
        )
    }

    /// Serves canned HTTP responses for one request then closes.
    fn serve_once(listener: TcpListener, body: &'static [u8], status: &'static str) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 65536];
            let _ = sock.read(&mut buf).await;
            let resp = format!("{status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let mut all = resp.into_bytes();
            all.extend_from_slice(body);
            let _ = sock.write_all(&all).await;
            let _ = sock.shutdown().await;
        })
    }

    #[tokio::test]
    async fn repo_index_refresh_upserts_extensions() {
        let Some(db) = setup_db().await else { eprintln!("SKIP: requires DATABASE_URL"); return };
        suwayomi_db::query("INSERT INTO suwayomi.extension_store (index_url, name, badge_label, signing_key, contact_website) VALUES ('http://127.0.0.1:1/repo-1', 't', '', '', '')")
            .execute(db.pool()).await.unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let index = br#"[{"name":"nhentai.com","pkg":"tachiyomi-all.nhentaicom","apk":"http://127.0.0.1:1/dl.apk","icon":"http://127.0.0.1:1/icon.png","lang":"all","versionName":"1.4.10","versionCode":14,"nsfw":true,"sources":[{"name":"nhentai.com","lang":"en","id":"5591830863732393712"}]},{"name":"MangaDex","pkg":"tachiyomi-all.mangadex","apk":"http://127.0.0.1:1/md.apk","icon":"http://127.0.0.1:1/icon.png","lang":"all","versionName":"1.2.3","versionCode":9,"nsfw":false}]"#;
        let _srv = serve_once(listener, index, "HTTP/1.1 200 OK");

        let tmp = tmp_root();
        let svc = service_in(&tmp, db.clone(), None);
        let n = svc.refresh_one(&format!("http://{addr}/index.json"), "t").await.expect("refresh");
        assert_eq!(n, 2, "two extensions upserted");

        let (name, apk_url, vc, cw, inst): (String, Option<String>, i64, i32, bool) = suwayomi_db::query_as(
            "SELECT name, apk_url, version_code, content_warning, is_installed FROM suwayomi.extension WHERE pkg_name = 'tachiyomi-all.nhentaicom'",
        )
        .fetch_one(db.pool()).await.unwrap();
        assert_eq!(name, "nhentai.com");
        assert_eq!(apk_url.as_deref(), Some("http://127.0.0.1:1/dl.apk"));
        assert_eq!(vc, 14);
        assert_eq!(cw, 1, "nsfw -> content_warning 1");
        assert!(!inst, "not installed yet");
    }

    #[tokio::test]
    async fn install_downloads_apk_and_registers_sources() {
        let Some(db) = setup_db().await else { eprintln!("SKIP: requires DATABASE_URL"); return };
        let tmp = tmp_root();
        let extensions_dir = tmp.join("extensions");

        // fake sandbox: /extensions + /sources + /reload
        let sb = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let sb_addr = sb.local_addr().unwrap();
        let sb_srv = tokio::spawn(async move {
            for _ in 0..3 {
                let (mut sock, _) = sb.accept().await.unwrap();
                let mut buf = vec![0u8; 65536];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let body: &[u8] = if req.contains("/extensions") {
                    br#"[{"pkgName":"tachiyomi-all.nhentaicom","name":"nhentai.com","lang":"all","versionName":"1.4.10","className":"a0","sources":[{"id":5591830863732393712,"name":"nhentai.com","lang":"en"}]}]"#
                } else if req.contains("/sources") {
                    br#"[{"id":5591830863732393712,"name":"nhentai.com","lang":"en","extension":1}]"#
                } else {
                    br#"{"ok":true,"extensions":1,"sources":1}"#
                };
                let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let mut all = resp.into_bytes();
                all.extend_from_slice(body);
                let _ = sock.write_all(&all).await;
                let _ = sock.shutdown().await;
            }
        });

        // fake apk download server
        let dl = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dl_addr = dl.local_addr().unwrap();
        let apk_bytes: &[u8] = b"PK\x03\x04 fake apk";
        let _dlsrv = serve_once(dl, apk_bytes, "HTTP/1.1 200 OK");

        suwayomi_db::query(
            "INSERT INTO suwayomi.extension (name, pkg_name, apk_url, version_name, version_code, lang, content_warning) \
             VALUES ('nhentai.com', 'tachiyomi-all.nhentaicom', $1, '1.4.10', 14, 'all', 1)",
        )
        .bind(format!("http://{dl_addr}/tachiyomi-all.nhentaicom-v1.4.10.apk"))
        .execute(db.pool()).await.unwrap();

        let svc = service_in(&tmp, db.clone(), Some(format!("http://{sb_addr}")));
        svc.install("tachiyomi-all.nhentaicom").await.expect("install");

        let files: Vec<String> = std::fs::read_dir(&extensions_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            files.iter().any(|f| f.contains("tachiyomi-all.nhentaicom") && f.ends_with(".apk")),
            "apk persisted: {files:?}"
        );
        let (sid, sname, slang): (i64, String, String) = suwayomi_db::query_as(
            "SELECT id, name, lang FROM suwayomi.source WHERE id = 5591830863732393712",
        )
        .fetch_one(db.pool()).await.unwrap();
        assert_eq!(sid, 5591830863732393712);
        assert_eq!(sname, "nhentai.com");
        assert_eq!(slang, "en");
        let inst: bool = suwayomi_db::query_scalar("SELECT is_installed FROM suwayomi.extension WHERE pkg_name = 'tachiyomi-all.nhentaicom'")
            .fetch_one(db.pool()).await.unwrap();
        assert!(inst, "extension marked installed");

        sb_srv.await.unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn uninstall_removes_apk_and_sources() {
        let Some(db) = setup_db().await else { eprintln!("SKIP: requires DATABASE_URL"); return };
        let tmp = tmp_root();
        let extensions_dir = tmp.join("extensions");
        std::fs::create_dir_all(&extensions_dir).unwrap();
        std::fs::write(extensions_dir.join("tachiyomi-all.mangadex-v1.2.3.apk"), b"PK fake").unwrap();

        suwayomi_db::query("INSERT INTO suwayomi.extension (name, pkg_name, version_name, version_code, lang, content_warning, is_installed) \
                     VALUES ('mangadex.org', 'tachiyomi-all.mangadex', '1.2.3', 9, 'all', 0, TRUE)")
            .execute(db.pool()).await.unwrap();
        let eid: i32 = suwayomi_db::query_scalar("SELECT id FROM suwayomi.extension WHERE pkg_name = 'tachiyomi-all.mangadex'")
            .fetch_one(db.pool()).await.unwrap();
        suwayomi_db::query("INSERT INTO suwayomi.source (id, name, lang, extension) VALUES (4422762036021677666, 'mangadex.org', 'en', $1)")
            .bind(eid).execute(db.pool()).await.unwrap();

        let sb = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let sb_addr = sb.local_addr().unwrap();
        let sb_srv = tokio::spawn(async move {
            let (mut sock, _) = sb.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = sock.read(&mut buf).await;
            let body = br#"{"ok":true,"extensions":0,"sources":0}"#;
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let mut all = resp.into_bytes();
            all.extend_from_slice(body);
            let _ = sock.write_all(&all).await;
            let _ = sock.shutdown().await;
        });

        let svc = service_in(&tmp, db.clone(), Some(format!("http://{sb_addr}")));
        svc.uninstall("tachiyomi-all.mangadex").await.expect("uninstall");

        let remaining: Vec<String> = std::fs::read_dir(&extensions_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(remaining.is_empty(), "apk removed: {remaining:?}");
        let src_count: i64 = suwayomi_db::query_scalar("SELECT COUNT(*) FROM suwayomi.source WHERE id = 4422762036021677666")
            .fetch_one(db.pool()).await.unwrap();
        assert_eq!(src_count, 0);
        let inst: bool = suwayomi_db::query_scalar("SELECT is_installed FROM suwayomi.extension WHERE pkg_name = 'tachiyomi-all.mangadex'")
            .fetch_one(db.pool()).await.unwrap();
        assert!(!inst, "marked uninstalled");

        sb_srv.await.unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn repo_index_refresh_falls_back_to_cache() {
        let Some(db) = setup_db().await else { eprintln!("SKIP: requires DATABASE_URL"); return };
        let tmp = tmp_root();

        let index = br#"[{"name":"nhentai.com","pkg":"tachiyomi-all.nhentaicom","apk":"http://127.0.0.1:1/dl.apk","icon":"http://127.0.0.1:1/icon.png","lang":"all","versionName":"1.4.10","versionCode":14,"nsfw":true}]"#;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _srv = serve_once(listener, index, "HTTP/1.1 200 OK");

        let svc = service_in(&tmp, db.clone(), None);
        let n = svc
            .refresh_one(&format!("http://{addr}/repo-1/index.json"), "t")
            .await
            .expect("first refresh");
        assert_eq!(n, 1);

        // write-through cache exists after the first successful refresh：
        // 平铺的 `index-<hash>.json`，不再给每个仓库建目录
        let cache_file = index_cache_path(&tmp.join("cache"), &format!("http://{addr}/repo-1/index.json"));
        assert!(cache_file.exists(), "cache written: {}", cache_file.display());
        assert_eq!(cache_file.parent().unwrap().file_name().unwrap(), "index");
        assert_eq!(cache_file.extension().unwrap(), "json");
        assert!(
            cache_file.file_name().unwrap().to_str().unwrap().starts_with("index-"),
            "缓存文件名形如 index-<hash>.<ext>：{}",
            cache_file.file_name().unwrap().to_string_lossy()
        );
        assert!(
            !tmp.join("cache").join("extensions").join("index").join("repo-1").exists(),
            "不该再按仓库建目录"
        );

        // second refresh: the one-shot server is gone (connection refused),
        // so refresh_one must fall back to the cached copy instead of failing.
        let n = svc
            .refresh_one(&format!("http://{addr}/repo-1/index.json"), "t")
            .await
            .expect("cached refresh");
        assert_eq!(n, 1, "offline refresh served from cache");

        let count: i64 = suwayomi_db::query_scalar(
            "SELECT COUNT(*) FROM suwayomi.extension WHERE pkg_name = 'tachiyomi-all.nhentaicom'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(count, 1, "upsert served from cache");
        let _ = std::fs::remove_dir_all(&tmp);
    }
    /// 旧版 Tachiyomi 仓库（裸数组 + `code`/`version` + 数字 `nsfw` + 相对 apk 路径）。
    /// 样本取自 stevenyomi/copymanga 的 index.min.json（issue #8）。
    #[test]
    fn parses_legacy_tachiyomi_index_json() {
        let sample = r#"[
          {
            "name": "Tachiyomi: CopyManga",
            "pkg": "eu.kanade.tachiyomi.extension.zh.copymanga",
            "apk": "tachiyomi-zh.copymanga-v1.4.53.apk",
            "lang": "zh",
            "code": 53,
            "version": "1.4.53",
            "nsfw": 1,
            "sources": [
              { "id": "6696312508930833206", "lang": "zh", "name": "拷贝漫画", "baseUrl": "https://www.mangacopy.com" }
            ]
          }
        ]"#;
        let url = "https://raw.githubusercontent.com/stevenyomi/copymanga/repo/index.min.json";
        let entries = parse_index(sample.as_bytes(), url).expect("legacy index parses");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].version_name, "1.4.53", "`version` 要认成 versionName");
        assert_eq!(entries[0].version_code, 53, "`code` 要认成 versionCode");
        assert!(entries[0].nsfw, "数字 1 要认成 true");
        assert_eq!(entries[0].lang, "zh");
        assert_eq!(entries[0].sources[0].lang, "zh");
        assert_eq!(entries[0].sources[0].id, "6696312508930833206");
        // 相对路径补成 <index 目录>/apk/<文件名>（旧版仓库的固定布局；只拼 index
        // 目录会 404，实测 .../repo/apk/xxx.apk 才是 200）
        assert_eq!(
            entries[0].apk.as_deref(),
            Some("https://raw.githubusercontent.com/stevenyomi/copymanga/repo/apk/tachiyomi-zh.copymanga-v1.4.53.apk")
        );

        // 旧版条目没有 icon 字段 → 按仓库约定补 <index 目录>/icon/<pkg>.png
        assert_eq!(
            entries[0].icon.as_deref(),
            Some("https://raw.githubusercontent.com/stevenyomi/copymanga/repo/icon/eu.kanade.tachiyomi.extension.zh.copymanga.png")
        );

        // 同一个数组格式但用新版写法的条目（versionName/versionCode/布尔 nsfw/
        // 绝对 URL）照样读；它也没 icon，同样按约定补
        let modern = r#"[{"name":"n","pkg":"p","apk":"https://x/a.apk","lang":"en","versionName":"1.0","versionCode":3,"nsfw":false}]"#;
        let entries = parse_index(modern.as_bytes(), url).expect("modern index parses");
        assert_eq!(entries[0].version_name, "1.0");
        assert_eq!(entries[0].version_code, 3);
        assert!(!entries[0].nsfw);
        assert_eq!(entries[0].apk.as_deref(), Some("https://x/a.apk"));
        assert_eq!(
            entries[0].icon.as_deref(),
            Some("https://raw.githubusercontent.com/stevenyomi/copymanga/repo/icon/p.png")
        );

        // 文件名按字段各自的子目录补；已经带了目录的（`apk/x.apk`）不重复拼
        assert_eq!(resolve_asset_url("https://h/repo/", "apk", "x.apk"), "https://h/repo/apk/x.apk");
        assert_eq!(resolve_asset_url("https://h/repo/", "apk", "apk/x.apk"), "https://h/repo/apk/x.apk");
        assert_eq!(resolve_asset_url("https://h/repo/", "icon", "p.png"), "https://h/repo/icon/p.png");
        assert_eq!(resolve_asset_url("https://h/repo/", "apk", "https://x/a.apk"), "https://x/a.apk");
    }

    /// 新版 JSON（keiyoushi `index.json`）的 v2 对象形状：没有 `lang`、没有 `nsfw`，
    /// NSFW 走 `contentWarning` 枚举，资源是绝对 URL。样本截自真实条目。
    #[test]
    fn parses_v2_index_json() {
        let sample = r#"{
          "name": "Keiyoushi",
          "extensionList": {
            "extensions": [
              {
                "name": "AKuma",
                "packageName": "eu.kanade.tachiyomi.extension.all.akuma",
                "resources": { "apkUrl": "https://h/rel/tachiyomi-all.akuma-v1.4.10.apk", "iconUrl": "https://h/icon.png" },
                "extensionLib": "1.6",
                "versionCode": "106004",
                "versionName": "1.4.10",
                "contentWarning": "CONTENT_WARNING_NSFW",
                "sources": [ { "id": "1", "name": "AKuma", "language": "ca", "homeUrl": "https://akuma.moe" } ]
              },
              {
                "name": "Safe",
                "packageName": "eu.kanade.tachiyomi.extension.en.safe",
                "resources": { "apkUrl": "https://h/rel/tachiyomi-en.safe-v1.0.0.apk" },
                "versionCode": "1",
                "versionName": "1.0.0",
                "contentWarning": "CONTENT_WARNING_SAFE",
                "sources": [ { "id": "2", "name": "Safe", "language": "en" } ]
              },
              {
                "name": "Mixed",
                "packageName": "eu.kanade.tachiyomi.extension.all.mixed",
                "resources": { "apkUrl": "https://h/rel/tachiyomi-all.mixed-v2.0.0.apk" },
                "versionCode": 200,
                "versionName": "2.0.0",
                "contentWarning": "CONTENT_WARNING_MIXED",
                "sources": []
              }
            ]
          }
        }"#;
        let url = "https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.json";
        let entries = parse_index(sample.as_bytes(), url).expect("v2 index parses");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].lang, "all", "lang 从 apk 名里取（源语言里有 27 种，取第一条是错的）");
        assert_eq!(entries[0].version_code, 106004, "versionCode 是字符串");
        assert!(entries[0].nsfw, "CONTENT_WARNING_NSFW 算 NSFW");
        assert_eq!(entries[0].apk.as_deref(), Some("https://h/rel/tachiyomi-all.akuma-v1.4.10.apk"));
        assert_eq!(entries[0].icon.as_deref(), Some("https://h/icon.png"), "v2 给绝对 icon，不按旧版约定补");
        assert!(!entries[1].nsfw, "CONTENT_WARNING_SAFE 不算");
        assert_eq!(entries[1].lang, "en");
        assert!(entries[2].nsfw, "MIXED 也算（与 .pb 的 ContentWarning >= 2 一致）");
        assert_eq!(entries[2].version_code, 200, "数字 versionCode 也认");

        assert_eq!(lang_from_apk_name("https://h/rel/tachiyomi-zh.copymanga-v1.4.53.apk").as_deref(), Some("zh"));
        assert_eq!(lang_from_apk_name("tachiyomi-all.akuma-v1.4.10.apk").as_deref(), Some("all"));
        assert_eq!(lang_from_apk_name("weird.apk"), None);
    }

    /// 仓库地址可能是根目录、也可能是索引文件本身（含旧版的 `index.min.json`）。
    #[test]
    fn normalize_index_url_accepts_index_files() {
        assert_eq!(normalize_index_url("https://h/repo"), "https://h/repo/index.json");
        assert_eq!(normalize_index_url("https://h/repo/"), "https://h/repo/index.json");
        assert_eq!(normalize_index_url("https://h/repo/index.json"), "https://h/repo/index.json");
        assert_eq!(normalize_index_url("https://h/repo/index.pb"), "https://h/repo/index.pb");
        // 旧版仓库的索引就叫 index.min.json，不能再往后拼 /index.json
        assert_eq!(
            normalize_index_url("https://raw.githubusercontent.com/stevenyomi/copymanga/repo/index.min.json"),
            "https://raw.githubusercontent.com/stevenyomi/copymanga/repo/index.min.json"
        );
        assert_eq!(normalize_index_url("  https://h/repo/index.min.json  "), "https://h/repo/index.min.json");
    }

    /// 索引缓存是平铺的 `index-<hash>.<ext>`：同一个 URL 稳定，扩展名跟协议走。
    #[test]
    fn index_cache_path_is_flat_and_stable() {
        let tmp = tmp_root();
        let cache = tmp.join("cache");
        let json_url = "https://raw.githubusercontent.com/stevenyomi/copymanga/repo/index.min.json";
        let a = index_cache_path(&cache, json_url);
        let b = index_cache_path(&cache, json_url);
        assert_eq!(a, b, "同一个 URL 的缓存路径必须稳定");
        assert_eq!(a.parent().unwrap().file_name().unwrap(), "index");
        assert_eq!(a.extension().unwrap(), "json");
        assert!(a.file_name().unwrap().to_str().unwrap().starts_with("index-"), "{}", a.display());

        let pb = index_cache_path(&cache, "https://github.com/keiyoushi/extensions/raw/repo/index.pb");
        assert_eq!(pb.extension().unwrap(), "pb");
        assert_ne!(a, pb, "不同仓库不共用一个缓存文件");

        // 老布局留下的 <repo>/ 目录会被清掉
        let legacy = cache.join("extensions").join("index").join("copymanga");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("index.json"), b"[]").unwrap();
        prune_legacy_index_dirs(a.parent().unwrap());
        assert!(!legacy.exists(), "老布局的仓库目录应被清掉");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

// ----------------------------------------------------------------------
// Mihon 扩展仓库协议（index.pb，gzip+protobuf；字段号同 mihon
// NetworkExtensionStore）：Index 1=name 2=badge 3=signingKey 101=extensionList；
// Extension 1=name 2=packageName 5=versionCode 6=versionName 8=Source；
// Resources 1=apkUrl 2=iconUrl；Source 1=id 2=name 3=language 4=homeUrl
// ----------------------------------------------------------------------

/// protobuf 解析的错误类型（避免与 crate 的单参数 Result 别名冲突）
type PbResult<T> = std::result::Result<T, String>;

/// 极简 protobuf wire-format 读取器（只读不解码，够用且零依赖）。
struct PbReader<'a> {
    d: &'a [u8],
    i: usize,
}

impl<'a> PbReader<'a> {
    fn varint(&mut self) -> PbResult<u64> {
        let mut r = 0u64;
        let mut shift = 0u32;
        loop {
            let b = *self.d.get(self.i).ok_or("protobuf: unexpected eof")?;
            self.i += 1;
            r |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(r);
            }
            shift += 7;
            if shift >= 64 {
                return Err("protobuf: varint overflow".into());
            }
        }
    }

    /// 返回 (field_number, wire_type)
    fn key(&mut self) -> PbResult<(u64, u64)> {
        let k = self.varint()?;
        Ok((k >> 3, k & 7))
    }

    fn skip(&mut self, wt: u64) -> PbResult<()> {
        match wt {
            0 => {
                self.varint()?;
            }
            1 => self.i += 8,
            2 => {
                let len = self.varint()? as usize;
                self.i += len;
            }
            5 => self.i += 4,
            _ => return Err(format!("protobuf: unsupported wire type {wt}")),
        }
        Ok(())
    }

    fn len_delimited(&mut self) -> PbResult<&'a [u8]> {
        let len = self.varint()? as usize;
        let end = self.i + len;
        let seg = self.d.get(self.i..end).ok_or("protobuf: len exceeds data")?;
        self.i = end;
        Ok(seg)
    }

    fn string(&mut self) -> PbResult<String> {
        Ok(String::from_utf8_lossy(self.len_delimited()?).into_owned())
    }
}

/// 解析 Mihon index.pb → 复用 Tachiyomi 的 RepoIndexEntry 结构。
fn parse_mihon_pb_index(bytes: &[u8]) -> PbResult<Vec<RepoIndexEntry>> {
    let bytes = decompress_gzip_if_needed(bytes);
    let mut p = PbReader { d: &bytes, i: 0 };
    let mut out = Vec::new();
    while p.i < bytes.len() {
        let (f, wt) = p.key()?;
        match f {
            // extensionList（101）
            101 if wt == 2 => {
                let list = p.len_delimited()?;
                let mut lp = PbReader { d: list, i: 0 };
                while lp.i < list.len() {
                    let (lf, lwt) = lp.key()?;
                    if lf == 1 && lwt == 2 {
                        if let Some(e) = parse_mihon_extension(&mut lp)? {
                            out.push(e);
                        }
                    } else {
                        lp.skip(lwt)?;
                    }
                }
            }
            _ => p.skip(wt)?,
        }
    }
    Ok(out)
}

fn parse_mihon_extension(p: &mut PbReader) -> PbResult<Option<RepoIndexEntry>> {
    let body = p.len_delimited()?;
    let mut b = PbReader { d: body, i: 0 };
    let mut name = String::new();
    let mut pkg = String::new();
    let mut apk_url: Option<String> = None;
    let mut icon_url: Option<String> = None;
    let mut version_name = String::new();
    let mut version_code = 0i64;
    let mut nsfw = false;
    let mut langs: Vec<String> = Vec::new();
    let mut sources = Vec::new();
    while b.i < body.len() {
        let (f, wt) = b.key()?;
        match f {
            1 => name = b.string()?,
            2 => pkg = b.string()?,
            3 if wt == 2 => {
                let res = b.len_delimited()?;
                let mut rp = PbReader { d: res, i: 0 };
                while rp.i < res.len() {
                    let (rf, rwt) = rp.key()?;
                    match (rf, rwt) {
                        (1, 2) => apk_url = Some(rp.string()?),
                        (2, 2) => icon_url = Some(rp.string()?),
                        _ => rp.skip(rwt)?,
                    }
                }
            }
            4 => {
                let _ = b.len_delimited()?; // extensionLib，忽略
            }
            5 => version_code = b.varint()? as i64,
            6 => version_name = b.string()?,
            7 => nsfw = b.varint()? >= 2, // ContentWarning: MIXED=2 / NSFW=3
            8 if wt == 2 => {
                let src = b.len_delimited()?;
                let mut sp = PbReader { d: src, i: 0 };
                let mut sname = String::new();
                let mut slang = String::new();
                let mut sid = 0i64;
                let mut sbase = String::new();
                while sp.i < src.len() {
                    let (sf, swt) = sp.key()?;
                    match sf {
                        1 => sid = sp.varint()? as i64,
                        2 => sname = sp.string()?,
                        3 => {
                            slang = sp.string()?;
                            langs.push(slang.clone());
                        }
                        4 => sbase = sp.string()?,
                        _ => sp.skip(swt)?,
                    }
                }
                sources.push(RepoSource {
                    name: sname,
                    lang: slang,
                    id: sid.to_string(),
                    base_url: sbase,
                    version_id: 0,
                    obsolete: false,
                });
            }
            _ => b.skip(wt)?,
        }
    }
    if name.is_empty() || pkg.is_empty() {
        return Ok(None);
    }
    // 语言：单源语言用该语言，多语言扩展归为 "all"（对齐 mihon 客户端）
    let lang = if langs.len() == 1 {
        langs.remove(0)
    } else if !langs.is_empty() {
        "all".to_string()
    } else {
        String::new()
    };
    Ok(Some(RepoIndexEntry {
        name,
        pkg,
        apk: apk_url,
        icon: icon_url,
        jar: None,
        lang,
        version_name,
        version_code,
        nsfw,
        obsolete: false,
        has_readme: false,
        sources,
    }))
}

#[cfg(test)]
mod mihon_pb_tests {
    use super::*;

    fn encode_varint(mut v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
        out
    }

    fn tag(field: u64, wt: u64) -> Vec<u8> {
        encode_varint((field << 3) | wt)
    }

    fn ld(field: u64, payload: &[u8]) -> Vec<u8> {
        let mut out = tag(field, 2);
        out.extend(encode_varint(payload.len() as u64));
        out.extend(payload);
        out
    }

    fn field_str(field: u64, s: &str) -> Vec<u8> {
        ld(field, s.as_bytes())
    }

    fn make_ext(name: &str, pkg: &str, ver_name: &str, ver_code: i64, cw: u64, lang: &str, apk_url: &str) -> Vec<u8> {
        let mut res = Vec::new();
        res.extend(field_str(1, apk_url));
        let mut src = Vec::new();
        // Source: f3 = language
        src.extend(tag(3, 2));
        let sn = lang.as_bytes();
        src.extend(encode_varint(sn.len() as u64));
        src.extend(sn);
        let mut e = Vec::new();
        e.extend(field_str(1, name));
        e.extend(field_str(2, pkg));
        e.extend(ld(3, &res));
        e.extend(tag(5, 0));
        e.extend(encode_varint(ver_code as u64));
        e.extend(field_str(6, ver_name));
        e.extend(tag(7, 0));
        e.extend(encode_varint(cw));
        e.extend(ld(8, &src));
        e
    }

    #[test]
    fn parses_mihon_index_pb() {
        let e1 = make_ext("TestSrc", "eu.kanade.tachiyomi.extension.en.testsrc", "1.2.3", 42, 1, "en", "https://x/a.apk");
        let e2 = make_ext("Multi", "eu.kanade.tachiyomi.extension.all.multi", "2.0", 7, 3, "zh", "https://x/b.apk");
        let mut list = Vec::new();
        list.extend(ld(1, &e1));
        list.extend(ld(1, &e2));
        let mut index = Vec::new();
        index.extend(field_str(1, "TestRepo"));
        index.extend(ld(101, &list));

        let mut gz = Vec::new();
        {
            use std::io::Write;
            let mut enc = flate2::write::GzEncoder::new(&mut gz, flate2::Compression::default());
            enc.write_all(&index).unwrap();
            enc.finish().unwrap();
        }

        let entries = parse_mihon_pb_index(&gz).expect("parse");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "TestSrc");
        assert_eq!(entries[0].pkg, "eu.kanade.tachiyomi.extension.en.testsrc");
        assert_eq!(entries[0].version_name, "1.2.3");
        assert_eq!(entries[0].version_code, 42);
        assert_eq!(entries[0].lang, "en");
        assert!(!entries[0].nsfw);
        assert_eq!(entries[0].apk.as_deref(), Some("https://x/a.apk"));
        assert_eq!(entries[0].sources.len(), 1);
        assert_eq!(entries[0].sources[0].lang, "en");
        assert!(entries[1].nsfw);
        assert_eq!(entries[1].lang, "zh");
    }
}
