//! Server 入口（对应 Main.kt + JavalinSetup.kt）：配置 → 数据库 → 迁移 →
//! HTTP（axum：REST /api/v1、GraphQL /api、OPDS、静态 WebUI）。

// release 无控制台窗口（隐藏启动在真实系统上可能被安全软件拦截）；日志由父进程重定向
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use suwayomi_core::config::ServerConfig;
use suwayomi_core::db::Db;
use suwayomi_domain::source::{SourceFetcher, StubFetcher};
use suwayomi_rest::AppState;

/// Build version name — `r{versionCode}`（由 suwayomi-core/build.rs 统一注入）。
pub const VERSION: &str = suwayomi_core::version::VERSION;
/// Internal version code — commit count + 3000 (see suwayomi-core/build.rs).
pub const VERSION_CODE: &str = suwayomi_core::version::VERSION_CODE;
/// Commit count baked in at build time (see suwayomi-core/build.rs).
pub const VERSION_COUNT: &str = suwayomi_core::version::VERSION_COUNT;

fn config_from_env() -> ServerConfig {
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
    cfg
}

/// 解析捆绑 WebUI 目录：`SUWAYOMI_WEBUI_DIR` → exe 同级 webui/（都不含 index.html 返回空）
fn resolve_webui_dir() -> std::path::PathBuf {
    let from_env = std::env::var("SUWAYOMI_WEBUI_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("webui"));
    if from_env.join("index.html").is_file() {
        return from_env;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand = dir.join("webui");
            if cand.join("index.html").is_file() {
                return cand;
            }
        }
    }
    std::path::PathBuf::new()
}

/// 用户数据根目录（backups/downloads/local 之下）：env → exe 上级 data（bin/ 布局）→ cwd/data
fn resolve_data_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("SUWAYOMI_DATA_DIR") {
        if !dir.trim().is_empty() {
            return std::path::PathBuf::from(dir);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.file_name().map(|n| n == "bin").unwrap_or(false) {
                if let Some(base) = dir.parent() {
                    return base.join("data");
                }
            }
        }
    }
    std::path::PathBuf::from("data")
}

/// 扩展沙盒 jar：`SUWAYOMI_SANDBOX_JAR` → exe 同级/../bin 的 jvm-sandbox.jar（发布布局）
fn resolve_sandbox_jar() -> Option<std::path::PathBuf> {
    if let Ok(jar) = std::env::var("SUWAYOMI_SANDBOX_JAR") {
        if !jar.is_empty() {
            let p = std::path::PathBuf::from(jar);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for cand in [dir.join("jvm-sandbox.jar"), dir.join("bin").join("jvm-sandbox.jar")] {
                if cand.is_file() {
                    return Some(cand);
                }
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
        .nest("/api", suwayomi_graphql::schema::graphql_router(graphql_schema))
        .nest("/api/opds/v1.2", suwayomi_opds::router::opds_router())
        // 优雅关闭端点（托盘用）：触发 axum graceful shutdown → Db drop 停
        // postgres、杀 JVM 沙盒子进程。仅限 loopback。
        .route(
            "/api/v1/shutdown",
            post(
                move |ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>| async move {
                    if !addr.ip().is_loopback() {
                        return (StatusCode::FORBIDDEN, "shutdown only allowed from loopback");
                    }
                    let _ = shutdown_tx.send(true);
                    (StatusCode::OK, "shutdown requested")
                },
            ),
        )
        // 本地图源文件双路径（GraphQL 返回相对 local/，WebUI 前缀 api/v1/local/）
        .route("/local/{*path}", get(local_file))
        .route("/api/v1/local/{*path}", get(local_file));

    let auth = middleware::from_fn_with_state(state.clone(), suwayomi_rest::auth::require_auth);
    if state.webui_dir.join("index.html").is_file() {
        tracing::info!("webui static hosting from {}", state.webui_dir.display());
        Router::new()
            .merge(api)
            .fallback(webui_fallback)
            .layer(auth)
            .with_state(state)
    } else {
        Router::new()
            .route("/", get(index))
            .route("/api/v1", get(index))
            .merge(api)
            .layer(auth)
            .with_state(state)
    }
}

/// 服务本地图源文件（封面/页面/归档内图片），防路径穿越
async fn local_file(State(_state): State<AppState>, path: axum::extract::Path<String>) -> Response {
    let rel = path.replace('\\', "/");
    if rel.is_empty() || rel.split('/').any(|seg| seg == "..") || rel.contains("://") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let root = suwayomi_domain::source::local::local_source_root();
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
async fn webui_fallback(State(state): State<AppState>, uri: axum::http::Uri) -> Response {
    let dir = &state.webui_dir;
    let rel = uri.path().trim_start_matches('/');
    let candidate = if rel.is_empty() {
        dir.join("index.html")
    } else {
        dir.join(rel)
    };
    let file = if candidate.is_file() {
        candidate
    } else {
        dir.join("index.html")
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

/// Phase 7: 定位 Kotlin H2 库，用 tools/h2-dump 导出并导入到当前后端
async fn import_h2_data(db: &Db, data_dir: &std::path::Path) -> anyhow::Result<()> {
    use sqlx::Executor;

    // 1) 定位 H2 文件
    let h2_file = if data_dir.join("tachidesk.mv.db").exists() {
        data_dir.join("tachidesk.mv.db")
    } else {
        let mut found = None;
        for entry in std::fs::read_dir(data_dir)? {
            let e = entry?;
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".mv.db") {
                found = Some(e.path());
                break;
            }
        }
        found.ok_or_else(|| anyhow::anyhow!("no *.mv.db found in {}", data_dir.display()))?
    };
    let h2_base = h2_file.to_string_lossy().trim_end_matches(".mv.db").to_string();
    tracing::info!("h2 database found: {}", h2_file.display());

    // 2) 定位 h2-dump jar
    let jar = std::env::var("SUWAYOMI_H2_DUMP_JAR").ok().map(std::path::PathBuf::from).unwrap_or_else(|| {
        std::path::PathBuf::from("tools/h2-dump/build/libs/h2-dump.jar")
    });
    if !jar.exists() {
        anyhow::bail!(
            "h2-dump jar not found at {} — build it with `gradle -p tools/h2-dump build` or set SUWAYOMI_H2_DUMP_JAR",
            jar.display()
        );
    }

    // 3) dump H2 -> SQL
    let out_sql = std::env::temp_dir().join(format!("suwayomi-h2dump-{}.sql", std::process::id()));
    let status = std::process::Command::new("java")
        .arg("-jar")
        .arg(&jar)
        .arg(&h2_base)
        .arg(&out_sql)
        .status()?;
    if !status.success() {
        anyhow::bail!("h2-dump exited with {status}");
    }
    let sql = std::fs::read_to_string(&out_sql)?;
    let _ = std::fs::remove_file(&out_sql);

    // 4) 逐条执行（h2-dump 已按 FK 安全序导出）
    let mut applied = 0usize;
    for stmt in sql.split(';').map(str::trim).filter(|s| !s.is_empty() && !s.starts_with("--")) {
        if stmt.is_empty() {
            continue;
        }
        db.pool().execute(stmt).await?;
        applied += 1;
    }
    tracing::info!("h2-dump import: {applied} statements applied");
    Ok(())
}

/// 把持久化的 localSourcePath（setSettings 存的 global_meta）还原到进程内
/// 本地图源根目录 override，自定义目录重启后仍生效
async fn load_local_source_path(db: &Db) {
    let Ok(Some((value,))) = sqlx::query_as::<_, (String,)>(
        "SELECT value FROM global_meta WHERE meta_key = 'settings'",
    )
    .fetch_optional(db.pool())
    .await
    else {
        return;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&value) else {
        return;
    };
    let Some(p) = json
        .get("localSourcePath")
        .and_then(|v| v.as_str())
        .filter(|p| !p.is_empty())
    else {
        return;
    };
    suwayomi_domain::source::local::set_local_source_root(Some(std::path::PathBuf::from(p)));
    tracing::info!("local source path from settings: {p}");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // `-v` / `--version`：打印版本信息后退出（替代打包产物里的 VERSION.txt）
    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    if cli_args.iter().any(|a| a == "-v" || a == "--version") {
        println!("Suwayomi {VERSION}");
        println!("{}", env!("CARGO_PKG_REPOSITORY"));
        return Ok(());
    }

    // 单实例：命名互斥体，已运行则退出（避免多开）
    #[cfg(windows)]
    let _instance_guard = {
        use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let name: Vec<u16> = "SuwayomiServerSingleInstance"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            let already = h.is_null() || GetLastError() == ERROR_ALREADY_EXISTS;
            if already {
                if !h.is_null() {
                    CloseHandle(h);
                }
                eprintln!("suwayomi-server 已在运行（单实例），本实例退出");
                return Ok(());
            }
            h
        }
    };
    #[cfg(not(windows))]
    let _instance_guard = ();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = config_from_env();
    tracing::info!(name = "Suwayomi (next)", version = VERSION, "starting");

    // `--migrate <kotlin-data-dir> [--h2-dump-jar <path>]`：H2 数据导入后退出（不起 HTTP）
    let migrate_dir: Option<std::path::PathBuf> = {
        let mut args = std::env::args().skip(1);
        let mut dir = None;
        while let Some(a) = args.next() {
            match a.as_str() {
                "--migrate" => dir = args.next().map(std::path::PathBuf::from),
                "--h2-dump-jar" => {
                    if let Some(p) = args.next() {
                        // SAFETY: single-threaded arg parsing before any
                        // other env access; this only sets one flag var.
                        unsafe { std::env::set_var("SUWAYOMI_H2_DUMP_JAR", p) };
                    }
                }
                _ => {}
            }
        }
        dir
    };

    // 后端：默认嵌入式 Oliphaunt（原生 PostgreSQL）；显式 SUWAYOMI_DATABASE_URL → 外部 PG。
    // SUWAYOMI_PGLITE_DATA_DIR 指定嵌入式数据目录（默认 ./pglite-data；"" = 临时库）
    let db = if config.database_url.is_empty() {
        let data_dir = match std::env::var("SUWAYOMI_PGLITE_DATA_DIR") {
            Ok(v) if !v.is_empty() => Some(std::path::PathBuf::from(v)),
            Ok(_) => None, // 显式空 → 临时库（测试/开发）
            Err(_) => Some(std::path::PathBuf::from("pglite-data")),
        };
        tracing::info!("database backend: embedded Oliphaunt PostgreSQL (set SUWAYOMI_DATABASE_URL to use external PostgreSQL)");
        // 预建目录让 oliphaunt 的 stable_root_lock 落在 pglite-data 内而非发布根
        if let Some(dir) = &data_dir {
            std::fs::create_dir_all(dir).map_err(anyhow::Error::from)?;
        }
        Db::connect_embedded(data_dir.as_deref()).await?
    } else {
        tracing::info!("database backend: external PostgreSQL at {}", config.database_url);
        Db::connect(&config.database_url).await?
    };
    db.migrate().await?;
    tracing::info!(mode = ?db.mode(), "database ready (migrations applied)");

    // 确保默认分类 (id=0) 存在——书架页首个 tab 依赖；ON CONFLICT 幂等（覆盖
    // 备份恢复后 category 表为空的情况）
    sqlx::query(
        "INSERT INTO category (id, name, sort_order, is_default, include_in_update, include_in_download) \
         VALUES (0, '默认', 0, TRUE, -1, -1) ON CONFLICT (id) DO NOTHING",
    )
    .execute(db.pool())
    .await
    .map_err(anyhow::Error::from)?;

    // 还原持久化的 localSourcePath，重启后自定义本地图源目录仍生效
    load_local_source_path(&db).await;

    // Phase 7: import the Kotlin H2 data and stop.
    if let Some(dir) = migrate_dir {
        import_h2_data(&db, &dir).await?;
        tracing::info!("--migrate finished; run `suwayomi` normally to serve");
        return Ok(());
    }

    // 启动 JVM 扩展沙盒（见 domain/source/sandbox.rs）。env：
    // SUWAYOMI_SANDBOX_JAR（缺省 exe 旁 jvm-sandbox.jar）、SUWAYOMI_SANDBOX_PORT
    // （默认 8091；避开 Windows Hyper-V 动态保留区 4501-4900）、SUWAYOMI_EXTENSIONS_DIR
    let mut sandbox_guard: Option<suwayomi_domain::source::sandbox::SandboxProcess> = None;
    if let Some(jar) = resolve_sandbox_jar() {
        let port = std::env::var("SUWAYOMI_SANDBOX_PORT").unwrap_or_else(|_| "8091".into());
        let jar_str = jar.to_string_lossy().into_owned();
        let proc = suwayomi_domain::source::sandbox::SandboxProcess::start(&jar_str, &port).await;
        match proc {
            Ok(p) => {
                tracing::info!("jvm sandbox connected at 127.0.0.1:{port} (jar: {jar_str})");
                sandbox_guard = Some(p);
            }
            Err(e) => tracing::warn!("jvm sandbox failed to start: {e}; falling back to StubFetcher"),
        }
    }

    let sandbox_base = sandbox_guard.as_ref().map(|g| g.fetcher().base_url().to_string());
    let fetcher: Arc<dyn SourceFetcher> =
        if let Some(guard) = &sandbox_guard { Arc::new(guard.fetcher()) } else { Arc::new(StubFetcher) };
    // `let _sandbox = sandbox_guard`（非 `let _ =`）：变量名形式保活整个 server
    // 生命周期，`let _ =` 会立即 drop 杀掉 JVM
    let _sandbox = sandbox_guard;

    // 用磁盘 downloads/** 对账数据库（历史下载显示"已下载"角标）；失败不阻塞启动
    let data_dir_path = resolve_data_dir();
    if let Err(e) = suwayomi_domain::download::reconcile_downloads(&db, &data_dir_path).await {
        tracing::warn!("downloads reconcile failed: {e}");
    }

    let graphql_state = suwayomi_graphql::GraphQLState::new(db.clone(), config.clone(), fetcher.clone(), sandbox_base.clone(), resolve_webui_dir(), data_dir_path.clone());
    // Scheduled auto-backup loop (`autoBackupFrequency`/`backupPath` settings).
    suwayomi_graphql::autobackup::spawn(graphql_state.clone());
    let schema = suwayomi_graphql::schema::build_schema(graphql_state);
    tracing::info!("graphql schema ready ({} type definitions)", suwayomi_graphql::schema::schema_type_count());
    let state = AppState::new(db, config.clone(), fetcher, sandbox_base, resolve_webui_dir(), data_dir_path.clone());
    // shutdown 通知通道：POST /api/v1/shutdown（或 Ctrl+C）触发优雅关闭，
    // 干净停掉嵌入式 postgres 与沙盒子进程而非遗留孤儿
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
    .with_graceful_shutdown(shutdown_signal(shutdown_rx))
    .await?;
    tracing::info!("server stopped; shutting down embedded database");
    // 释放 router 持有的 Db/AppState/GraphQLState 引用，让 Oliphaunt 会话 Drop
    // 执行 pg_ctl stop；再清掉 oliphaunt 遗留的 root lock 文件
    drop(app);
    cleanup_oliphaunt_lock_files();
    Ok(())
}

/// 等待关闭信号（Ctrl+C / shutdown 端点 watch 通道）。优雅关闭让 Db Drop 停
/// postgres（否则残留 postmaster 锁阻塞下次启动）、沙盒 Drop 杀 JVM。
async fn shutdown_signal(mut rx: tokio::sync::watch::Receiver<bool>) {
    tokio::select! {
        _ = tokio::signal::ctrl_c() => tracing::info!("ctrl-c received; graceful shutdown"),
        _ = rx.changed() => tracing::info!("shutdown requested via /api/v1/shutdown; graceful shutdown"),
    }
}

/// 清理优雅停机后 oliphaunt 遗留的 root lock：NativeRootLock Drop 只解锁不删
/// 文件；数据目录内/父级（含旧版位置的兼容清扫）的 `.oliphaunt-root-*.lock`
/// 与 `.oliphaunt.lock` 下次 open 会重建
fn cleanup_oliphaunt_lock_files() {
    let data_dir = std::env::var("SUWAYOMI_PGLITE_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("pglite-data"));
    for dir in [data_dir.parent().map(std::path::PathBuf::from), Some(data_dir.clone())]
        .into_iter()
        .flatten()
    {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(".oliphaunt-root-") && name.ends_with(".lock") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    let _ = std::fs::remove_file(data_dir.join(".oliphaunt.lock"));
}
