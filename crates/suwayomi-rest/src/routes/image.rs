//! `/api/v1/image/{b64url}` — external image proxy with a disk cache.
//!
//! Extension covers point at upstream CDNs (zrocdn.xyz, i2.nhentaimg.com,
//! cdn.nhentai.com, …). Loading them directly from the browser fails when the
//! CDN omits CORS headers (the WebUI sets `crossOrigin='anonymous'`), and
//! every browse re-downloads the artwork. Serving covers through this
//! same-origin endpoint fixes CORS and caches the bytes under
//! `<cache root>/images/`, keyed by a FNV-1a hash of the URL (16 hex chars —
//! short enough for Windows path limits).

use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine;

use crate::state::AppState;
use suwayomi_core::config::cache_root;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new().route("/{b64}", axum::routing::get(proxy_image))
}

/// FNV-1a 64-bit — cheap, stable cache key (URLs here are few, collisions
/// irrelevant for a local image cache).
fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

async fn proxy_image(State(_s): State<AppState>, Path(b64): Path<String>) -> Response {
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&b64)
        .ok()
        .and_then(|b| String::from_utf8(b).ok());
    let Some(url) = decoded else {
        return err_response(StatusCode::BAD_REQUEST, "bad base64 url");
    };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return err_response(StatusCode::BAD_REQUEST, "http(s) only");
    }

    let root = cache_root().join("images");
    let key = format!("{:016x}", fnv1a64(&url));
    let img_path = root.join(format!("{key}.img"));
    let mime_path = root.join(format!("{key}.mime"));

    // cache hit
    if let Ok(bytes) = tokio::fs::read(&img_path).await {
        let ct = tokio::fs::read_to_string(&mime_path)
            .await
            .unwrap_or_else(|_| "application/octet-stream".into());
        return ok_response(bytes, &ct);
    }

    // upstream fetch
    let client = match reqwest::Client::builder()
        .user_agent("Suwayomi-next/1.0")
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, "http client"),
    };
    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(_) => return err_response(StatusCode::BAD_GATEWAY, "upstream unreachable"),
    };
    if !resp.status().is_success() {
        return err_response(StatusCode::BAD_GATEWAY, "upstream error");
    }
    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let bytes = match resp.bytes().await {
        Ok(b) => b.to_vec(),
        Err(_) => return err_response(StatusCode::BAD_GATEWAY, "upstream read failed"),
    };

    // persist for next time
    let _ = tokio::fs::create_dir_all(&root).await;
    let _ = tokio::fs::write(&img_path, &bytes).await;
    let _ = tokio::fs::write(&mime_path, ct.as_bytes()).await;

    ok_response(bytes, &ct)
}

fn ok_response(bytes: Vec<u8>, content_type: &str) -> Response {
    let mut r = (bytes).into_response();
    if let Ok(v) = HeaderValue::from_str(content_type) {
        r.headers_mut().insert(header::CONTENT_TYPE, v);
    }
    // covers are immutable by URL — long-lived cache is safe
    if let Ok(v) = HeaderValue::from_str("public, max-age=31536000, immutable") {
        r.headers_mut().insert(header::CACHE_CONTROL, v);
    }
    r
}

fn err_response(status: StatusCode, msg: &str) -> Response {
    (status, msg.to_string()).into_response()
}
