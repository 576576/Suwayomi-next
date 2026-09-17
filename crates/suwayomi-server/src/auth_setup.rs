//! 认证参数的启动期解析。
//!
//! 优先级：环境变量 → 设置（`global_meta['settings']` blob，WebUI 写的那份）
//! → `ServerConfig` 默认值。改完设置必须重启才生效（与 `dataDir` 同构），
//! 所以设置页只需要把值存下来，不必热更新运行中的进程。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use suwayomi_core::auth::{AuthContext, AuthMode, SecretSource, load_or_create_secret, parse_duration};
use suwayomi_core::config::ServerConfig;

/// 解析结果与密钥来源（后者只用于启动日志）。
pub fn resolve(
    config: &ServerConfig,
    blob: Option<&serde_json::Value>,
    db_dir: &Path,
) -> anyhow::Result<(AuthContext, SecretSource)> {
    let mode = AuthMode::parse(&pick("SUWAYOMI_AUTH_MODE", blob, "authMode", &config.auth_mode))
        .map_err(|e| anyhow::anyhow!("{e} (SUWAYOMI_AUTH_MODE / settings.authMode)"))?;

    let username = pick("SUWAYOMI_AUTH_USERNAME", blob, "authUsername", &config.auth_username);
    let password = pick("SUWAYOMI_AUTH_PASSWORD", blob, "authPassword", &config.auth_password);
    let jwt_audience = pick("SUWAYOMI_JWT_AUDIENCE", blob, "jwtAudience", &config.jwt_audience);

    let token_ttl = duration_setting(
        "SUWAYOMI_JWT_TOKEN_EXPIRY",
        blob,
        "jwtTokenExpiry",
        &config.jwt_token_expiry,
        Duration::from_secs(5 * 60),
    );
    let refresh_ttl = duration_setting(
        "SUWAYOMI_JWT_REFRESH_EXPIRY",
        blob,
        "jwtRefreshExpiry",
        &config.jwt_refresh_expiry,
        Duration::from_secs(60 * 24 * 3600),
    );

    let cookie_secure = matches!(
        std::env::var("SUWAYOMI_AUTH_COOKIE_SECURE").unwrap_or_default().trim().to_lowercase().as_str(),
        "1" | "true" | "yes"
    );

    // 关掉认证时不需要落盘密钥：省得给纯本地部署凭空多出一个 session.key
    let (secret, source) = if mode == AuthMode::Disabled {
        let mut seed = vec![0u8; 32];
        getrandom::fill(&mut seed).map_err(|e| anyhow::anyhow!("generating session secret: {e}"))?;
        (Arc::from(seed.into_boxed_slice()), SecretSource::Ephemeral)
    } else {
        let (secret, source) = load_or_create_secret(db_dir)?;
        (secret, source)
    };

    if mode != AuthMode::Disabled && (username.is_empty() || password.is_empty()) {
        anyhow::bail!(
            "auth mode {} requires a non-empty username and password (set SUWAYOMI_AUTH_USERNAME / \
             SUWAYOMI_AUTH_PASSWORD, or save them in the WebUI server settings and restart)",
            mode.as_str()
        );
    }

    Ok((
        AuthContext::new(mode, username, password, secret, jwt_audience, token_ttl, refresh_ttl, cookie_secure),
        source,
    ))
}

fn pick(env_key: &str, blob: Option<&serde_json::Value>, blob_key: &str, fallback: &str) -> String {
    if let Ok(value) = std::env::var(env_key)
        && !value.trim().is_empty()
    {
        return value.trim().to_string();
    }
    if let Some(value) = blob.and_then(|b| b.get(blob_key)).and_then(|v| v.as_str())
        && !value.trim().is_empty()
    {
        return value.trim().to_string();
    }
    fallback.to_string()
}

fn duration_setting(
    env_key: &str,
    blob: Option<&serde_json::Value>,
    blob_key: &str,
    fallback: &str,
    default: Duration,
) -> Duration {
    let raw = pick(env_key, blob, blob_key, fallback);
    match parse_duration(&raw) {
        Some(value) => value,
        None => {
            if !raw.is_empty() {
                tracing::warn!("ignoring unparsable duration {raw:?} for {blob_key}");
            }
            default
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这些用例读环境变量，机器上真配了 `SUWAYOMI_AUTH_*` 就跳过，避免误判。
    fn env_is_clean() -> bool {
        ["SUWAYOMI_AUTH_MODE", "SUWAYOMI_AUTH_USERNAME", "SUWAYOMI_AUTH_PASSWORD"]
            .iter()
            .all(|key| std::env::var(key).map(|v| v.trim().is_empty()).unwrap_or(true))
    }

    #[test]
    fn unknown_mode_fails_startup() {
        if !env_is_clean() {
            return;
        }
        let config = ServerConfig { auth_mode: "BASIC".into(), ..ServerConfig::default() };
        let err = resolve(&config, None, &std::env::temp_dir()).unwrap_err().to_string();
        assert!(err.contains("unknown auth mode"), "{err}");
    }

    #[test]
    fn enabled_mode_requires_credentials() {
        if !env_is_clean() {
            return;
        }
        let config = ServerConfig { auth_mode: "SIMPLE_LOGIN".into(), ..ServerConfig::default() };
        assert!(resolve(&config, None, &std::env::temp_dir()).is_err());
    }
}
