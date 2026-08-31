//! Suwayomi server entry point.
//!
//! Mirrors Suwayomi `Main.kt` + `JavalinSetup.kt`:
//! - application setup: config → database → migrations
//! - HTTP server (axum) with `/api/v1/**` (Phase 3), GraphQL (Phase 4),
//!   OPDS (Phase 6) and static WebUI hosting.

// release 版不创建控制台窗口（替代托盘/bat 的隐藏 CLI 启动——隐藏启动在
// 真实系统上可能被安全软件拦截）。日志仍可被父进程重定向到文件。
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

/// Build version name — `r{versionCode}` (see build.rs), e.g. r3050.
pub const VERSION: &str = env!("SUWAYOMI_VERSION_NAME");
/// Internal version code — commit count + 3000 (see build.rs).
pub const VERSION_CODE: &str = env!("SUWAYOMI_VERSION_CODE");
/// Commit count baked in at build time (see build.rs).
pub const VERSION_COUNT: &str = env!("SUWAYOMI_VERSION_COUNT");

fn config_from_env() -> ServerConfig {
    // Pure-PostgreSQL decision (2026-08-30): Rust backend only supports PG.
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

/// Resolves the bundled WebUI directory: `SUWAYOMI_WEBUI_DIR` env, else the
/// `webui/` folder next to the executable (发布布局：exe 同级 webui/）。
/// Returns empty path when neither exists.
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

/// Resolves the JVM extension sandbox jar: `SUWAYOMI_SANDBOX_JAR` env, else the
/// `jvm-sandbox.jar` bundled next to this executable, else the `bin/` subfolder
/// (发布布局：suwayomi-server.exe 与 jvm-sandbox.jar 同在 bin/）。
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
        // Graceful shutdown endpoint used by the tray app (and scripts):
        // `POST /api/v1/shutdown` triggers axum's graceful shutdown, letting
        // the runtime unwind normally (Db drop → oliphaunt `pg_ctl stop`, JVM
        // sandbox child kill). Only callable from loopback.
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
        // Local-source files (covers, chapter pages) — exposed under both
        // `/local/...` (relative `local/...` URLs returned by GraphQL) and
        // `/api/v1/local/...` (which `getValidImgUrlFor` in the WebUI
        // prefixes with the API version).
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

/// Serves local-source files from the local source root (default
/// `data/local/<path>`, or the configured `localSourcePath`) — covers, page
/// images, and images inside archive chapters. Guards against path traversal.
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
    // Archive member: `local/<manga>/<chapter>.zip/<page>` — find the archive
    // segment and extract the image by its file name.
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

/// Serves the bundled WebUI: static files when present, otherwise the SPA
/// `index.html` (client-side routing). Mirrors a classic SPA hosting setup.
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

/// Phase 7: locates the Kotlin H2 database, dumps it with tools/h2-dump and
/// imports the generated PostgreSQL script into the configured backend.
async fn import_h2_data(db: &Db, data_dir: &std::path::Path) -> anyhow::Result<()> {
    use sqlx::Executor;

    // 1) locate the H2 file
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

    // 2) resolve the h2-dump jar
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

    // 4) execute statements one by one (FK-safe order is emitted by h2-dump)
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

/// Load a persisted `localSourcePath` (the `settings` global_meta blob saved
/// via `setSettings`) into the process-wide local source root override, so a
/// custom local source directory survives restarts.
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

    // Phase 7: `--migrate <kotlin-data-dir> [--h2-dump-jar <path>]` — dump the
    // Kotlin H2 database via tools/h2-dump and import it into the configured
    // backend, then exit (no HTTP server).
    let migrate_dir: Option<std::path::PathBuf> = {
        let mut args = std::env::args().skip(1);
        let mut dir = None;
        while let Some(a) = args.next() {
            match a.as_str() {
                "--migrate" => dir = args.next().map(std::path::PathBuf::from),
                "--h2-dump-jar" => {
                    if let Some(p) = args.next() {
                        std::env::set_var("SUWAYOMI_H2_DUMP_JAR", p);
                    }
                }
                _ => {}
            }
        }
        dir
    };

    // Database backend (Phase 6): embedded Oliphaunt (renamed pglite-oxide,
    // native PostgreSQL) by default; an explicit `SUWAYOMI_DATABASE_URL`
    // switches to an external PostgreSQL server.
    //   SUWAYOMI_PGLITE_DATA_DIR   -> embedded data directory
    //                                (default ./pglite-data; "" = ephemeral)
    let db = if config.database_url.is_empty() {
        let data_dir = match std::env::var("SUWAYOMI_PGLITE_DATA_DIR") {
            Ok(v) if !v.is_empty() => Some(std::path::PathBuf::from(v)),
            Ok(_) => None, // explicitly empty -> ephemeral (tests/dev)
            Err(_) => Some(std::path::PathBuf::from("pglite-data")),
        };
        tracing::info!("database backend: embedded Oliphaunt PostgreSQL (set SUWAYOMI_DATABASE_URL to use external PostgreSQL)");
        Db::connect_embedded(data_dir.as_deref()).await?
    } else {
        tracing::info!("database backend: external PostgreSQL at {}", config.database_url);
        Db::connect(&config.database_url).await?
    };
    db.migrate().await?;
    tracing::info!(mode = ?db.mode(), "database ready (migrations applied)");

    // Apply a persisted `localSourcePath` (saved via setSettings) to the
    // local source root so custom directories survive restarts.
    load_local_source_path(&db).await;

    // Phase 7: import the Kotlin H2 data and stop.
    if let Some(dir) = migrate_dir {
        import_h2_data(&db, &dir).await?;
        tracing::info!("--migrate finished; run `suwayomi` normally to serve");
        return Ok(());
    }

    // Phase 5: launch the JVM extension sandbox when configured.
    // SUWAYOMI_SANDBOX_JAR  -> path to the built sandbox jar (optional; falls back
    //                          to `jvm-sandbox.jar` next to this executable, the
    //                          bundled layout used by the desktop tray / CI zips)
    // SUWAYOMI_SANDBOX_PORT -> sandbox HTTP port (default 8091; 4501-4900 is
    //                          reserved by Windows for Hyper-V dynamic ports)
    // SUWAYOMI_EXTENSIONS_DIR -> directory holding extension jars (default ./extensions)
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
    // NB: `let _x = sandbox_guard` (NOT `let _ = ...`) keeps the process alive
    // for the whole server lifetime — `let _ =` would drop it immediately and
    // kill the JVM in Drop.
    let _sandbox = sandbox_guard;
    let graphql_state = suwayomi_graphql::GraphQLState::new(db.clone(), config.clone(), fetcher.clone(), sandbox_base.clone());
    let schema = suwayomi_graphql::schema::build_schema(graphql_state);
    tracing::info!("graphql schema ready ({} type definitions)", suwayomi_graphql::schema::schema_type_count());
    let state = AppState::new(db, config.clone(), fetcher, sandbox_base, resolve_webui_dir());
    // Shutdown notification channel: `POST /api/v1/shutdown` (or Ctrl+C)
    // triggers graceful shutdown so the embedded postgres + sandbox child
    // processes are stopped cleanly instead of being orphaned.
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let app = build_router(state, schema, shutdown_tx);

    // Bind with automatic port fallback. On Windows the configured port may be
    // inside the Hyper-V dynamic exclusion range (4501-4900) or already taken,
    // which surfaces as os error 10013 (WSAEACCES) / 10048 (WSAEADDRINUSE).
    // Instead of crashing, walk upward a few ports and log what happened.
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
    // Release every Db reference held through the router (AppState /
    // GraphQLState) so oliphaunt's session Drop runs `pg_ctl stop` on the
    // postgres child and the NativeRootLock is unlocked.
    drop(app);
    // oliphaunt's root locks are unlocked but NOT deleted on Drop — remove
    // the leftover `.oliphaunt-root-*.lock` (data dir's parent) and the
    // `.oliphaunt.lock` marker inside the data dir.
    cleanup_oliphaunt_lock_files();
    Ok(())
}

/// Wait for a shutdown request — Ctrl+C, or the `POST /api/v1/shutdown`
/// endpoint (watch channel) — and then let the runtime unwind normally. This
/// is what lets the embedded Oliphaunt PostgreSQL server stop cleanly: `Db`
/// is dropped as `main` returns and `Oliphaunt`'s session Drop runs `pg_ctl
/// stop` on the child postgres process (and the JVM sandbox child is killed
/// in its Drop). Without graceful shutdown, an abrupt process exit leaves
/// the postgres child running and the next start fails on the
/// `postmaster.pid` lock.
async fn shutdown_signal(mut rx: tokio::sync::watch::Receiver<bool>) {
    tokio::select! {
        _ = tokio::signal::ctrl_c() => tracing::info!("ctrl-c received; graceful shutdown"),
        _ = rx.changed() => tracing::info!("shutdown requested via /api/v1/shutdown; graceful shutdown"),
    }
}

/// Remove the oliphaunt root lock files left behind after a graceful stop:
/// `NativeRootLock` unlocks the files on Drop but does not delete them — the
/// stable lock lives in the data dir's parent (`.oliphaunt-root-<hash>.lock`)
/// and the root marker `.oliphaunt.lock` sits inside the data dir. Deleting
/// them keeps the deployment root clean between runs; they are recreated on
/// the next open.
fn cleanup_oliphaunt_lock_files() {
    let data_dir = std::env::var("SUWAYOMI_PGLITE_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("pglite-data"));
    if let Some(parent) = data_dir.parent() {
        if let Ok(entries) = std::fs::read_dir(parent) {
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
