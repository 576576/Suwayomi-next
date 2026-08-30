//! Suwayomi server entry point.
//!
//! Mirrors Suwayomi `Main.kt` + `JavalinSetup.kt`:
//! - application setup: config → database → migrations
//! - HTTP server (axum) with `/api/v1/**` (Phase 3), GraphQL (Phase 4),
//!   OPDS (Phase 6) and static WebUI hosting.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::get;
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

fn build_router(state: AppState, graphql_schema: suwayomi_graphql::schema::GraphQLSchema) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/v1", get(index))
        .nest("/api/v1", suwayomi_rest::routes::api_v1_router())
        .nest("/api", suwayomi_graphql::schema::graphql_router(graphql_schema))
        .nest("/api/opds/v1.2", suwayomi_opds::router::opds_router())
        .layer(middleware::from_fn_with_state(state.clone(), suwayomi_rest::auth::require_auth))
        .with_state(state)
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // `-v` / `--version`：打印版本信息后退出（替代打包产物里的 VERSION.txt）
    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    if cli_args.iter().any(|a| a == "-v" || a == "--version") {
        println!("Suwayomi {VERSION}");
        println!("{}", env!("CARGO_PKG_REPOSITORY"));
        return Ok(());
    }

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

    // Database backend (Phase 6): embedded PGlite by default; an explicit
    // `SUWAYOMI_DATABASE_URL` switches to an external PostgreSQL server.
    //   SUWAYOMI_PGLITE_DATA_DIR   -> embedded data directory
    //                                (default ./pglite-data; "" = ephemeral)
    let db = if config.database_url.is_empty() {
        let data_dir = match std::env::var("SUWAYOMI_PGLITE_DATA_DIR") {
            Ok(v) if !v.is_empty() => Some(std::path::PathBuf::from(v)),
            Ok(_) => None, // explicitly empty -> ephemeral (tests/dev)
            Err(_) => Some(std::path::PathBuf::from("pglite-data")),
        };
        tracing::info!("database backend: embedded PGlite (set SUWAYOMI_DATABASE_URL to use external PostgreSQL)");
        Db::connect_embedded(data_dir.as_deref()).await?
    } else {
        tracing::info!("database backend: external PostgreSQL at {}", config.database_url);
        Db::connect(&config.database_url).await?
    };
    db.migrate().await?;
    tracing::info!(mode = ?db.mode(), "database ready (migrations applied)");

    // Phase 7: import the Kotlin H2 data and stop.
    if let Some(dir) = migrate_dir {
        import_h2_data(&db, &dir).await?;
        tracing::info!("--migrate finished; run `suwayomi` normally to serve");
        return Ok(());
    }

    // Phase 5: launch the JVM extension sandbox when configured.
    // SUWAYOMI_SANDBOX_JAR  -> path to the built sandbox jar (optional)
    // SUWAYOMI_SANDBOX_PORT -> sandbox HTTP port (default 8091; 4501-4900 is
    //                          reserved by Windows for Hyper-V dynamic ports)
    // SUWAYOMI_EXTENSIONS_DIR -> directory holding extension jars (default ./extensions)
    let mut sandbox_guard: Option<suwayomi_domain::source::sandbox::SandboxProcess> = None;
    if let Ok(jar) = std::env::var("SUWAYOMI_SANDBOX_JAR") {
        let port = std::env::var("SUWAYOMI_SANDBOX_PORT").unwrap_or_else(|_| "8091".into());
        let proc = suwayomi_domain::source::sandbox::SandboxProcess::start(&jar, &port).await;
        match proc {
            Ok(p) => {
                tracing::info!("jvm sandbox connected at 127.0.0.1:{port}");
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
    let state = AppState::new(db, config.clone(), fetcher, sandbox_base);
    let app = build_router(state, schema);

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
    axum::serve(listener, app).await?;
    Ok(())
}
