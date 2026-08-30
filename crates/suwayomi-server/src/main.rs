//! Suwayomi server entry point.
//!
//! Mirrors Suwayomi-Server `Main.kt` + `JavalinSetup.kt`:
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
    let mut cfg = ServerConfig {
        database_type: suwayomi_core::config::DatabaseType::Postgresql,
        ..ServerConfig::default()
    };
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
        .layer(middleware::from_fn_with_state(state.clone(), suwayomi_rest::auth::require_auth))
        .with_state(state)
}

async fn index(State(_s): State<AppState>) -> Result<String, StatusCode> {
    Ok(format!("Suwayomi (next) v{VERSION} — GraphQL at /api/graphql, REST at /api/v1, OPDS/WebUI in Phase 6"))
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

    let db_url = if config.database_url.is_empty() {
        "postgres://postgres:postgres@localhost:5432/postgres".to_string()
    } else {
        config.database_url.clone()
    };
    let db = Db::connect(&db_url).await?;
    db.migrate().await?;
    tracing::info!("database ready (migrations applied)");

    let fetcher: Arc<dyn SourceFetcher> = Arc::new(StubFetcher);
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
