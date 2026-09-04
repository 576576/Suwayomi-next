//! 本地图源扫描（对应 Tachidesk LocalSource）。目录布局：
//!   data/local/<MangaTitle>/<ChapterName>/page01.jpg …（目录章节）
//!   data/local/<MangaTitle>/ch001.zip …（归档章节）/ cover.jpg / details.json
//! details.json 支持 title/author/artist/description/genre/status 字段
//! （genre 可为数组或逗号串）；归档常带 nhentai 格式 meta.json。

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use suwayomi_core::models::UpdateStrategy;
use suwayomi_core::source::{SChapter, SManga, SourcePage};

/// Supported archive chapter extensions (case-insensitive).
pub const ARCHIVE_EXTS: &[&str] = &["zip", "cbz", "rar", "cbr", "epub"];

/// Supported page image extensions for directory chapters.
pub const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp", "avif", "heic"];

/// Process-wide override for the local source root, set from the
/// `localSourcePath` server setting (`set_settings`) or loaded at startup.
static LOCAL_ROOT_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

/// Point the local source at a custom directory (`localSourcePath` setting).
/// `None`/empty resets to the default `<cwd>/data/local`.
pub fn set_local_source_root(path: Option<PathBuf>) {
    let lock = LOCAL_ROOT_OVERRIDE.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = lock.write() {
        *guard = path.filter(|p| !p.as_os_str().is_empty());
    }
}

/// 本地图源根目录。解析顺序：localSourcePath override → SUWAYOMI_LOCAL_SOURCE_DIR
/// env（托盘 spawn 时 server cwd=data，默认会解析成 data/data/local）→ exe bin/
/// 布局的发布根 data/local → cwd/data/local
pub fn local_source_root() -> PathBuf {
    if let Some(lock) = LOCAL_ROOT_OVERRIDE.get() {
        if let Ok(guard) = lock.read() {
            if let Some(path) = guard.as_ref() {
                return path.clone();
            }
        }
    }
    if let Ok(dir) = std::env::var("SUWAYOMI_LOCAL_SOURCE_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.file_name().map(|n| n == "bin").unwrap_or(false) {
                if let Some(base) = dir.parent() {
                    return base.join("data").join("local");
                }
            }
        }
    }
    std::env::current_dir()
        .unwrap_or_default()
        .join("data")
        .join("local")
}

/// Resolve the manga folder for a local manga url (the folder name) — used by
/// chapter scanning / image serving.
pub fn local_manga_dir(root: &Path, manga_url: &str) -> Option<PathBuf> {
    let dir = root.join(manga_url);
    if dir.is_dir() {
        Some(dir)
    } else {
        None
    }
}

/// Scan the chapters of one local manga folder: image subdirectories and
/// archive files (ZIP/CBZ/RAR/CBR/EPUB). Sorted by name. Chapter names come
/// from the file/directory name on disk (never from embedded metadata);
/// archive metadata parsed from `meta.json`/`ComicInfo.xml` still supplies
/// the chapter number, scanlator and upload date.
pub fn scan_local_chapters(manga_dir: &Path) -> Vec<SChapter> {
    let mut chapters = Vec::new();
    let Ok(entries) = std::fs::read_dir(manga_dir) else {
        return chapters;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.eq_ignore_ascii_case("cover.jpg") || name == "details.json" {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
        let is_archive = ARCHIVE_EXTS.contains(&ext.as_str());
        if !is_dir && !is_archive {
            continue;
        }
        let meta = if is_archive { read_archive_meta(&path) } else { None };
        // Chapter name always comes from the on-disk name (the archive file
        // stem, e.g. `Chapter 1.zip` -> `Chapter 1`). Embedded metadata is
        // used only for number / scanlator / upload date below.
        let chapter_name = if is_archive {
            path.file_stem().and_then(|x| x.to_str()).unwrap_or(&name).to_string()
        } else {
            name.clone()
        };
        chapters.push(SChapter {
            url: name.clone(),
            name: chapter_name,
            chapter_number: meta
                .as_ref()
                .and_then(|m| m.number)
                .unwrap_or_else(|| parse_chapter_number(&name, chapters.len() as f32 + 1.0)),
            scanlator: meta.as_ref().and_then(|m| m.scanlator.clone()),
            date_upload: meta.as_ref().and_then(|m| m.upload_date).unwrap_or(0),
            memo: serde_json::Value::Null,
        });
    }
    chapters.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    chapters
}

/// Natural-order comparison: numeric runs are compared by value so that
/// `1.jpg < 2.jpg < 10.jpg` (lexicographic order would put "10" before "2",
/// which shows up as page 1 → page 10 in the reader).
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let (mut pa, mut pb) = (0usize, 0usize);
    while pa < ab.len() && pb < bb.len() {
        let (ca, cb) = (ab[pa], bb[pb]);
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let mut ea = pa;
            while ea < ab.len() && ab[ea].is_ascii_digit() {
                ea += 1;
            }
            let mut eb = pb;
            while eb < bb.len() && bb[eb].is_ascii_digit() {
                eb += 1;
            }
            // 去前导零后按（长度 → 字典序）比较数值
            let da = a[pa..ea].trim_start_matches('0');
            let db = b[pb..eb].trim_start_matches('0');
            let ord = da.len().cmp(&db.len()).then_with(|| da.cmp(db));
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
            pa = ea;
            pb = eb;
        } else {
            let ord = ca.cmp(&cb);
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
            pa += 1;
            pb += 1;
        }
    }
    ab.len().cmp(&bb.len())
}

/// Scan the page images of one chapter folder. For directory chapters this
/// lists the image files; for archive chapters (ZIP/CBZ/…) it lists the
/// images inside the archive. `url_prefix` (e.g. `local/<manga>/<chapter>`)
/// is prepended to each page's image URL.
pub fn scan_local_pages(chapter_path: &Path, url_prefix: &str) -> Vec<SourcePage> {
    let mut pages = Vec::new();
    if chapter_path.is_dir() {
        let mut files: Vec<_> = std::fs::read_dir(chapter_path)
            .map(|it| {
                it.filter_map(|e| e.ok())
                    .filter(|e| {
                        let path = e.path();
                        let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("");
                        IMAGE_EXTS.contains(&ext.to_lowercase().as_str())
                    })
                    .collect()
            })
            .unwrap_or_default();
        files.sort_by(|a, b| natural_cmp(&a.file_name().to_string_lossy(), &b.file_name().to_string_lossy()));
        for (i, f) in files.into_iter().enumerate() {
            let name = f.file_name().to_string_lossy().into_owned();
            let image_url = format!("{url_prefix}/{name}");
            pages.push(SourcePage::new(i as i32, name.clone(), Some(image_url)));
        }
    } else {
        // Archive chapter: one page per image inside the archive.
        for (i, name) in list_archive_pages(chapter_path) {
            let image_url = format!("{url_prefix}/{name}");
            pages.push(SourcePage::new(i as i32, name.clone(), Some(image_url)));
        }
    }
    pages
}

/// List image file names inside an archive (folders ignored, deduped by
/// name, sorted by name) — mirrors the Local Source wiki rule that folder
/// structure inside archives is ignored.
pub fn list_archive_pages(archive: &Path) -> Vec<(usize, String)> {
    let file = match std::fs::File::open(archive) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(z) => z,
        Err(_) => return Vec::new(),
    };
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut names = Vec::new();
    for i in 0..zip.len() {
        let Ok(entry) = zip.by_index(i) else { continue };
        let full = entry.name().to_string();
        let name = full.rsplit('/').next().unwrap_or(&full).to_string();
        if name.starts_with('.') || name.is_empty() {
            continue;
        }
        let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
        if !IMAGE_EXTS.contains(&ext.as_str()) || !seen.insert(name.clone()) {
            continue;
        }
        names.push(name);
    }
    names.sort_by(|a, b| natural_cmp(a, b));
    names.into_iter().enumerate().collect()
}

/// Extract one image (matched by its file name, folders ignored) from an
/// archive, returning the raw bytes.
pub fn read_archive_image(archive: &Path, file_name: &str) -> Option<Vec<u8>> {
    let file = std::fs::File::open(archive).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    for i in 0..zip.len() {
        let Ok(mut entry) = zip.by_index(i) else { continue };
        let full = entry.name().to_string();
        let name = full.rsplit('/').next().unwrap_or(&full);
        if name == file_name && entry.is_file() {
            let mut buf = Vec::with_capacity(entry.size() as usize);
            std::io::Read::read_to_end(&mut entry, &mut buf).ok()?;
            return Some(buf);
        }
    }
    None
}

/// Metadata parsed from an archive's `meta.json` (nhentai export / Tachiyomi
/// LocalSource) or `ComicInfo.xml` (ComicRack standard inside CBZ). The two
/// files never coexist.
#[derive(Debug, Clone, Default)]
pub struct ArchiveMeta {
    /// Chapter-level title from `ComicInfo.Title` (not used for the chapter
    /// name — the on-disk file name wins — but kept for completeness).
    pub title: Option<String>,
    /// Chapter number — `ComicInfo.Number`.
    pub number: Option<f32>,
    pub scanlator: Option<String>,
    pub upload_date: Option<i64>,
    pub page_count: Option<i32>,
    /// Manga-level title — `meta.json.title` (japanese preferred, then
    /// english) or `ComicInfo.Series`.
    pub manga_title: Option<String>,
    /// Other-language titles from `meta.json.title` (everything except the
    /// selected main title) — shown as "Alternative title: …" on the details
    /// page when more than one title is present.
    pub alt_titles: Vec<String>,
    pub author: Option<String>,
    pub artist: Option<String>,
    pub genre: Option<String>,
    /// Free-text description — `meta.json.description` or the `Description:`
    /// line inside `ComicInfo.Summary` (skipped when it's `null`/empty).
    pub description: Option<String>,
}

/// Read and parse `meta.json` / `ComicInfo.xml` from inside an archive.
pub fn read_archive_meta(archive: &Path) -> Option<ArchiveMeta> {
    let file = std::fs::File::open(archive).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    for i in 0..zip.len() {
        let Ok(mut entry) = zip.by_index(i) else { continue };
        let full = entry.name().to_string();
        let name = full.rsplit('/').next().unwrap_or(&full).to_lowercase();
        if !entry.is_file() {
            continue;
        }
        if name == "meta.json" {
            let mut buf = Vec::with_capacity(entry.size() as usize);
            std::io::Read::read_to_end(&mut entry, &mut buf).ok()?;
            return parse_meta_json(&buf);
        }
        if name == "comicinfo.xml" {
            let mut buf = Vec::with_capacity(entry.size() as usize);
            std::io::Read::read_to_end(&mut entry, &mut buf).ok()?;
            return parse_comic_info_xml(&buf);
        }
    }
    None
}

/// Scan the manga-level metadata contributed by the first archive chapter
/// that carries `meta.json`/`ComicInfo.xml` (used to enrich a manga that has
/// no `details.json`).
fn scan_manga_archive_meta(manga_dir: &Path) -> Option<ArchiveMeta> {
    let entries = std::fs::read_dir(manga_dir).ok()?;
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
            if ARCHIVE_EXTS.contains(&ext.as_str()) {
                if let Some(meta) = read_archive_meta(&path) {
                    if meta.manga_title.is_some() || meta.artist.is_some() || meta.genre.is_some() {
                        return Some(meta);
                    }
                }
            }
        }
    }
    None
}

/// 解析 nhentai/导出用的 `meta.json`：
/// ```json
/// { "title": {"english": "…", "japanese": "…"}, "upload_date": 1594958203,
///   "num_pages": 24, "tags": [{"type": "artist|…|character", "name": "…"}] }
/// ```
fn parse_meta_json(bytes: &[u8]) -> Option<ArchiveMeta> {
    // 任意语言键都接受；serde(flatten) 保留未知键不丢
    #[derive(serde::Deserialize, Default)]
    struct MetaJsonTitle {
        #[serde(default)]
        english: Option<String>,
        #[serde(default)]
        japanese: Option<String>,
        #[serde(flatten)]
        extra: std::collections::HashMap<String, String>,
    }
    #[derive(serde::Deserialize, Default)]
    struct MetaJsonTag {
        #[serde(default, rename = "type")]
        r#type: Option<String>,
        #[serde(default)]
        name: Option<String>,
    }
    #[derive(serde::Deserialize, Default)]
    struct MetaJson {
        #[serde(default)]
        title: Option<MetaJsonTitle>,
        #[serde(default)]
        upload_date: Option<i64>,
        #[serde(default)]
        num_pages: Option<i64>,
        #[serde(default)]
        scanlator: Option<String>,
        #[serde(default)]
        tags: Option<Vec<MetaJsonTag>>,
    }

    let m: MetaJson = serde_json::from_slice(bytes).ok()?;
    // Title ordering — english is ALWAYS the fallback, never preferred:
    //   main title:  japanese > other non-english langs (sorted) > english
    //   alternatives: every remaining title, english last.
    let mut langs: Vec<String> = Vec::new(); // (title) in preference order
    if let Some(t) = m.title.as_ref() {
        let push = |langs: &mut Vec<String>, s: &Option<String>| {
            if let Some(s) = s.as_deref().filter(|s| !s.trim().is_empty()) {
                langs.push(s.to_string());
            }
        };
        push(&mut langs, &t.japanese);
        let mut extras: Vec<(&String, &String)> =
            t.extra.iter().filter(|(_, v)| !v.trim().is_empty()).collect();
        extras.sort_by(|a, b| a.0.cmp(b.0));
        for (k, v) in extras {
            if k != "english" {
                langs.push(v.clone());
            }
        }
        push(&mut langs, &t.english);
    }
    let mut alt_titles: Vec<String> = Vec::new();
    let manga_title = langs.first().cloned();
    if let Some(main) = &manga_title {
        alt_titles = langs.iter().filter(|t| *t != main).cloned().collect();
    }
    let mut artist = Vec::new();
    let mut author = Vec::new();
    let mut genre = Vec::new();
    if let Some(tags) = &m.tags {
        for tag in tags {
            let Some(name) = tag.name.as_deref().filter(|n| !n.trim().is_empty()) else {
                continue;
            };
            match tag.r#type.as_deref() {
                Some("artist") => artist.push(name.to_string()),
                Some("group") | Some("circle") => author.push(name.to_string()),
                Some("parody") | Some("character") | Some("category") => genre.push(name.to_string()),
                _ => genre.push(name.to_string()),
            }
        }
    }
    Some(ArchiveMeta {
        // meta.json describes the whole work; the chapter keeps the file
        // name, the title goes to the manga level.
        title: manga_title.clone(),
        number: None,
        scanlator: m.scanlator.filter(|s| !s.trim().is_empty()),
        upload_date: m.upload_date,
        page_count: m.num_pages.map(|n| n as i32),
        manga_title,
        alt_titles,
        author: if author.is_empty() { None } else { Some(author.join(", ")) },
        artist: if artist.is_empty() { None } else { Some(artist.join(", ")) },
        genre: if genre.is_empty() { None } else { Some(genre.join(", ")) },
        description: None,
    })
}

/// Parse `ComicInfo.xml` — the ComicRack metadata standard used inside CBZ
/// files. Only the commonly-present fields are extracted.
fn parse_comic_info_xml(bytes: &[u8]) -> Option<ArchiveMeta> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut fields: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut current: Option<String> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                current = Some(String::from_utf8_lossy(e.name().as_ref()).to_lowercase());
            }
            Ok(Event::Text(t)) => {
                if let Some(tag) = &current {
                    let text = t.unescape().unwrap_or_default().trim().to_string();
                    if !text.is_empty() {
                        fields.entry(tag.clone()).or_insert(text);
                    }
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        buf.clear();
    }

    let text = |k: &str| fields.get(k).cloned().filter(|v| !v.trim().is_empty());
    let title = text("title");
    let series = text("series");
    let manga_title = series.or_else(|| title.clone());
    let number = text("number").and_then(|n| n.trim().parse::<f32>().ok());
    let page_count = text("pagecount").and_then(|n| n.trim().parse::<i32>().ok());
    let upload_date = text("publicationdate").and_then(|d| parse_date_str(&d));
    let mut genre_parts: Vec<String> = Vec::new();
    if let Some(g) = text("genre") {
        genre_parts.extend(g.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    }
    if let Some(t) = text("tags") {
        genre_parts.extend(t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    }
    genre_parts.dedup();
    let genre = if genre_parts.is_empty() { None } else { Some(genre_parts.join(", ")) };

    // ComicInfo.Summary 是多行块（Alternative Title / Groups / Description /
    // Pages / Category …）：整体作漫画简介渲染，另抽出 Alternative Title 行
    let mut alt_titles: Vec<String> = Vec::new();
    if let Some(summary) = text("summary") {
        for line in summary.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("Alternative Title:") {
                let v = v.trim();
                if !v.is_empty() {
                    alt_titles.push(v.to_string());
                }
            }
        }
        alt_titles.dedup();
    }
    // Description: full Summary body (trimmed, multi-line). Drop the literal
    // `Description: null` line if present; drop the leading blank line that
    // follows the header so the rendering reads cleanly.
    let description = text("summary").map(|raw| {
        let mut lines: Vec<&str> = Vec::new();
        for line in raw.lines() {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            if t.eq_ignore_ascii_case("Description: null") {
                continue;
            }
            lines.push(line);
        }
        lines.join("\n").trim().to_string()
    });

    Some(ArchiveMeta {
        title,
        number,
        scanlator: text("scaninformation"),
        upload_date,
        page_count,
        manga_title,
        alt_titles,
        author: text("writer"),
        artist: text("penciller"),
        genre,
        description,
    })
}

/// Parse `YYYY-MM-DD` (and `YYYY-MM-DDTHH:MM:SS`) date strings to epoch
/// seconds (UTC). Used for `ComicInfo.PublicationDate`.
fn parse_date_str(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.len() >= 10 {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&s[..10], "%Y-%m-%d") {
            return Some(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp());
        }
    }
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.timestamp())
}

/// Extract a numeric chapter number from a chapter name ("Chapter 1",
/// "ch001", "01 - cover", …). Falls back to `fallback`.
fn parse_chapter_number(name: &str, fallback: f32) -> f32 {
    let mut best: Option<f32> = None;
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let num: f32 = name[start..i].parse().unwrap_or(0.0);
            if best.is_none() || num < best.unwrap_or(f32::MAX) {
                best = Some(num);
            }
        } else {
            i += 1;
        }
    }
    best.filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(fallback)
}

/// Scan `local/` and produce the SManga list (one entry per subdirectory).
pub fn scan_local_source(root: &Path) -> Vec<SManga> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    dirs.sort_by(|a, b| natural_cmp(&a.file_name().to_string_lossy(), &b.file_name().to_string_lossy()));

    let mut out = Vec::with_capacity(dirs.len());
    for entry in dirs {
        let dir = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let details = read_details(&dir.join("details.json"));
        // Enrich from archive metadata (meta.json/ComicInfo.xml) when the
        // manga folder has no details.json.
        let archive_meta = if details.is_none() { scan_manga_archive_meta(&dir) } else { None };
        let title = details
            .as_ref()
            .and_then(|d| d.title.clone())
            .filter(|t| !t.trim().is_empty())
            .or_else(|| archive_meta.as_ref().and_then(|m| m.manga_title.clone()))
            .unwrap_or_else(|| name.clone());
        let cover = dir.join("cover.jpg");
        // 根相对路径（前导斜杠）：WebUI getValidImgUrlFor 拼接 `baseUrl + thumbnailUrl`，
        // 无前导斜杠会拼出 `http://hostlocal/...`（缺 /）导致封面加载失败；
        // 与阅读器 pages 的 `/local/...` 修复保持一致。
        let thumbnail_url = cover.is_file().then(|| format!("/local/{name}/cover.jpg"));
        let status = details
            .as_ref()
            .and_then(|d| d.status.as_deref())
            .and_then(|s| s.trim().parse::<i32>().ok())
            .unwrap_or(SManga::UNKNOWN);
        let genre = details
            .as_ref()
            .and_then(|d| d.genre.as_ref())
            .map(|g| match g {
                GenreValue::List(v) => v.join(", "),
                GenreValue::Single(s) => s.clone(),
            })
            .or_else(|| archive_meta.as_ref().and_then(|m| m.genre.clone()));
        out.push(SManga {
            url: name.clone(),
            title,
            thumbnail_url,
            artist: details
                .as_ref()
                .and_then(|d| d.artist.clone())
                .or_else(|| archive_meta.as_ref().and_then(|m| m.artist.clone())),
            author: details
                .as_ref()
                .and_then(|d| d.author.clone())
                .or_else(|| archive_meta.as_ref().and_then(|m| m.author.clone())),
            status,
            description: details.as_ref().and_then(|d| d.description.clone()),
            genre,
            alt_titles: archive_meta.as_ref().map(|m| m.alt_titles.clone()).unwrap_or_default(),
            update_strategy: UpdateStrategy::default(),
            initialized: true,
            memo: serde_json::Value::Null,
        });
    }
    out
}

#[derive(serde::Deserialize, Default)]
struct DetailsJson {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    genre: Option<GenreValue>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum GenreValue {
    List(Vec<String>),
    Single(String),
}

fn read_details(path: &Path) -> Option<DetailsJson> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn natural_sort_orders_pages_numerically() {
        // 回归：字典序会把 10 排在 2 前面（阅读器第 1 页后接第 10 页）
        let mut names = vec!["10.jpg", "1.jpg", "2.jpg", "11.jpg", "20.jpg", "3.jpg"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["1.jpg", "2.jpg", "3.jpg", "10.jpg", "11.jpg", "20.jpg"]);

        let mut chapters = vec!["Chapter 10", "Chapter 2", "Chapter 1"];
        chapters.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(chapters, ["Chapter 1", "Chapter 2", "Chapter 10"]);

        // 混合：非数字前缀按字典序、数字段按数值
        let mut mixed = vec!["b-2.jpg", "a-10.jpg", "a-2.jpg", "b-1.jpg"];
        mixed.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(mixed, ["a-2.jpg", "a-10.jpg", "b-1.jpg", "b-2.jpg"]);
    }

    #[test]
    fn scans_directories_and_details() {
        let tmp = std::env::temp_dir().join(format!("local-src-test-{}", std::process::id()));
        let manga = tmp.join("My Manga");
        std::fs::create_dir_all(&manga).unwrap();
        std::fs::write(manga.join("cover.jpg"), b"jpeg").unwrap();
        std::fs::write(
            manga.join("details.json"),
            r#"{"title":"T","author":"A","artist":"Ar","description":"D","genre":["g1","g2"],"status":"2"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(manga.join("ch01")).unwrap();
        std::fs::write(tmp.join(".hidden"), b"x").unwrap();

        let mangas = scan_local_source(&tmp);
        assert_eq!(mangas.len(), 1);
        assert_eq!(mangas[0].title, "T");
        assert_eq!(mangas[0].author.as_deref(), Some("A"));
        assert_eq!(mangas[0].genre.as_deref(), Some("g1, g2"));
        assert_eq!(mangas[0].status, 2);
        assert_eq!(mangas[0].thumbnail_url.as_deref(), Some("/local/My Manga/cover.jpg"));
        assert!(local_manga_dir(&tmp, "My Manga").is_some());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn parses_meta_json_and_applies_to_chapter() {
        use zip::write::SimpleFileOptions;
        let tmp = std::env::temp_dir().join(format!("local-meta-test-{}", std::process::id()));
        let manga = tmp.join("M");
        std::fs::create_dir_all(&manga).unwrap();
        let zip_path = manga.join("ch001.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zw = zip::ZipWriter::new(file);
            let opts = SimpleFileOptions::default();
            zw.start_file("1.jpg", opts).unwrap();
            zw.write_all(&[0xFF, 0xD8, 0xFF]).unwrap();
            zw.start_file(
                "meta.json",
                opts,
            )
            .unwrap();
            zw.write_all(
                br#"{"title":{"english":"Work EN","japanese":"Work JP"},"upload_date":1594958203,"num_pages":24,"scanlator":"X","tags":[{"type":"artist","name":"Pochi"},{"type":"group","name":"Circle"},{"type":"parody","name":"Series"},{"type":"character","name":"Chizuru"}]}"#,
            )
            .unwrap();
            zw.finish().unwrap();
        }

        let meta = read_archive_meta(&zip_path).expect("meta.json parsed");
        // Manga title prefers japanese over english.
        assert_eq!(meta.manga_title.as_deref(), Some("Work JP"));
        assert_eq!(meta.artist.as_deref(), Some("Pochi"));
        assert_eq!(meta.author.as_deref(), Some("Circle"));
        assert_eq!(meta.upload_date, Some(1594958203));
        assert_eq!(meta.page_count, Some(24));
        assert_eq!(meta.scanlator.as_deref(), Some("X"));
        assert!(meta.genre.as_deref().unwrap().contains("Series"));

        let chapters = scan_local_chapters(&manga);
        assert_eq!(chapters.len(), 1);
        // Chapter names come from the on-disk archive file name, never from
        // embedded metadata (meta.json only enriches number/date/scanlator).
        assert_eq!(chapters[0].name, "ch001");
        assert_eq!(chapters[0].scanlator.as_deref(), Some("X"));
        assert_eq!(chapters[0].date_upload, 1594958203);

        // manga-level enrichment when no details.json exists
        let mangas = scan_local_source(&tmp);
        assert_eq!(mangas[0].title, "Work JP");
        assert_eq!(mangas[0].artist.as_deref(), Some("Pochi"));
        assert!(mangas[0].genre.as_deref().unwrap().contains("Series"));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn parses_comic_info_xml() {
        let xml = br#"<?xml version="1.0"?>
<ComicInfo xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Title>Chapter 5</Title>
  <Series>My Series</Series>
  <Number>5</Number>
  <Writer>W Author</Writer>
  <Penciller>P Artist</Penciller>
  <Genre>Action, Drama</Genre>
  <Tags>Romance</Tags>
  <PageCount>20</PageCount>
  <PublicationDate>2023-05-10</PublicationDate>
  <ScanInformation>Scan Group</ScanInformation>
</ComicInfo>"#;
        let meta = parse_comic_info_xml(xml).expect("comicinfo parsed");
        assert_eq!(meta.title.as_deref(), Some("Chapter 5"));
        assert_eq!(meta.manga_title.as_deref(), Some("My Series"));
        assert_eq!(meta.number, Some(5.0));
        assert_eq!(meta.author.as_deref(), Some("W Author"));
        assert_eq!(meta.artist.as_deref(), Some("P Artist"));
        assert_eq!(meta.page_count, Some(20));
        assert_eq!(meta.scanlator.as_deref(), Some("Scan Group"));
        assert!(meta.genre.as_deref().unwrap().contains("Action"));
        // PublicationDate -> epoch seconds
        assert_eq!(meta.upload_date, Some(1683676800)); // 2023-05-10T00:00:00Z
    }
}
