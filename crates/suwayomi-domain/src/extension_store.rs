//! Extension store management — the online-install half of Phase 6:
//!
//! 1. `refresh_stores`  — pull each repo's `index.json` (tachiyomi repo v1
//!    format) and upsert the `extension` table (available + apkUrl).
//! 2. `install`/`update`/`uninstall` — download the APK into the extensions
//!    directory, ask the JVM sandbox to hot-reload, then register sources.
//! 3. `sync_sources` — read the sandbox `/sources` (stable ids from the
//!    extension's `Source.getId()`) and upsert the `source` table so the
//!    manga list / library can use them.
//!
//! The sandbox base URL and the extensions directory are optional: without a
//! sandbox the store still refreshes its index, but install is a no-op error.

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
                jar: e.resources.jar_url,
                lang: e.lang,
                version_name: e.version_name,
                version_code: e.version_code.as_i64(),
                nsfw: e.nsfw,
                obsolete: e.obsolete,
                has_readme: false,
                sources: e.sources,
            },
        }
    }
}

impl RepoIndex {
    fn entries(self) -> Vec<RepoIndexEntry> {
        match self {
            RepoIndex::V1(v) => v.into_iter().map(RepoEntry::into_v1).collect(),
            RepoIndex::V2 { extension_list } => extension_list
                .extensions
                .into_iter()
                .map(RepoEntry::into_v1)
                .collect(),
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
    #[serde(default)]
    nsfw: bool,
    #[serde(default)]
    obsolete: bool,
    #[serde(default)]
    sources: Vec<RepoSource>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepoResources {
    #[serde(default)]
    apk_url: Option<String>,
    #[serde(default)]
    jar_url: Option<String>,
}

/// One entry of a tachiyomi repo `index.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoIndexEntry {
    pub name: String,
    pub pkg: String,
    #[serde(default)]
    pub apk: Option<String>,
    #[serde(default)]
    pub jar: Option<String>,
    #[serde(default)]
    pub lang: String,
    pub version_name: String,
    pub version_code: i64,
    #[serde(default)]
    pub nsfw: bool,
    #[serde(default)]
    pub obsolete: bool,
    #[serde(default)]
    pub has_readme: bool,
    #[serde(default)]
    pub sources: Vec<RepoSource>,
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
}

impl ExtensionStoreService {
    pub fn new(db: Db, sandbox_base: Option<String>) -> Self {
        let extensions_dir = std::env::var("SUWAYOMI_EXTENSIONS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./extensions"));
        let mut builder = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(30))
            .read_timeout(std::time::Duration::from_secs(60));
        // reuse the sandbox outbound proxy for repo fetches / APK downloads
        if let Ok(proxy) = std::env::var("SUWAYOMI_SANDBOX_PROXY") { if !proxy.is_empty() {
                if let Ok(p) = reqwest::Proxy::all(&proxy) {
                    builder = builder.proxy(p);
                }
            }
        }
        Self {
            db,
            http: builder.build().expect("reqwest client"),
            sandbox: sandbox_base.map(HttpSandboxFetcher::new),
            extensions_dir,
        }
    }

    pub fn sandbox_available(&self) -> bool {
        self.sandbox.is_some()
    }

    pub fn extensions_dir(&self) -> &Path {
        &self.extensions_dir
    }

    // ------------------------------------------------------------------
    // Repo index refresh
    // ------------------------------------------------------------------

    /// Fetches `index.json` from every configured store and upserts the
    /// extension table. Returns the number of extensions now known.
    pub async fn refresh_stores(&self) -> Result<usize> {
        let stores: Vec<(String, String)> = sqlx::query_as(
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

    /// Fetches one repo index and upserts its entries.
    pub async fn refresh_one(&self, index_url: &str, store_name: &str) -> Result<usize> {
        let url = normalize_index_url(index_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| DomainError::Source(format!("repo fetch {index_url}: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::Source(format!(
                "repo {index_url} returned {}",
                resp.status()
            )));
        }
        let bytes = resp.bytes().await.map_err(DomainError::from)?;
        // some repos serve gzip regardless of accept-encoding
        let bytes = decompress_gzip_if_needed(&bytes);
        let index: RepoIndex =
            serde_json::from_slice(&bytes).map_err(|e| DomainError::Source(format!("repo index parse: {e}")))?;
        let entries = index.entries();
        let pool = self.db.pool();
        let mut n = 0usize;
        for e in entries {
            let content_warning = if e.nsfw { 1 } else { 0 };
            sqlx::query(
                "INSERT INTO suwayomi.extension \
                 (apk_name, store_index_url, name, pkg_name, apk_url, jar_url, version_name, version_code, lang, content_warning, is_installed, has_update, is_obsolete, class_name) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, FALSE, $12, '') \
                 ON CONFLICT (pkg_name) DO UPDATE SET \
                   apk_name = EXCLUDED.apk_name, store_index_url = EXCLUDED.store_index_url, \
                   name = EXCLUDED.name, apk_url = EXCLUDED.apk_url, jar_url = EXCLUDED.jar_url, \
                   version_name = EXCLUDED.version_name, version_code = EXCLUDED.version_code, \
                   lang = EXCLUDED.lang, content_warning = EXCLUDED.content_warning, \
                   is_obsolete = EXCLUDED.is_obsolete, \
                   has_update = (EXCLUDED.version_code > suwayomi.extension.version_code AND suwayomi.extension.is_installed)",
            )
            .bind(e.apk.as_deref().map(apk_file_name))
            .bind(index_url)
            .bind(&e.name)
            .bind(&e.pkg)
            .bind(&e.apk)
            .bind(&e.jar)
            .bind(&e.version_name)
            .bind(e.version_code)
            .bind(&e.lang)
            .bind(content_warning)
            .bind(e.is_installed_for_local_dir(&self.extensions_dir))
            .bind(e.obsolete)
            .execute(pool)
            .await
            .map_err(|err| DomainError::Source(format!("extension upsert {}: {err}", e.pkg)))?;
            n += 1;
        }
        let _ = store_name;
        Ok(n)
    }

    // ------------------------------------------------------------------
    // Install / update / uninstall
    // ------------------------------------------------------------------

    /// Downloads and installs (or updates) the extension identified by pkg.
    pub async fn install(&self, pkg: &str) -> Result<()> {
        let fetcher = self.require_sandbox()?;
        let row: Option<InstallRow> = sqlx::query_as(
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
        sqlx::query(
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
        sqlx::query(
            "DELETE FROM suwayomi.source WHERE extension = (SELECT id FROM suwayomi.extension WHERE pkg_name = $1)",
        )
        .bind(pkg)
        .execute(self.db.pool())
        .await?;
        remove_matching_apks(&self.extensions_dir, pkg)?;
        fetcher.reload().await?;
        sqlx::query("UPDATE suwayomi.extension SET is_installed = FALSE, has_update = FALSE WHERE pkg_name = $1")
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
        let file_name = format!("tachiyomi-{}.{}-v{}.apk", meta.lang, meta.pkg_name, meta.version_name);
        let target = self.extensions_dir.join(&file_name);
        std::fs::write(&target, apk).map_err(|e| DomainError::Source(format!("write {file_name}: {e}")))?;
        fetcher.reload().await?;
        self.sync_sources().await?;
        sqlx::query(
            "INSERT INTO suwayomi.extension \
             (apk_name, name, pkg_name, version_name, version_code, lang, content_warning, is_installed, class_name) \
             VALUES ($1, $2, $3, $4, 0, $5, 0, TRUE, $6) \
             ON CONFLICT (pkg_name) DO UPDATE SET apk_name = EXCLUDED.apk_name, name = EXCLUDED.name, \
               version_name = EXCLUDED.version_name, lang = EXCLUDED.lang, is_installed = TRUE, class_name = EXCLUDED.class_name",
        )
        .bind(&file_name)
        .bind(&meta.name)
        .bind(&meta.pkg_name)
        .bind(&meta.version_name)
        .bind(&meta.lang)
        .bind(&meta.class_name)
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
        let mut n = 0usize;

        // The sandbox reports each extension together with the sources it
        // provides, so the pkg link is unambiguous here.
        for e in &exts {
            for s in &e.sources {
                sqlx::query(
                    "INSERT INTO suwayomi.source (id, name, lang, extension) VALUES ($1, $2, $3, \
                     (SELECT id FROM suwayomi.extension WHERE pkg_name = $4)) \
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, lang = EXCLUDED.lang, \
                       extension = EXCLUDED.extension",
                )
                .bind(s.id)
                .bind(&s.name)
                .bind(&s.lang)
                .bind(&e.pkg_name)
                .execute(pool)
                .await?;
                n += 1;
            }
        }

        // refresh class_name/version for loaded extensions
        for e in &exts {
            sqlx::query(
                "UPDATE suwayomi.extension SET class_name = $1, version_name = $2 WHERE pkg_name = $3 AND is_installed",
            )
            .bind(&e.class_name)
            .bind(&e.version_name)
            .bind(&e.pkg_name)
            .execute(pool)
            .await?;
        }
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
        if let Some(n) = name {
            if dir.join(n).exists() {
                return true;
            }
        }
        // fall back: any file named tachiyomi-{lang}.{pkg}* in the dir
        let prefix = format!("{}.{}", self.lang, self.pkg);
        if let Ok(rd) = std::fs::read_dir(dir) {
            for f in rd.flatten() {
                if let Some(fn_) = f.file_name().to_str() {
                    if fn_.starts_with(&prefix) && fn_.ends_with(".apk") {
                        return true;
                    }
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

fn normalize_index_url(url: &str) -> String {
    if url.ends_with("index.json") {
        url.to_string()
    } else {
        format!("{}/index.json", url.trim_end_matches('/'))
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

    /// These tests write to `extension_store` / `extension`, which the
    /// embedded pglite build cannot handle (INSERT causes the proxy to drop
    /// the session). They run against external PostgreSQL via DATABASE_URL;
    /// skipped otherwise (same convention as the version-bump trigger test).
    async fn setup_db() -> Option<Db> {
        let url = std::env::var("DATABASE_URL").ok().filter(|u| !u.is_empty())?;
        let db = suwayomi_core::db::Db::connect(&url).await.expect("db");
        db.migrate().await.expect("migrate");
        // clear extension-related tables so tests are repeatable
        let _ = sqlx::query("TRUNCATE suwayomi.source, suwayomi.extension, suwayomi.extension_store CASCADE")
            .execute(db.pool()).await;
        Some(db)
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
        sqlx::query("INSERT INTO suwayomi.extension_store (index_url, name, badge_label, signing_key, contact_website) VALUES ('http://127.0.0.1:1/repo-1', 't', '', '', '')")
            .execute(db.pool()).await.unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let index = br#"[{"name":"nhentai.com","pkg":"tachiyomi-all.nhentaicom","apk":"http://127.0.0.1:1/dl.apk","lang":"all","versionName":"1.4.10","versionCode":14,"nsfw":true,"sources":[{"name":"nhentai.com","lang":"en","id":"5591830863732393712"}]},{"name":"MangaDex","pkg":"tachiyomi-all.mangadex","apk":"http://127.0.0.1:1/md.apk","lang":"all","versionName":"1.2.3","versionCode":9,"nsfw":false}]"#;
        let _srv = serve_once(listener, index, "HTTP/1.1 200 OK");

        let svc = ExtensionStoreService::new(db.clone(), None);
        let n = svc.refresh_one(&format!("http://{addr}/index.json"), "t").await.expect("refresh");
        assert_eq!(n, 2, "two extensions upserted");

        let (name, apk_url, vc, cw, inst): (String, Option<String>, i64, i32, bool) = sqlx::query_as(
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
        let tmp = std::env::temp_dir().join(format!("ext-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("SUWAYOMI_EXTENSIONS_DIR", &tmp);

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

        sqlx::query(
            "INSERT INTO suwayomi.extension (name, pkg_name, apk_url, version_name, version_code, lang, content_warning) \
             VALUES ('nhentai.com', 'tachiyomi-all.nhentaicom', $1, '1.4.10', 14, 'all', 1)",
        )
        .bind(format!("http://{dl_addr}/tachiyomi-all.nhentaicom-v1.4.10.apk"))
        .execute(db.pool()).await.unwrap();

        let svc = ExtensionStoreService::new(db.clone(), Some(format!("http://{sb_addr}")));
        svc.install("tachiyomi-all.nhentaicom").await.expect("install");

        let files: Vec<String> = std::fs::read_dir(&tmp)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            files.iter().any(|f| f.contains("tachiyomi-all.nhentaicom") && f.ends_with(".apk")),
            "apk persisted: {files:?}"
        );
        let (sid, sname, slang): (i64, String, String) = sqlx::query_as(
            "SELECT id, name, lang FROM suwayomi.source WHERE id = 5591830863732393712",
        )
        .fetch_one(db.pool()).await.unwrap();
        assert_eq!(sid, 5591830863732393712);
        assert_eq!(sname, "nhentai.com");
        assert_eq!(slang, "en");
        let inst: bool = sqlx::query_scalar("SELECT is_installed FROM suwayomi.extension WHERE pkg_name = 'tachiyomi-all.nhentaicom'")
            .fetch_one(db.pool()).await.unwrap();
        assert!(inst, "extension marked installed");

        sb_srv.await.unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn uninstall_removes_apk_and_sources() {
        let Some(db) = setup_db().await else { eprintln!("SKIP: requires DATABASE_URL"); return };
        let tmp = std::env::temp_dir().join(format!("ext-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("SUWAYOMI_EXTENSIONS_DIR", &tmp);
        std::fs::write(tmp.join("tachiyomi-all.mangadex-v1.2.3.apk"), b"PK fake").unwrap();

        sqlx::query("INSERT INTO suwayomi.extension (name, pkg_name, version_name, version_code, lang, content_warning, is_installed) \
                     VALUES ('mangadex.org', 'tachiyomi-all.mangadex', '1.2.3', 9, 'all', 0, TRUE)")
            .execute(db.pool()).await.unwrap();
        let eid: i32 = sqlx::query_scalar("SELECT id FROM suwayomi.extension WHERE pkg_name = 'tachiyomi-all.mangadex'")
            .fetch_one(db.pool()).await.unwrap();
        sqlx::query("INSERT INTO suwayomi.source (id, name, lang, extension) VALUES (4422762036021677666, 'mangadex.org', 'en', $1)")
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

        let svc = ExtensionStoreService::new(db.clone(), Some(format!("http://{sb_addr}")));
        svc.uninstall("tachiyomi-all.mangadex").await.expect("uninstall");

        let remaining: Vec<String> = std::fs::read_dir(&tmp)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(remaining.is_empty(), "apk removed: {remaining:?}");
        let src_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM suwayomi.source WHERE id = 4422762036021677666")
            .fetch_one(db.pool()).await.unwrap();
        assert_eq!(src_count, 0);
        let inst: bool = sqlx::query_scalar("SELECT is_installed FROM suwayomi.extension WHERE pkg_name = 'tachiyomi-all.mangadex'")
            .fetch_one(db.pool()).await.unwrap();
        assert!(!inst, "marked uninstalled");

        sb_srv.await.unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
