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

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = config_from_env();
    tracing::info!(name = "Suwayomi (next)", version = VERSION, "starting");

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

    // Phase 5: launch the JVM extension sandbox when configured.
    // SUWAYOMI_SANDBOX_JAR  -> path to the built sandbox jar (optional)
    // SUWAYOMI_SANDBOX_PORT -> sandbox HTTP port (default 4569)
    // SUWAYOMI_EXTENSIONS_DIR -> directory holding extension jars (default ./extensions)
    let mut sandbox_guard: Option<suwayomi_domain::source::sandbox::SandboxProcess> = None;
    if let Ok(jar) = std::env::var("SUWAYOMI_SANDBOX_JAR") {
        let port = std::env::var("SUWAYOMI_SANDBOX_PORT").unwrap_or_else(|_| "4569".into());
        let proc = suwayomi_domain::source::sandbox::SandboxProcess::start(&jar, &port).await;
        match proc {
            Ok(p) => {
                tracing::info!("jvm sandbox connected at 127.0.0.1:{port}");
                sandbox_guard = Some(p);
            }
            Err(e) => tracing::warn!("jvm sandbox failed to start: {e}; falling back to StubFetcher"),
        }
    }

    let fetcher: Arc<dyn SourceFetcher> =
        if let Some(guard) = &sandbox_guard { Arc::new(guard.fetcher()) } else { Arc::new(StubFetcher) };
    let _ = sandbox_guard; // keep process alive for the server lifetime
    let graphql_state = suwayomi_graphql::GraphQLState::new(db.clone(), config.clone(), fetcher.clone());
    let schema = suwayomi_graphql::schema::build_schema(graphql_state);
    tracing::info!("graphql schema ready ({} type definitions)", suwayomi_graphql::schema::schema_type_count());
    let state = AppState::new(db, config.clone(), fetcher);
    let app = build_router(state, schema);

    let addr: SocketAddr = format!("{}:{}", config.ip, config.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("server listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
