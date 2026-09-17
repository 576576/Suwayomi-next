//! 认证核心：模式解析、凭据比对、会话 cookie 与 JWT 的签发/校验。
//!
//! 放在 core 是因为三处要共用同一份实现：`suwayomi-rest`（中间件、登录页）、
//! `suwayomi-graphql`（`login` / `refreshToken`）与 `suwayomi-server`（启动期解析）。
//! 会话 cookie 与 JWT 共用同一把密钥，改用户名或换密钥会让两者同时失效。

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

const B64: base64::engine::general_purpose::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// 会话 cookie 名。与上游一致（`JavalinSetup` 写 `logged-in`）。
pub const SESSION_COOKIE: &str = "logged-in";

/// 上游把 token 也接受在 cookie 里，键名叫这个。
pub const TOKEN_COOKIE: &str = "suwayomi-server-token";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Disabled,
    BasicAuth,
    SimpleLogin,
    UiLogin,
}

impl AuthMode {
    /// 解析 `SUWAYOMI_AUTH_MODE` / 设置里的 `authMode`。
    ///
    /// 不认识的取值必须报错：静默退化成 `Disabled` 等于把整个服务敞开，
    /// 而写错一个字母是很容易发生的事（`UI_LOGIN` 曾经就这样被吃掉）。
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_uppercase().as_str() {
            "DISABLED" | "NONE" | "" => Ok(Self::Disabled),
            "BASIC_AUTH" => Ok(Self::BasicAuth),
            "SIMPLE_LOGIN" => Ok(Self::SimpleLogin),
            "UI_LOGIN" => Ok(Self::UiLogin),
            other => Err(format!(
                "unknown auth mode {other:?}; expected one of DISABLED, BASIC_AUTH, SIMPLE_LOGIN, UI_LOGIN"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "DISABLED",
            Self::BasicAuth => "BASIC_AUTH",
            Self::SimpleLogin => "SIMPLE_LOGIN",
            Self::UiLogin => "UI_LOGIN",
        }
    }
}

/// 请求主体。只有两级——上游也是单用户模型（`UserType.Admin` / `Visitor`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Principal {
    Anonymous,
    Authenticated,
}

impl Principal {
    pub fn is_authenticated(self) -> bool {
        matches!(self, Self::Authenticated)
    }
}

/// 运行期认证参数。启动时解析一次（env 优先、设置里的值为兜底），之后只读。
#[derive(Debug)]
pub struct AuthContext {
    pub mode: AuthMode,
    pub username: String,
    pub password: String,
    /// 会话 cookie 与 JWT 共用的 HMAC 密钥。
    secret: Arc<[u8]>,
    pub jwt_audience: String,
    pub token_ttl: Duration,
    pub refresh_ttl: Duration,
    /// 只在 HTTPS 反代后开启（`SUWAYOMI_AUTH_COOKIE_SECURE=1`）。
    pub cookie_secure: bool,
}

impl AuthContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mode: AuthMode,
        username: String,
        password: String,
        secret: Arc<[u8]>,
        jwt_audience: String,
        token_ttl: Duration,
        refresh_ttl: Duration,
        cookie_secure: bool,
    ) -> Self {
        Self { mode, username, password, secret, jwt_audience, token_ttl, refresh_ttl, cookie_secure }
    }

    /// 关闭认证的实例（`DISABLED`，以及不需要认证的测试/工具场景）。
    pub fn disabled() -> Self {
        Self::new(
            AuthMode::Disabled,
            String::new(),
            String::new(),
            Arc::from(vec![0u8; 32].into_boxed_slice()),
            String::new(),
            Duration::from_secs(300),
            Duration::from_secs(60 * 60 * 24 * 60),
            false,
        )
    }

    /// 用户名 + 密码比对。空用户名或空密码一律不通过——否则「什么都没配」
    /// 会变成「人人可登录」。
    ///
    /// 认证关闭时也一律不通过：`DISABLED` 的实例不该签发任何凭据。配置里可能
    /// 还留着上次用过的用户名密码，放行会让人在认证关闭期间先换好令牌，等管理员
    /// 打开认证后继续用（刷新令牌默认有效 60 天）。
    pub fn verify_credentials(&self, username: &str, password: &str) -> bool {
        if self.is_disabled() || self.username.is_empty() || self.password.is_empty() {
            return false;
        }
        constant_time_eq(username.as_bytes(), self.username.as_bytes())
            & constant_time_eq(password.as_bytes(), self.password.as_bytes())
    }

    pub fn is_disabled(&self) -> bool {
        self.mode == AuthMode::Disabled
    }

    // ---------- 会话 cookie ----------

    /// payload = `v1:<username>:<exp_unix>`，签名覆盖 payload 原文。
    pub fn session_cookie(&self, now: SystemTime) -> String {
        let exp = now + session_ttl();
        let payload = format!("v1:{}:{}", self.username, unix_secs(exp));
        let sig = self.sign(payload.as_bytes());
        format!("{payload}.{}", B64.encode(sig))
    }

    /// 校验签名、过期时间与用户名三者，任一不过视为匿名。
    pub fn verify_session_cookie(&self, value: &str, now: SystemTime) -> bool {
        let Some((payload, sig)) = value.rsplit_once('.') else {
            return false;
        };
        let Ok(sig) = B64.decode(sig) else {
            return false;
        };
        if !self.verify_signature(payload.as_bytes(), &sig) {
            return false;
        }
        let mut parts = payload.splitn(3, ':');
        let (Some("v1"), Some(user), Some(exp)) = (parts.next(), parts.next(), parts.next()) else {
            return false;
        };
        let Ok(exp) = exp.parse::<u64>() else {
            return false;
        };
        user == self.username && exp > unix_secs(now)
    }

    /// `Set-Cookie` 的完整值。`HttpOnly` 保证 JS 读不到，`SameSite=Lax` 挡掉
    /// 跨站写请求（CSRF 中间件是第二道）。
    pub fn session_set_cookie(&self, now: SystemTime) -> String {
        let max_age = if self.is_disabled() { 0 } else { session_ttl().as_secs() };
        let secure = if self.cookie_secure { "; Secure" } else { "" };
        format!(
            "{SESSION_COOKIE}={}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure}",
            self.session_cookie(now)
        )
    }

    /// 清 cookie（登出）。
    pub fn session_clear_cookie(&self) -> String {
        let secure = if self.cookie_secure { "; Secure" } else { "" };
        format!("{SESSION_COOKIE}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax{secure}")
    }

    // ---------- JWT ----------

    /// 签发（access, refresh）。`typ` 进 claims，两种 token 不能互相顶替。
    pub fn issue_tokens(&self, now: SystemTime) -> (String, String) {
        let iat = unix_secs(now);
        let access = self.sign_jwt("access", iat + self.token_ttl.as_secs(), iat);
        let refresh = self.sign_jwt("refresh", iat + self.refresh_ttl.as_secs(), iat);
        (access, refresh)
    }

    pub fn issue_access_token(&self, now: SystemTime) -> String {
        let iat = unix_secs(now);
        self.sign_jwt("access", iat + self.token_ttl.as_secs(), iat)
    }

    /// 校验签名 + `alg` + `aud` + `typ` + 过期。
    pub fn verify_token(&self, token: &str, kind: &str, now: SystemTime) -> bool {
        let mut parts = token.split('.');
        let (Some(header_b64), Some(payload_b64), Some(sig_b64), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        // 只认 HS256：`alg: none` 与 RS256 混淆都是 JWT 的经典漏洞
        let Some(header) = decode_json(header_b64) else {
            return false;
        };
        if header.get("alg").and_then(|v| v.as_str()) != Some("HS256") {
            return false;
        }
        let Ok(sig) = B64.decode(sig_b64) else {
            return false;
        };
        if !self.verify_signature(format!("{header_b64}.{payload_b64}").as_bytes(), &sig) {
            return false;
        }
        let Some(claims) = decode_json(payload_b64) else {
            return false;
        };
        let str_claim = |key: &str| claims.get(key).and_then(|v| v.as_str()).unwrap_or_default();
        let num_claim = |key: &str| claims.get(key).and_then(|v| v.as_u64()).unwrap_or_default();
        str_claim("sub") == self.username
            && str_claim("typ") == kind
            && (self.jwt_audience.is_empty() || str_claim("aud") == self.jwt_audience)
            && num_claim("exp") > unix_secs(now)
    }

    fn sign_jwt(&self, kind: &str, exp: u64, iat: u64) -> String {
        let header = B64.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
        let claims = serde_json::json!({
            "sub": self.username,
            "aud": self.jwt_audience,
            "typ": kind,
            "iat": iat,
            "exp": exp,
        });
        let payload = B64.encode(serde_json::to_vec(&claims).unwrap_or_default());
        let signing_input = format!("{header}.{payload}");
        let sig = self.sign(signing_input.as_bytes());
        format!("{signing_input}.{}", B64.encode(sig))
    }

    fn sign(&self, data: &[u8]) -> Vec<u8> {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("hmac accepts any key length");
        mac.update(data);
        mac.finalize().into_bytes().to_vec()
    }

    fn verify_signature(&self, data: &[u8], sig: &[u8]) -> bool {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("hmac accepts any key length");
        mac.update(data);
        // verify_slice 做的是常数时间比较
        mac.verify_slice(sig).is_ok()
    }
}

/// 会话有效期。上游是 30 分钟的 Javalin session 默认值。
fn session_ttl() -> Duration {
    Duration::from_secs(30 * 60)
}

/// 解析时长配置（`SUWAYOMI_JWT_TOKEN_EXPIRY` / 设置里的 `jwtTokenExpiry` 等）。
///
/// 同时接受简写（`5m` / `30s` / `2h` / `60d` / 裸秒数 `300`）与 ISO-8601
/// （`PT5M` / `P60D` / `PT1H30M`）——环境变量里写简写更顺手，而 GraphQL 的
/// `Duration` 标量序列化出来是 ISO，两条路进的是同一个解析函数。
///
/// 不支持月/年：`P1M` 是 1 个月而不是 1 分钟（ISO 的日期段里 `M` 是月），
/// 猜错就是数量级错误，所以直接判非法。下限 1 秒，避免写出 0 导致一签发就过期。
pub fn parse_duration(value: &str) -> Option<Duration> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (body, date_section) = match value.strip_prefix("PT").or_else(|| value.strip_prefix("pt")) {
        Some(rest) => (rest, false),
        None => match value.strip_prefix('P').or_else(|| value.strip_prefix('p')) {
            Some(rest) => (rest, true),
            None => (value, false),
        },
    };

    let mut total = 0.0f64;
    let mut num = String::new();
    let mut seen_unit = false;
    for c in body.chars() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
            continue;
        }
        let n: f64 = num.parse().ok()?;
        num.clear();
        seen_unit = true;
        match c.to_ascii_uppercase() {
            'D' => total += n * 86400.0,
            'H' => total += n * 3600.0,
            // 日期段里的 M 是月（`P1M`），不是分钟
            'M' if !date_section => total += n * 60.0,
            'S' if !date_section => total += n,
            _ => return None,
        }
    }
    if !seen_unit {
        // 裸数字按秒处理（`300`）
        let secs: f64 = value.parse().ok()?;
        return Some(Duration::from_secs_f64(secs.max(1.0)));
    }
    if !num.is_empty() {
        return None;
    }
    Some(Duration::from_secs_f64(total.max(1.0)))
}

/// base64url 解出 JSON 对象；任何一步失败都是 `None`（调用方一律视为无效凭据）。
fn decode_json(part: &str) -> Option<serde_json::Value> {
    let bytes = B64.decode(part).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn now() -> SystemTime {
    SystemTime::now()
}

/// 常数时间比较，避免用 `==` 逐字节短路泄漏信息。
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// 会话/JWT 密钥：`SUWAYOMI_SESSION_SECRET` → 否则 `<dir>/session.key`。
///
/// 落盘是为了让重启后旧 cookie 仍然有效；文件权限在 unix 上是 0600。
pub fn load_or_create_secret(dir: &Path) -> std::io::Result<(Arc<[u8]>, SecretSource)> {
    if let Ok(secret) = std::env::var("SUWAYOMI_SESSION_SECRET")
        && !secret.trim().is_empty()
    {
        let bytes = secret.into_bytes();
        if bytes.len() < 32 {
            tracing::warn!(
                "SUWAYOMI_SESSION_SECRET is shorter than 32 bytes; use a long random value"
            );
        }
        return Ok((Arc::from(bytes.into_boxed_slice()), SecretSource::Env));
    }

    let path = dir.join("session.key");
    if let Ok(bytes) = std::fs::read(&path) {
        if bytes.is_empty() {
            tracing::warn!("{} is empty; regenerating", path.display());
        } else {
            return Ok((Arc::from(bytes.into_boxed_slice()), SecretSource::File(path)));
        }
    }

    let mut secret = vec![0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::create_dir_all(dir)?;
    write_secret_file(&path, &secret)?;
    Ok((Arc::from(secret.into_boxed_slice()), SecretSource::File(path)))
}

#[cfg(unix)]
fn write_secret_file(path: &Path, secret: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    file.write_all(secret)
}

#[cfg(not(unix))]
fn write_secret_file(path: &Path, secret: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, secret)
}

/// 密钥来源，仅用于启动日志。
#[derive(Debug)]
pub enum SecretSource {
    Env,
    File(std::path::PathBuf),
    /// 认证关闭时用的临时密钥（进程重启即变，不落盘）。
    Ephemeral,
}

impl SecretSource {
    pub fn describe(&self) -> String {
        match self {
            Self::Env => "SUWAYOMI_SESSION_SECRET".to_string(),
            Self::File(p) => p.display().to_string(),
            Self::Ephemeral => "ephemeral (auth disabled)".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(mode: AuthMode) -> AuthContext {
        AuthContext::new(
            mode,
            "admin".into(),
            "secret".into(),
            Arc::from(b"0123456789abcdef0123456789abcdef".to_vec().into_boxed_slice()),
            "suwayomi-server-api".into(),
            Duration::from_secs(300),
            Duration::from_secs(3600),
            false,
        )
    }

    #[test]
    fn unknown_mode_is_an_error() {
        assert!(AuthMode::parse("UI_LOGIN").is_ok());
        assert_eq!(AuthMode::parse("ui_login").unwrap(), AuthMode::UiLogin);
        assert_eq!(AuthMode::parse("none").unwrap(), AuthMode::Disabled);
        assert!(AuthMode::parse("BASIC").is_err());
    }

    #[test]
    fn session_cookie_round_trip() {
        let c = ctx(AuthMode::SimpleLogin);
        let now = now();
        let cookie = c.session_cookie(now);
        assert!(c.verify_session_cookie(&cookie, now));
        // 换个用户名（等价于换了密钥的实例）不通过
        let other = AuthContext::new(
            AuthMode::SimpleLogin,
            "someone".into(),
            "secret".into(),
            Arc::from(b"0123456789abcdef0123456789abcdef".to_vec().into_boxed_slice()),
            String::new(),
            Duration::from_secs(300),
            Duration::from_secs(3600),
            false,
        );
        assert!(!other.verify_session_cookie(&cookie, now));
        // 篡改 payload 不通过
        assert!(!c.verify_session_cookie("v1:admin:9999999999.xxx", now));
        // 过期不通过
        assert!(!c.verify_session_cookie(&cookie, now + Duration::from_secs(31 * 60)));
        // 只判断 cookie 名存在的老写法必须失效
        assert!(!c.verify_session_cookie("x", now));
    }

    #[test]
    fn jwt_round_trip() {
        let c = ctx(AuthMode::UiLogin);
        let now = now();
        let (access, refresh) = c.issue_tokens(now);
        assert!(c.verify_token(&access, "access", now));
        assert!(!c.verify_token(&access, "refresh", now));
        assert!(c.verify_token(&refresh, "refresh", now));
        assert!(!c.verify_token(&access, "access", now + Duration::from_secs(400)));
        assert!(!c.verify_token("a.b.c", "access", now));
        // alg:none 必须被拒
        let header = B64.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = B64.encode(br#"{"sub":"admin","typ":"access","aud":"suwayomi-server-api","exp":99999999999}"#);
        assert!(!c.verify_token(&format!("{header}.{payload}."), "access", now));
    }

    #[test]
    fn empty_credentials_never_authenticate() {
        let c = AuthContext::new(
            AuthMode::BasicAuth,
            String::new(),
            String::new(),
            Arc::from(vec![0u8; 32].into_boxed_slice()),
            String::new(),
            Duration::from_secs(300),
            Duration::from_secs(3600),
            false,
        );
        assert!(!c.verify_credentials("", ""));
    }

    /// 认证关掉了就不能再签发凭据：配置里往往还留着上次的用户名密码，
    /// 放行等于给「先关认证、后开认证」留一个提前换令牌的窗口。
    #[test]
    fn disabled_mode_issues_no_credentials() {
        let c = ctx(AuthMode::Disabled);
        assert!(c.is_disabled());
        assert!(!c.verify_credentials("admin", "secret"));
    }

    #[test]
    fn duration_forms() {
        assert_eq!(parse_duration("5m"), Some(Duration::from_secs(300)));
        assert_eq!(parse_duration("60d"), Some(Duration::from_secs(60 * 86400)));
        assert_eq!(parse_duration("PT5M"), Some(Duration::from_secs(300)));
        assert_eq!(parse_duration("P60D"), Some(Duration::from_secs(60 * 86400)));
        assert_eq!(parse_duration("PT1H30M"), Some(Duration::from_secs(5400)));
        assert_eq!(parse_duration("300"), Some(Duration::from_secs(300)));
        // 0 会被抬到 1 秒，避免签出立刻就过期的 token
        assert_eq!(parse_duration("PT0S"), Some(Duration::from_secs(1)));
        assert_eq!(parse_duration("nonsense"), None);
        assert_eq!(parse_duration(""), None);
        // 月/年不支持
        assert_eq!(parse_duration("P1M"), None);
    }
}
