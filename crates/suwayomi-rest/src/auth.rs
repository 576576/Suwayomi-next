//! Authentication — mirrors `JavalinSetup.kt` `beforeMatched` rules.
//! `DISABLED` / `BASIC_AUTH` / `SIMPLE_LOGIN` modes with the same
//! exceptions (login.html, site.webmanifest, manifest.json, page icons,
//! preflight OPTIONS are exempt).

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use base64::Engine;

use crate::state::AppState;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Disabled,
    Basic,
    SimpleLogin,
}

impl AuthMode {
    pub fn from_config_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "BASIC_AUTH" => Self::Basic,
            "SIMPLE_LOGIN" => Self::SimpleLogin,
            _ => Self::Disabled,
        }
    }
}

fn is_exempt(path: &str, method: &str) -> bool {
    if method == "OPTIONS" {
        return true;
    }
    // Graceful shutdown endpoint — loopback-only is enforced by the handler
    // itself (see main.rs /api/v1/shutdown).
    if path == "/api/v1/shutdown" {
        return true;
    }
    if path.ends_with("login.html") || path.ends_with("site.webmanifest") || path.ends_with("manifest.json") {
        return true;
    }
    // page-level icons: single path segment ending in png/jpg/ico
    let trimmed = path.trim_start_matches('/');
    if !trimmed.contains('/') && (trimmed.ends_with(".png") || trimmed.ends_with(".jpg") || trimmed.ends_with(".ico")) {
        return true;
    }
    false
}

fn basic_valid(state: &AppState, req: &Request<Body>) -> bool {
    let Some(auth) = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Some(encoded) = auth.strip_prefix("Basic ") else {
        return false;
    };
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
        return false;
    };
    let Ok(text) = String::from_utf8(decoded) else {
        return false;
    };
    let Some((user, pass)) = text.split_once(':') else {
        return false;
    };
    user == state.config.auth_username && pass == state.config.auth_password
}

pub async fn require_auth(State(state): State<AppState>, req: Request<Body>, next: Next) -> Response {
    let mode = AuthMode::from_config_str(&state.config.auth_mode);
    let path = req.uri().path().to_string();
    let method = req.method().as_str().to_string();

    if mode == AuthMode::Disabled || is_exempt(&path, &method) {
        return next.run(req).await;
    }

    match mode {
        AuthMode::Basic => {
            if !basic_valid(&state, &req) {
                return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Basic")]).into_response();
            }
            next.run(req).await
        }
        AuthMode::SimpleLogin => {
            let cookie_ok = req
                .headers()
                .get(header::COOKIE)
                .and_then(|v| v.to_str().ok())
                .map(|c| c.contains("logged-in="))
                .unwrap_or(false);
            if cookie_ok || path.starts_with("/api/") {
                next.run(req).await
            } else {
                let target = format!("/login.html?redirect={}", path);
                Redirect::to(&target).into_response()
            }
        }
        AuthMode::Disabled => next.run(req).await,
    }
}
