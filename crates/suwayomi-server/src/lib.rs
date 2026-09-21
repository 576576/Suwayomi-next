//! 服务端启动逻辑（对应 Main.kt + JavalinSetup.kt）：配置 → 数据库 → 迁移 →
//! HTTP（axum：REST /api/v1、GraphQL /api、OPDS、静态 WebUI）。
//!
//! 参数一律由调用方经 [`ServerOptions`] 注入，[`run`] 自己不读环境变量（数据库设置
//! 例外：未显式给出时按 `SUWAYOMI_*` 解析）。桌面 CLI 在 `main.rs` 里组装参数；
//! Android 宿主 App 通过 JNI 组装同一份结构（见 docs/migration/ANDROID_IMPL.md）。

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Extension, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use suwayomi_core::auth::Principal;
use suwayomi_core::config::ServerConfig;
use suwayomi_core::db::{Db, DbSettings};
use suwayomi_domain::source::{SourceFetcher, StubFetcher};
use suwayomi_rest::AppState;

/// 认证参数的启动期解析（env → 设置 → 默认值）。
pub mod auth_setup;

/// Build version name — `r{versionCode}`（由 suwayomi-core/build.rs 统一注入）。
pub const VERSION: &str = suwayomi_core::version::VERSION;
/// Internal version code — commit count + 3000 (see suwayomi-core/build.rs).
pub const VERSION_CODE: &str = suwayomi_core::version::VERSION_CODE;
/// Commit count baked in at build time (see suwayomi-core/build.rs).
pub const VERSION_COUNT: &str = suwayomi_core::version::VERSION_COUNT;

pub fn config_from_env() -> ServerConfig {
    // Rust 后端只支持 PostgreSQL
    let mut cfg =
        ServerConfig { database_type: suwayomi_core::config::DatabaseType::Postgresql, ..ServerConfig::default() };
    if let Ok(v) = std::env::var("SUWAYOMI_PORT") {
        cfg.port = v.parse().unwrap_or(cfg.port);
    }
    if let Ok(v) = std::env::var("SUWAYOMI_IP") {
        cfg.ip = v;
    }
    if let Ok(v) = std::env::var("SUWAYOMI_DATABASE_URL") {
        cfg.database_url = v;
    }
    if let Ok(v) = std::env::var("SUWAYOMI_AUTH_MODE") {
        cfg.auth_mode = v;
    }
    if let Ok(v) = std::env::var("SUWAYOMI_AUTH_USERNAME") {
        cfg.auth_username = v;
    }
    if let Ok(v) = std::env::var("SUWAYOMI_AUTH_PASSWORD") {
        cfg.auth_password = v;
    }
    if let Ok(v) = std::env::var("SUWAYOMI_JWT_AUDIENCE") {
        cfg.jwt_audience = v;
    }
    if let Ok(v) = std::env::var("SUWAYOMI_JWT_TOKEN_EXPIRY") {
        cfg.jwt_token_expiry = v;
    }
    if let Ok(v) = std::env::var("SUWAYOMI_JWT_REFRESH_EXPIRY") {
        cfg.jwt_refresh_expiry = v;
    }
    cfg
}

/// 解析捆绑 WebUI 目录：`SUWAYOMI_WEBUI_DIR` → exe 同级 webui/（都不含 index.html 返回空）
pub fn resolve_webui_dir() -> std::path::PathBuf {
    let from_env = std::env::var("SUWAYOMI_WEBUI_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("webui"));
    if from_env.join("index.html").is_file() {
        return from_env;
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let cand = dir.join("webui");
        if cand.join("index.html").is_file() {
            return cand;
        }
    }
    std::path::PathBuf::new()
}

/// 用户数据根目录（backups/downloads/local 之下）：env → exe 上级 data（bin/ 布局）→ cwd/data
pub fn resolve_data_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("SUWAYOMI_DATA_DIR")
        && !dir.trim().is_empty()
    {
        return std::path::PathBuf::from(dir);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && dir.file_name().map(|n| n == "bin").unwrap_or(false)
        && let Some(base) = dir.parent()
    {
        return base.join("data");
    }
    std::path::PathBuf::from("data")
}

/// 追踪器 OAuth 应用凭据文件：`SUWAYOMI_TRACKERS_CONFIG` → `<settings 目录>/trackers.json`。
///
/// settings 目录优先取 `SUWAYOMI_SETTINGS_DIR`（沙盒写源偏好用的是同一个变量、同一个
/// 目录），其次按扩展目录的上一级推（与沙盒的默认规则一致），最后落在数据根的上一级
/// —— Android 没有环境变量可用，只有 `data_dir`，靠最后一条。
pub fn resolve_trackers_config_file(data_dir: &std::path::Path) -> std::path::PathBuf {
    if let Ok(file) = std::env::var("SUWAYOMI_TRACKERS_CONFIG")
        && !file.trim().is_empty()
    {
        return std::path::PathBuf::from(file);
    }
    resolve_settings_dir(data_dir).join("trackers.json")
}

fn resolve_settings_dir(data_dir: &std::path::Path) -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("SUWAYOMI_SETTINGS_DIR")
        && !dir.trim().is_empty()
    {
        return std::path::PathBuf::from(dir);
    }
    if let Ok(ext) = std::env::var("SUWAYOMI_EXTENSIONS_DIR")
        && let Some(parent) = std::path::Path::new(&ext).parent()
        && !parent.as_os_str().is_empty()
    {
        return parent.join("settings");
    }
    data_dir.parent().map(|base| base.join("settings")).unwrap_or_else(|| std::path::PathBuf::from("settings"))
}

/// 扩展沙盒 jar：`SUWAYOMI_SANDBOX_JAR` → exe 同级/../bin 的 jvm-sandbox.jar（发布布局）
pub fn resolve_sandbox_jar() -> Option<std::path::PathBuf> {
    if let Ok(jar) = std::env::var("SUWAYOMI_SANDBOX_JAR")
        && !jar.is_empty()
    {
        let p = std::path::PathBuf::from(jar);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        for cand in [dir.join("jvm-sandbox.jar"), dir.join("bin").join("jvm-sandbox.jar")] {
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

fn build_router(
    state: AppState,
    graphql_schema: suwayomi_graphql::schema::GraphQLSchema,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
) -> Router {
    let api = Router::new()
        .nest("/api/v1", suwayomi_rest::routes::api_v1_router())
        .nest("/api", suwayomi_graphql::schema::graphql_router(graphql_schema, state.auth.clone()))
        .nest("/api/opds/v1.2", suwayomi_opds::router::opds_router())
        // 优雅关闭端点（托盘用）：触发 axum graceful shutdown → Db drop 停
        // postgres、杀 JVM 沙盒子进程。
        //
        // 要凭据，但**放行不带 `Origin` 的本机调用**：桌面托盘是裸 HTTP 客户端，
        // 拿不到凭据；而浏览器发的 POST 一定带 `Origin`（恶意页面从本机页面
        // 触发它才是真正的风险）。两条判据叠加后，跨站页面仍打不进来。
        .route(
            "/api/v1/shutdown",
            post(
                |State(state): State<AppState>,
                 ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
                 Extension(principal): Extension<Principal>,
                 headers: axum::http::HeaderMap| async move {
                    if !addr.ip().is_loopback() {
                        return (StatusCode::FORBIDDEN, "shutdown only allowed from loopback");
                    }
                    let from_browser = headers.contains_key(axum::http::header::ORIGIN)
                        || headers.contains_key("sec-fetch-site");
                    if !state.auth.is_disabled() && !principal.is_authenticated() && from_browser {
                        return (StatusCode::UNAUTHORIZED, "shutdown requires authentication");
                    }
                    let _ = shutdown_tx.send(true);
                    (StatusCode::OK, "shutdown requested")
                },
            ),
        )
        // 本地图源文件双路径（GraphQL 返回相对 local/，WebUI 前缀 api/v1/local/）
        .route("/local/{*path}", get(local_file))
        .route("/api/v1/local/{*path}", get(local_file));

    // 登录流程：服务端自渲染的最小页面，不依赖 /assets/*（issue #5 的白屏就是
    // 重定向到一个并不存在的登录页，落到 SPA fallback 后又被门禁掐死 assets）。
    let login = Router::new()
        .route(
            "/login.html",
            get(suwayomi_rest::auth::login_page).post(suwayomi_rest::auth::login_submit),
        )
        .route("/logout", get(suwayomi_rest::auth::logout));

    let auth = middleware::from_fn_with_state(state.clone(), suwayomi_rest::auth::require_auth);
    if state.webui_dir.join("index.html").is_file() {
        tracing::info!("webui static hosting from {}", state.webui_dir.display());
        Router::new()
            .merge(api)
            .merge(login)
            .fallback(webui_fallback)
            .layer(auth)
            .with_state(state)
    } else {
        Router::new()
            .route("/", get(index))
            .route("/api/v1", get(index))
            .merge(api)
            .merge(login)
            .layer(auth)
            .with_state(state)
    }
}

/// 服务本地图源文件（封面/页面/归档内图片），防路径穿越
async fn local_file(State(_state): State<AppState>, path: axum::extract::Path<String>) -> Response {
    let rel = path.replace('\\', "/");
    if !suwayomi_rest::auth::is_safe_rel(&rel) || rel.contains("://") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let root = suwayomi_domain::source::local::local_source_root();
    // 拼接结果必须还在 root 之下：Windows 上带盘符的绝对路径会整体替换 base
    if !root.join(&rel).starts_with(&root) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let file = root.join(&rel);
    if file.is_file() {
        return read_file_response(&file).await;
    }
    // 归档成员路径：local/<manga>/<chapter>.zip/<page>
    let segments: Vec<&str> = rel.split('/').collect();
    for split in 0..segments.len() {
        let ext = segments[split].rsplit('.').next().unwrap_or("");
        if suwayomi_domain::source::local::ARCHIVE_EXTS.contains(&ext.to_lowercase().as_str()) {
            let archive_rel = segments[..=split].join("/");
            let member = segments[split + 1..].join("/");
            if member.is_empty() {
                continue;
            }
            if let Some(bytes) =
                suwayomi_domain::source::local::read_archive_image(&root.join(&archive_rel), &member)
            {
                return Response::builder()
                    .header(axum::http::header::CONTENT_TYPE, image_content_type(&member))
                    .header(axum::http::header::CACHE_CONTROL, "public, max-age=3600")
                    .header(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                    .body(axum::body::Body::from(bytes))
                    .expect("build response");
            }
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

async fn read_file_response(file: &std::path::Path) -> Response {
    match tokio::fs::read(file).await {
        Ok(bytes) => {
            let ct = webui_content_type(file);
            Response::builder()
                .header(axum::http::header::CONTENT_TYPE, ct)
                .header(axum::http::header::CACHE_CONTROL, "public, max-age=3600")
                .header(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(axum::body::Body::from(bytes))
                .expect("build response")
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

fn image_content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_lowercase().as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "heic" => "image/heic",
        _ => "image/jpeg",
    }
}

/// WebUI 静态托管 fallback：存在则返回文件，否则回退 index.html（SPA 路由）
///
/// 路径一律经 [`suwayomi_rest::auth::safe_join`] 解析——`dir.join(rel)` 允许
/// `..` 与 Windows 盘符绝对路径逃出目录，等于把整个磁盘暴露出去。
async fn webui_fallback(State(state): State<AppState>, uri: axum::http::Uri) -> Response {
    let dir = &state.webui_dir;
    let rel = uri.path().trim_start_matches('/');
    let resolved = if rel.is_empty() {
        Some(dir.join("index.html"))
    } else {
        // 先解码再判越界（`%2f` 写法必须被识破），且**不要求文件存在**：
        // 深链接要回退到 index.html，用要求文件存在的 safe_join 会让刷新变 404。
        suwayomi_rest::auth::safe_public_path(dir, rel)
    };
    let file = match resolved {
        Some(path) if path.is_file() => path,
        // 目录内不存在的普通路径按 SPA 深链接处理
        Some(_) => dir.join("index.html"),
        // 越界路径（`..`、盘符、NTFS 数据流）直接 404，不回退 index.html
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    match tokio::fs::read(&file).await {
        Ok(bytes) => {
            let ct = webui_content_type(&file);
            Response::builder()
                .header(axum::http::header::CONTENT_TYPE, ct)
                .body(axum::body::Body::from(bytes))
                .expect("build response")
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

fn webui_content_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("webp") => "image/webp",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("wasm") => "application/wasm",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

async fn index(State(_s): State<AppState>) -> Result<String, StatusCode> {
    Ok(format!("Suwayomi (next) v{VERSION} — GraphQL at /api/graphql, REST at /api/v1, OPDS at /api/opds/v1.2"))
}

/// 读 `global_meta['settings']` 里那个 JSON blob（WebUI 的 setSettings 写的）。
/// 不存在或不是对象 → `None`。
async fn load_settings_blob(db: &Db) -> Option<serde_json::Value> {
    let Ok(Some((value,))) =
        suwayomi_db::query_as::<(String,)>("SELECT value FROM global_meta WHERE meta_key = 'settings'")
            .fetch_optional(db.pool())
            .await
    else {
        return None;
    };
    serde_json::from_str::<serde_json::Value>(&value).ok()
}

/// blob 里的非空字符串设置项。
fn blob_str(json: &serde_json::Value, key: &str) -> Option<String> {
    json.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned)
}

/// 把持久化的 localSourcePath（setSettings 存的 global_meta）还原到进程内
/// 本地图源根目录 override，自定义目录重启后仍生效
fn load_local_source_path(blob: Option<&serde_json::Value>) {
    let Some(p) = blob.and_then(|json| blob_str(json, "localSourcePath")) else {
        return;
    };
    suwayomi_domain::source::local::set_local_source_root(Some(std::path::PathBuf::from(&p)));
    tracing::info!("local source path from settings: {p}");
}

/// 持久化的数据目录（WebUI「数据与存储」页的「存储位置」）。
///
/// 设置里显式填了就**以它为准**（`SUWAYOMI_DATA_DIR` / `ServerOptions::data_dir`
/// 退居默认值）——否则 WebUI 里改完重启就白改了：桌面壳总会塞一个
/// `SUWAYOMI_DATA_DIR` 进来，env 优先的话这个设置项就永远是死的。
///
/// 这一项之所以能存进库里，是因为**数据库文件不在数据目录下**
/// （见 `suwayomi_db::config::default_db_dir`）。
fn load_data_dir_setting(blob: Option<&serde_json::Value>, fallback: std::path::PathBuf) -> std::path::PathBuf {
    let Some(dir) = blob.and_then(|json| blob_str(json, "dataDir")) else {
        return fallback;
    };
    let dir = std::path::PathBuf::from(dir);
    if dir != fallback {
        tracing::info!(
            "data dir from settings: {} (overrides {})",
            dir.display(),
            fallback.display()
        );
    }
    dir
}

/// 扩展（源）来源。
///
/// 桌面走 [`SandboxMode::Spawn`]（JVM 子进程）；Android 上 Android 10+ 禁止 App 从
/// 私有目录 exec，所以宿主 App 把扩展宿主跑在自己的进程里，server 走
/// [`SandboxMode::External`] 连过去。
pub enum SandboxMode {
    /// 拉起独立 JVM 沙盒子进程（见 `suwayomi_domain::source::sandbox`）。
    Spawn { jar: std::path::PathBuf, port: String },
    /// 连接一个已经在运行的扩展宿主。
    External { base_url: String },
    /// 不接扩展：source 调用返回「不可用」，服务照常启动。
    Disabled,
}

/// 启动参数（桌面 CLI 与 Android JNI 共用同一份）。
pub struct ServerOptions {
    pub config: ServerConfig,
    /// 用户数据根目录（backups/downloads/local 之下）。
    pub data_dir: std::path::PathBuf,
    /// 静态 WebUI 目录（无 index.html 时回退内置占位页）。
    pub webui_dir: std::path::PathBuf,
    /// 显式数据库设置；`None` → 按 `SUWAYOMI_*` 环境变量解析（桌面路径）。
    pub db: Option<DbSettings>,
    pub sandbox: SandboxMode,
    /// 外部关闭信号（Android 宿主 `stop()` 用）。`None` → 只认 Ctrl+C 与
    /// `POST /api/v1/shutdown`。
    pub shutdown: Option<tokio::sync::watch::Receiver<bool>>,
}

/// 初始化日志订阅者：桌面写 stdout/stderr，Android 写 logcat（tag `Suwayomi`）。
///
/// 幂等 —— 已经初始化过就保留原订阅者（Android 宿主会 start/stop 反复调用）。
pub fn init_logging(default_filter: &str) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_filter));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    #[cfg(target_os = "android")]
    let builder = builder.with_writer(android_log::LogcatWriter);
    let _ = builder.try_init();
}

/// 启动服务直到收到关闭信号（Ctrl+C、`POST /api/v1/shutdown`，或 Android 宿主的停止调用）。
pub async fn run(opts: ServerOptions) -> anyhow::Result<()> {
    let ServerOptions { config, data_dir, webui_dir, db: db_settings, sandbox, shutdown } = opts;
    tracing::info!(name = "Suwayomi (next)", version = VERSION, "starting");
    let settings = db_settings.unwrap_or_else(DbSettings::from_env);
    tracing::info!("database backend: {}", settings.describe());
    let db = Db::connect(&settings).await?;
    db.migrate().await?;
    tracing::info!(backend = db.kind().as_str(), "database ready (migrations applied)");

    // 确保默认分类 (id=0) 存在——书架页首个 tab 依赖；ON CONFLICT 幂等（覆盖
    // 备份恢复后 category 表为空的情况）。
    // 名字必须与上游 M0027_AddDefaultCategory 一致，固定英文 'Default'：
    // 上游 WebUI 的「编辑分类」页正是按 `nodes[0].name === 'Default'` 这个字面量
    // 把默认分类从列表里剔除的（CategorySettings.tsx）。若写成中文「默认」，该判据
    // 失效，默认分类会混进可排序列表。DO UPDATE 用于纠正历史库里已有的中文名。
    suwayomi_db::query(
        "INSERT INTO category (id, name, sort_order, is_default, include_in_update, include_in_download) \
         VALUES (0, 'Default', 0, TRUE, -1, -1) \
         ON CONFLICT (id) DO UPDATE SET name = 'Default' WHERE category.name <> 'Default'",
    )
    .execute(db.pool())
    .await
    .map_err(anyhow::Error::from)?;

    // WebUI 写的那份设置（`global_meta['settings']`）：下面几项启动期设置都从
    // 它兜底，只查一次
    let settings_blob = load_settings_blob(&db).await;

    // 还原持久化的 localSourcePath，重启后自定义本地图源目录仍生效
    load_local_source_path(settings_blob.as_ref());

    // 存储位置（dataDir）同样从设置里读 —— 数据库文件不在这个目录下，所以它
    // 可以被随便改而不影响设置本身（见 suwayomi_db::config::default_db_dir）
    let data_dir = load_data_dir_setting(settings_blob.as_ref(), data_dir);
    tracing::info!("data dir: {}", data_dir.display());

    // 认证：模式解析失败直接不启动。静默退化成「无认证」比启动失败危险得多。
    let mut config = config;
    let (auth, secret_source) =
        auth_setup::resolve(&config, settings_blob.as_ref(), &suwayomi_db::config::default_db_dir())?;
    config.auth_mode = auth.mode.as_str().to_string();
    config.auth_username = auth.username.clone();
    config.auth_password = auth.password.clone();
    config.jwt_audience = auth.jwt_audience.clone();
    let auth = Arc::new(auth);
    tracing::info!("auth mode: {} (session secret: {})", auth.mode.as_str(), secret_source.describe());
    if auth.is_disabled() {
        tracing::warn!("authentication is disabled; the library is reachable by anyone who can reach this port");
    }

    // 扩展来源（见 docs/migration/ANDROID_IMPL.md）：
    // * Spawn    —— 桌面默认：拉起 JVM 沙盒子进程（jar 由调用方解析好）
    // * External —— 已运行的扩展宿主；Android 上是宿主 App 起的宿主，桌面也可用它
    //               接管一个自己拉起的沙盒
    // * Disabled —— 不接扩展（StubFetcher，服务照常启动）
    let mut sandbox_guard: Option<suwayomi_domain::source::sandbox::SandboxProcess> = None;
    let mut external_host: Option<suwayomi_domain::source::sandbox::HttpSandboxFetcher> = None;
    match sandbox {
        SandboxMode::Spawn { jar, port } => {
            let jar_str = jar.to_string_lossy().into_owned();
            match suwayomi_domain::source::sandbox::SandboxProcess::start(&jar_str, &port).await {
                Ok(p) => {
                    tracing::info!("jvm sandbox connected at 127.0.0.1:{port} (jar: {jar_str})");
                    sandbox_guard = Some(p);
                }
                Err(e) => tracing::warn!("jvm sandbox failed to start: {e}; falling back to StubFetcher"),
            }
        }
        SandboxMode::External { base_url } => {
            let host = suwayomi_domain::source::sandbox::HttpSandboxFetcher::new(base_url.clone());
            // 宿主可能比 server 晚就绪（Android 宿主先起监听再拉 server，也可能反过来）；
            // 探活失败不致命，source 调用自己会报错
            if host.health().await {
                tracing::info!("external extension host connected at {base_url}");
            } else {
                tracing::warn!("external extension host at {base_url} does not answer /health yet");
            }
            external_host = Some(host);
        }
        SandboxMode::Disabled => tracing::info!("extension sandbox disabled (no source host)"),
    }

    let sandbox_base = sandbox_guard
        .as_ref()
        .map(|g| g.fetcher().base_url().to_string())
        .or_else(|| external_host.as_ref().map(|f| f.base_url().to_string()));
    let fetcher: Arc<dyn SourceFetcher> = if let Some(guard) = &sandbox_guard {
        Arc::new(guard.fetcher())
    } else if let Some(host) = &external_host {
        Arc::new(host.clone())
    } else {
        Arc::new(StubFetcher)
    };
    // `let _sandbox = sandbox_guard`（非 `let _ =`）：变量名形式保活整个 server
    // 生命周期，`let _ =` 会立即 drop 杀掉 JVM
    let _sandbox = sandbox_guard;

    // 启动时把沙盒里**已经加载**的扩展同步进库：建 `extension` 行（此前只认
    // 仓库索引行，手工放进 extensions/ 的 APK 永远进不了库）+ 注册 source 行。
    //
    // Android 上这一步是**唯一**的入库路径 —— 扩展装在系统里（PackageManager），
    // 既没有 extensions/ 目录也没有仓库索引，不同步的话 WebUI 扩展页恒为空。
    // 失败不阻塞启动：扩展不可用不影响书架/阅读等主功能。
    if let Some(base) = &sandbox_base {
        let store = suwayomi_domain::extension_store::ExtensionStoreService::new(db.clone(), Some(base.clone()));
        match store.sync_sources().await {
            Ok(n) => tracing::info!("extension sync at startup: {n} source(s) registered"),
            Err(e) => tracing::warn!("extension sync at startup failed: {e}"),
        }
    }

    // 用磁盘 downloads/** 对账数据库（历史下载显示"已下载"角标）；失败不阻塞启动
    let data_dir_path = data_dir;
    if let Err(e) = suwayomi_domain::download::reconcile_downloads(&db, &data_dir_path).await {
        tracing::warn!("downloads reconcile failed: {e}");
    }

    let update = suwayomi_domain::updater::UpdateManager::new(db.clone(), fetcher.clone());
    // REST 与 GraphQL 共用同一个追踪器句柄：登录态是从数据库读的，两个入口看到
    // 的东西必须一致，克隆出两个实例会让「其中一个刚登录」的状态不同步。
    let oauth_config = resolve_trackers_config_file(&data_dir_path);
    let oauth_apps = suwayomi_domain::tracker::oauth::load_or_create(&oauth_config);
    let tracker = suwayomi_domain::tracker::TrackerManager::with_oauth(
        db.clone(),
        std::sync::Arc::new(std::sync::RwLock::new(oauth_apps)),
        oauth_config,
    );
    let graphql_state = suwayomi_graphql::GraphQLState::new(db.clone(), config.clone(), auth.clone(), fetcher.clone(), update.clone(), tracker.clone(), sandbox_base.clone(), webui_dir.clone(), data_dir_path.clone());
    // 持久化设置（`global_meta` 的 settings blob）盖到 env 基线上：KOReader 同步
    // 策略、SyncYomi 开关这类设置由服务在运行时读取，重启后必须生效。
    graphql_state.reload_runtime_config().await;
    // Scheduled auto-backup loop (`autoBackupFrequency`/`backupPath` settings).
    suwayomi_graphql::autobackup::spawn(graphql_state.clone());
    let schema = suwayomi_graphql::schema::build_schema(graphql_state);
    tracing::info!("graphql schema ready ({} type definitions)", suwayomi_graphql::schema::schema_type_count());
    let state = AppState::new(
        db.clone(),
        config.clone(),
        auth.clone(),
        fetcher,
        update,
        tracker,
        sandbox_base,
        webui_dir.clone(),
        data_dir_path.clone(),
    );
    // shutdown 通知通道：POST /api/v1/shutdown（或 Ctrl+C）触发优雅关闭，
    // 干净停掉数据库连接与沙盒子进程而非遗留孤儿
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let app = build_router(state, schema, shutdown_tx);

    // 端口自动回退：Windows 上 4501-4900 可能是 Hyper-V 动态保留段（10013）
    // 或端口被占（10048）——上探几个端口而不是崩溃
    let start = config.port;
    let mut port = start;
    let (listener, addr) = loop {
        let candidate: SocketAddr = format!("{}:{}", config.ip, port).parse()?;
        match tokio::net::TcpListener::bind(candidate).await {
            Ok(l) => break (l, candidate),
            Err(e) => {
                if port >= start + 50 {
                    return Err(e.into());
                }
                // Windows Hyper-V 动态保留区（4501-4900）整段不可用：10013 直接跳过
                let in_hyperv_range = cfg!(windows) && e.raw_os_error() == Some(10013) && port <= 4900;
                if in_hyperv_range {
                    tracing::warn!("port {port} 处于 Hyper-V 动态保留区（os error 10013）; 跳到 4901");
                    port = 4901;
                } else {
                    tracing::warn!(
                        "port {port} unavailable ({e}); trying {} — \
                         Windows 上 4501-4900 可能被 Hyper-V 动态保留，或被其他进程占用",
                        port + 1
                    );
                    port += 1;
                }
            }
        }
    };
    tracing::info!("server listening on http://{addr}");
    axum::serve(
        listener,
        app.clone().into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal(shutdown_rx, shutdown))
    .await?;
    tracing::info!("server stopped; shutting down database");
    // 先释放 router 持有的 Db/AppState/GraphQLState 引用，再关掉剩下的连接：
    // SQLite 停掉专属线程并释放文件锁，PostgreSQL 归还连接池。
    drop(app);
    if let Err(e) = db.close().await {
        tracing::warn!("closing database failed: {e}");
    }
    Ok(())
}

/// 等待关闭信号（Ctrl+C / shutdown 端点 watch 通道）。优雅关闭让 Db 释放连接
/// （否则残留连接会阻塞下次启动）、沙盒 Drop 杀 JVM。
async fn shutdown_signal(mut rx: tokio::sync::watch::Receiver<bool>, mut host: Option<tokio::sync::watch::Receiver<bool>>) {
    // 宿主（Android App）没有 Ctrl+C 可发，只能靠这条通道；没有宿主时该分支永不就绪
    let host_fired = async {
        match host.as_mut() {
            Some(rx) => {
                let _ = rx.changed().await;
            }
            None => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => tracing::info!("ctrl-c received; graceful shutdown"),
        _ = rx.changed() => tracing::info!("shutdown requested via /api/v1/shutdown; graceful shutdown"),
        _ = host_fired => tracing::info!("shutdown requested by the host; graceful shutdown"),
    }
}

/// Android 专用：tracing → logcat。
///
/// App 进程里 stdout/stderr 默认进不到任何地方，只有 logcat 看得到启动日志。
/// 不引 `android_logger` / `tracing-android`：需要的只是 liblog 里一个 C 函数。
#[cfg(target_os = "android")]
mod android_log {
    use std::io;

    pub struct LogcatWriter;

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogcatWriter {
        type Writer = LogcatStream;

        fn make_writer(&'a self) -> Self::Writer {
            LogcatStream
        }
    }

    pub struct LogcatStream;

    impl io::Write for LogcatStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            log_line(&String::from_utf8_lossy(buf));
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// logcat 优先级 INFO（`adb logcat -s Suwayomi`）
    const ANDROID_LOG_INFO: i32 = 4;

    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(prio: i32, tag: *const std::ffi::c_char, text: *const std::ffi::c_char) -> i32;
    }

    fn log_line(text: &str) {
        let text = text.trim_end_matches(|c| c == '\r' || c == '\n');
        if text.is_empty() {
            return;
        }
        // logcat 单行上限约 4KB，超出会被截断；启动日志远小于此
        if let Ok(line) = std::ffi::CString::new(text) {
            unsafe { __android_log_write(ANDROID_LOG_INFO, c"Suwayomi".as_ptr(), line.as_ptr()) };
        }
    }
}
