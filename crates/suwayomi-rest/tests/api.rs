//! REST API v1 integration tests — full router against PostgreSQL.
//! Requires `DATABASE_URL`; skipped when absent.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use suwayomi_core::config::ServerConfig;
use suwayomi_core::db::Db;
use suwayomi_domain::source::StubFetcher;
use suwayomi_rest::routes::api_v1_router;
use suwayomi_rest::AppState;
use tower::ServiceExt;

async fn setup() -> Option<(Router, sqlx::postgres::PgPool)> {
    let url = std::env::var("DATABASE_URL").or_else(|_| std::env::var("SUWAYOMI_TEST_DB")).ok()?;
    let db = Db::connect(&url).await.expect("connect postgres");
    db.migrate().await.expect("migrate");
    for t in [
        "track_search",
        "track_record",
        "extension_store",
        "global_meta",
        "source_meta",
        "manga_meta",
        "chapter_meta",
        "category_meta",
        "category_manga",
        "page",
        "chapter",
        "manga",
        "category",
        "source",
        "extension",
    ] {
        let _ = sqlx::query(&format!("TRUNCATE TABLE suwayomi.{t} RESTART IDENTITY CASCADE")).execute(db.pool()).await;
    }
    let pool = db.pool().clone();
    let fetcher: Arc<dyn suwayomi_domain::source::SourceFetcher> = Arc::new(StubFetcher);
    let state = AppState::new(db, ServerConfig::default(), fetcher);
    Some((api_v1_router().with_state(state), pool))
}

async fn send(app: &Router, req: Request<Body>) -> (StatusCode, String) {
    let resp = app.clone().oneshot(req).await.expect("send request");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("collect body").to_bytes();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

fn req(method: &str, uri: &str, body: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(b) = body {
        builder = builder.header("content-type", "application/json");
        return builder.body(Body::from(b.to_string())).unwrap();
    }
    builder.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn category_crud_via_http() {
    let Some((app, _pool)) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };

    // create two categories
    let (status, body) = send(&app, req("POST", "/category", Some(r#"{"name":"Action"}"#))).await;
    assert_eq!(status, StatusCode::OK, "create category: {body}");
    let (status, _) = send(&app, req("POST", "/category", Some(r#"{"name":"Drama"}"#))).await;
    assert_eq!(status, StatusCode::OK);

    // list categories
    let (status, body) = send(&app, req("GET", "/category", None)).await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2, "two categories created: {body}");
    let names: Vec<&str> = arr.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"Action"));

    // modify category name
    let (status, _) = send(&app, req("PATCH", "/category/1", Some(r#"{"name":"Action2"}"#))).await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = send(&app, req("GET", "/category", None)).await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v.as_array().unwrap().iter().any(|c| c["name"] == "Action2"));

    // delete category
    let (status, _) = send(&app, req("DELETE", "/category/2", None)).await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = send(&app, req("GET", "/category", None)).await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn manga_meta_and_library_via_http() {
    let Some((app, pool)) = setup().await else {
        eprintln!("skipped: DATABASE_URL not set");
        return;
    };

    // seed a manga directly
    let manga_id: i32 = sqlx::query_scalar(
        "INSERT INTO suwayomi.manga (url, title, source, initialized) VALUES ('/m/1', 'Seed', 1, TRUE) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("seed manga");

    // add to library
    let (status, body) = send(&app, req("GET", &format!("/manga/{manga_id}/library"), None)).await;
    assert_eq!(status, StatusCode::OK, "add to library: {body}");

    // meta upsert
    let (status, body) =
        send(&app, req("PATCH", &format!("/manga/{manga_id}/meta"), Some(r#"{"key":"test","value":"v1"}"#))).await;
    assert_eq!(status, StatusCode::OK, "set meta: {body}");

    // read back via full detail
    let (status, body) = send(&app, req("GET", &format!("/manga/{manga_id}/full"), None)).await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["id"], manga_id);
    assert_eq!(v["inLibrary"], true);
    assert_eq!(v["title"], "Seed");

    // 404 for missing manga
    let (status, _) = send(&app, req("GET", "/manga/99999", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
