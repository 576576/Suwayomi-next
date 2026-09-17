//! 认证中间件、登录页与静态路径的公开判定。
//!
//! 与上游同样是两层结构，但分工按 axum 的路由边界划：
//!
//! * **这一层**管「页面 / 数据路径」的门禁与凭据解析，并把 [`Principal`] 注入
//!   请求扩展供 handler 取用；同时负责登录流程（`/login.html`、`/logout`）与
//!   静态资源的公开判定。
//! * **GraphQL 那一层**（`/api/graphql`）自己按操作判定——因为 `login` 与
//!   `refreshToken` 必须匿名可达，而这是路径判不出来的。`/api/graphql` 在这里
//!   只做凭据解析，不做拦截。
//!
//! 任何未识别的情况都倾向拒绝：豁免是「算出来的」（safe_join 命中 webui 目录
//! 里的真实文件），不是维护一张文件名清单。

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use base64::Engine;
use suwayomi_core::auth::{AuthContext, AuthMode, Principal, SESSION_COOKIE, TOKEN_COOKIE, now};

use crate::state::AppState;

/// 登录页与登出：必须匿名可达，否则没人能登录进来。
pub fn is_login_flow(path: &str) -> bool {
    matches!(path, "/login.html" | "/logout" | "/logout.html")
}

/// 数据接口：匿名一律 401（页面则是 302 / 401 挑战）。
pub fn is_data_path(path: &str) -> bool {
    path == "/local" || path.starts_with("/local/") || path.starts_with("/api/") || path == "/api"
}

/// `?token=` 查询参数凭据只在这两条路径族上开放。
///
/// 查询参数会进访问日志、浏览器历史与 `Referer`，所以不放给全站：
/// OPDS 阅读器与取页链接是唯一真正需要它的场景（拿不到自定义请求头）。
fn token_query_allowed(path: &str) -> bool {
    if path.starts_with("/api/opds/") {
        return true;
    }
    // 取页：/api/v1/manga/{id}/chapter/{i}/page/{n}
    path.starts_with("/api/v1/manga/") && path.contains("/chapter/") && path.contains("/page/")
}

/// 请求的凭据解析结果。
struct Resolved {
    principal: Principal,
    /// 是否靠 cookie 通过——决定要不要做 CSRF 校验。
    via_cookie: bool,
}

fn resolve_principal(auth: &AuthContext, headers: &HeaderMap, path: &str, query: Option<&str>) -> Resolved {
    let authenticated = |via_cookie| Resolved { principal: Principal::Authenticated, via_cookie };

    let authorization = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok());
    let cookies = headers.get(header::COOKIE).and_then(|v| v.to_str().ok());
    // 一次请求里取一次时间，避免几条通道之间出现微妙的过期判定差异
    let at = now();

    if let Some(encoded) = authorization.and_then(|v| v.strip_prefix("Basic "))
        && basic_valid(auth, encoded)
    {
        return authenticated(false);
    }

    if let Some(token) = authorization.and_then(|v| v.strip_prefix("Bearer ")).map(str::trim)
        && auth.verify_token(token, "access", at)
    {
        return authenticated(false);
    }

    // WebUI 的 graphql-ws 把 token 原样放在 `Authorization` 里（没有 `Bearer ` 前缀）
    if let Some(token) = authorization.map(str::trim).filter(|v| !v.is_empty())
        && auth.verify_token(token, "access", at)
    {
        return authenticated(false);
    }

    if let Some(value) = cookie_value(cookies, SESSION_COOKIE)
        && auth.verify_session_cookie(value, at)
    {
        return authenticated(true);
    }

    if let Some(value) = cookie_value(cookies, TOKEN_COOKIE)
        && auth.verify_token(value, "access", at)
    {
        return authenticated(true);
    }

    if token_query_allowed(path)
        && let Some(token) = query.and_then(query_param_token)
        && auth.verify_token(&token, "access", at)
    {
        return authenticated(false);
    }

    Resolved { principal: Principal::Anonymous, via_cookie: false }
}

fn query_param_token(query: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "token").then(|| value.to_string())
    })
}

fn cookie_value<'a>(cookies: Option<&'a str>, name: &str) -> Option<&'a str> {
    cookies?.split(';').find_map(|pair| {
        let (key, value) = pair.trim().split_once('=')?;
        (key == name).then_some(value)
    })
}

fn basic_valid(auth: &AuthContext, encoded: &str) -> bool {
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
        return false;
    };
    let Ok(text) = String::from_utf8(decoded) else {
        return false;
    };
    let Some((user, pass)) = text.split_once(':') else {
        return false;
    };
    auth.verify_credentials(user, pass)
}

/// 相对路径是否安全：不含 `..`、不含盘符/NTFS 数据流的 `:`、不为空。
///
/// 与 [`safe_join`] 分开是因为归档内成员（`a.zip/page/1.png`）指向的不是真实
/// 文件，只能做成分检查，不能要求 `canonicalize` 成功。
pub fn is_safe_rel(rel: &str) -> bool {
    !rel.is_empty() && rel.split('/').all(|segment| segment != ".." && !segment.contains(':'))
}

/// 把请求路径解析到 `root` 之下的真实文件；任何越界输入返回 `None`。
///
/// 不能写成 `root.join(rel)`：Windows 上带盘符的绝对路径会**整体替换** base，
/// `..` 也完全不受限——这两点合起来就是「读任意文件」。
///
/// 注意它**要求文件真实存在**：SPA 深链接（`/library`）要用 [`safe_rel_path`]。
pub fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let joined = safe_rel_path(root, rel)?;
    let root = root.canonicalize().ok()?;
    let path = joined.canonicalize().ok()?;
    path.starts_with(&root).then_some(path)
}

/// 只做词法判定的安全拼接：拒绝 `..`、盘符 / NTFS 数据流的 `:`，也不允许拼出
/// `root` 之外。**不要求文件存在**。
///
/// 与 [`safe_join`] 分开是必须的：SPA 深链接（`/library`、`/settings/download`）
/// 指向的是构建产物里并不存在的路径，它们要落到 `index.html`；如果这里也要求
/// `canonicalize` 成功，每一次刷新都会变成 404。
pub fn safe_rel_path(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.replace('\\', "/");
    let rel = rel.trim_start_matches('/');
    if !is_safe_rel(rel) {
        return None;
    }
    let joined = root.join(rel);
    // 词法兜底：拼接结果必须还在 root 之下
    joined.starts_with(root).then_some(joined)
}

/// 静态托管用：先把请求路径**解码**，再做词法安全判定。
///
/// 必须"先解码后判定"。`http::Uri::path()` 给的是请求行原文（`%2f` 还是 `%2f`），
/// 直接拿去拼路径会把 `/..%2f..%2fetc%2fpasswd` 当成一个普通文件名，落进 SPA 兜底
/// 返回 200；而它明显是个穿越尝试，应当 404。解码一次之后再判，`%20` 这类正常转义
/// 的文件名也才能命中真实文件。
///
/// 注意只解码 `%XX`，不处理 `+`：在路径里 `+` 是字面量，不是空格。
pub fn safe_public_path(root: &Path, raw_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(raw_path);
    safe_rel_path(root, &decoded)
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 公开静态资源：`safe_join` 命中 webui 目录里的真实文件，且**不是 SPA 入口**。
///
/// `index.html` 必须走认证，否则匿名用户直接拿到应用外壳；反过来，只要构建
/// 产物落在 webui 目录里就自动公开，新增文件不会再造成白屏。
pub fn is_public_asset(webui_dir: &Path, path: &str, method: &Method) -> bool {
    if method == Method::OPTIONS {
        return true;
    }
    if webui_dir.as_os_str().is_empty() {
        return false;
    }
    let Some(file) = safe_join(webui_dir, path) else {
        return false;
    };
    // SPA 入口不算公开资源；顺带要求它是真实存在的文件，这样"豁免清单"是算出来的
    file.file_name().is_some_and(|name| name != "index.html") && file.is_file()
}

fn is_safe_method(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// CSRF 纵深防御：只作用于「靠 cookie 认证」的非安全方法。
///
/// Basic / Bearer 不带浏览器自动附带的凭据，跨站请求构造不出来，不受影响。
fn csrf_ok(headers: &HeaderMap) -> bool {
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok())
        && !matches!(site, "same-origin" | "none")
    {
        return false;
    }
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
            return false;
        };
        let origin_host = origin.split_once("://").map(|(_, rest)| rest).unwrap_or(origin);
        if !origin_host.eq_ignore_ascii_case(host) {
            return false;
        }
    }
    true
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, [(header::CONTENT_TYPE, "application/json")], r#"{"error":"unauthorized"}"#)
        .into_response()
}

fn challenge(auth: &AuthContext, path: &str, query: Option<&str>) -> Response {
    match auth.mode {
        AuthMode::BasicAuth => {
            (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Basic realm=\"Suwayomi\"")]).into_response()
        }
        _ => {
            let mut target = format!("/login.html?redirect={}", urlencode(&page_target(path, query)));
            if target.len() > 4096 {
                target = "/login.html".to_string();
            }
            Redirect::to(&target).into_response()
        }
    }
}

/// 登录后要回到的位置。带上前面的 query，但把 `token=` 摘掉——它不该进下一个 URL。
fn page_target(path: &str, query: Option<&str>) -> String {
    let Some(query) = query.filter(|q| !q.is_empty()) else {
        return path.to_string();
    };
    let kept: Vec<&str> = query.split('&').filter(|pair| !pair.starts_with("token=")).collect();
    if kept.is_empty() { path.to_string() } else { format!("{path}?{}", kept.join("&")) }
}

fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// 认证中间件。
pub async fn require_auth(State(state): State<AppState>, mut req: Request<Body>, next: Next) -> Response {
    let auth = state.auth.clone();
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    let query = req.uri().query().map(str::to_string);

    let Resolved { principal, via_cookie } = resolve_principal(&auth, req.headers(), &path, query.as_deref());
    req.extensions_mut().insert(principal);

    if auth.is_disabled() {
        return next.run(req).await;
    }
    if is_login_flow(&path) {
        return next.run(req).await;
    }
    if via_cookie && principal.is_authenticated() && !is_safe_method(&method) && !csrf_ok(req.headers()) {
        return (StatusCode::FORBIDDEN, "cross-site request rejected").into_response();
    }
    if is_public_asset(&state.webui_dir, &path, &method) {
        return next.run(req).await;
    }
    // GraphQL 按操作判定（见本模块文档），这里只负责把主体带过去
    if path == "/api/graphql" {
        return next.run(req).await;
    }
    // 关闭端点由 handler 自己判：它要放行**不带 Origin 的本机调用**（桌面托盘
    // 是裸 HTTP 客户端，给不出凭据），这条规则按路径判不出来
    if path == "/api/v1/shutdown" {
        return next.run(req).await;
    }
    if principal.is_authenticated() {
        return next.run(req).await;
    }
    if is_data_path(&path) {
        return unauthorized();
    }
    // UI_LOGIN 下页面必须匿名可达：登录页由 SPA 自己渲染，外壳加载不出来
    // 就永远看不到登录入口
    if auth.mode == AuthMode::UiLogin {
        return next.run(req).await;
    }
    challenge(&auth, &path, query.as_deref())
}

/// 扩展：从请求扩展取出主体（handler 需要更细的判定时用）。
pub fn principal_of(req: &Request<Body>) -> Principal {
    req.extensions().get::<Principal>().copied().unwrap_or(Principal::Anonymous)
}

// ---------------- 登录流程 ----------------

/// 只接受站内相对路径：`//host`、绝对 URL、带 `\` 或 `:` 的值一律拒绝，
/// 否则这里就是一个开放重定向。
pub fn safe_redirect(value: Option<&str>) -> String {
    let Some(value) = value.map(str::trim) else {
        return "/".to_string();
    };
    if !value.starts_with('/') || value.starts_with("//") || value.contains('\\') || value.contains(':') {
        return "/".to_string();
    }
    value.to_string()
}

fn html_document(title: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>
:root {{ color-scheme: light dark; }}
body {{ margin:0; min-height:100vh; display:flex; align-items:center; justify-content:center;
  font:14px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif; background:#f6f6f7; color:#1a1a1a; }}
form, main {{ width:min(320px,88vw); background:#fff; padding:24px; border-radius:12px;
  border:1px solid rgba(0,0,0,.12); }}
h1 {{ margin:0 0 16px; font-size:16px; font-weight:500; }}
label {{ display:block; margin-bottom:12px; font-size:13px; }}
input {{ width:100%; box-sizing:border-box; margin-top:4px; padding:8px 10px; font:inherit;
  border:1px solid rgba(0,0,0,.2); border-radius:8px; background:#fff; color:inherit; }}
button {{ width:100%; padding:9px; font:inherit; font-weight:500; color:#fff; background:#1a73e8;
  border:0; border-radius:8px; cursor:pointer; }}
.error {{ margin:0 0 12px; padding:8px 10px; border-radius:8px; background:#fcebeb; color:#a32d2d; font-size:13px; }}
@media (prefers-color-scheme: dark) {{
  body {{ background:#161617; color:#f1f1f1; }}
  form, main {{ background:#1f1f20; border-color:rgba(255,255,255,.14); }}
  input {{ background:#161617; border-color:rgba(255,255,255,.2); }}
  .error {{ background:#3a1d1d; color:#f09595; }}
}}
</style></head><body>{body}</body></html>"#
    )
}

fn html_response(status: StatusCode, html: String, set_cookie: Option<String>) -> Response {
    let mut response = (
        status,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        html,
    )
        .into_response();
    if let Some(cookie) = set_cookie
        && let Ok(value) = cookie.parse()
    {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

fn login_form(redirect: &str, error: &str) -> String {
    let error_html = if error.is_empty() {
        String::new()
    } else {
        format!("<p class=\"error\">{}</p>", escape_html(error))
    };
    html_document(
        "Sign in",
        &format!(
            r#"<form method="post" action="/login.html?redirect={redirect_attr}">
<h1>Suwayomi</h1>
{error_html}
<label>Username<input name="user" autocomplete="username" autofocus></label>
<label>Password<input name="pass" type="password" autocomplete="current-password"></label>
<button type="submit">Sign in</button>
</form>"#,
            redirect_attr = escape_html(&urlencode(redirect)),
        ),
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// `GET /login.html`
pub async fn login_page(uri: Uri) -> Response {
    let params = QueryParams::parse(uri.query());
    let redirect = safe_redirect(params.get("redirect"));
    html_response(StatusCode::OK, login_form(&redirect, params.get("error").unwrap_or("")), None)
}

/// `POST /login.html`
pub async fn login_submit(State(state): State<AppState>, uri: Uri, body: String) -> Response {
    let params = QueryParams::parse(uri.query());
    let redirect = safe_redirect(params.get("redirect"));
    let form = QueryParams::parse(Some(&body));
    let user = form.get("user").unwrap_or("").to_string();
    let password = form.get("pass").unwrap_or("").to_string();

    if state.auth.verify_credentials(&user, &password) {
        return (
            StatusCode::SEE_OTHER,
            [
                (header::SET_COOKIE, state.auth.session_set_cookie(now())),
                (header::LOCATION, redirect),
            ],
        )
            .into_response();
    }

    // 失败不写会话，并把已有会话作废
    html_response(
        StatusCode::OK,
        login_form(&redirect, "Invalid username or password"),
        Some(state.auth.session_clear_cookie()),
    )
}

/// `GET /logout`
pub async fn logout(State(state): State<AppState>) -> Response {
    (
        StatusCode::SEE_OTHER,
        [
            (header::SET_COOKIE, state.auth.session_clear_cookie()),
            (header::LOCATION, "/login.html".to_string()),
        ],
    )
        .into_response()
}

/// `application/x-www-form-urlencoded` 与查询串的极简解析（含 `+` 与 `%XX`）。
struct QueryParams(Vec<(String, String)>);

impl QueryParams {
    fn parse(raw: Option<&str>) -> Self {
        let Some(raw) = raw else {
            return Self(Vec::new());
        };
        Self(
            raw.split('&')
                .filter(|pair| !pair.is_empty())
                .map(|pair| {
                    let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                    (decode_form(key), decode_form(value))
                })
                .collect(),
        )
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

fn decode_form(value: &str) -> String {
    // 表单编码里 `+` 表示空格，路径里不是
    percent_decode(&value.replace('+', " "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_rejects_traversal_and_drive_letters() {
        let root = std::env::temp_dir();
        let root = root.canonicalize().unwrap();
        std::fs::write(root.join("ok.txt"), b"x").unwrap();
        assert!(safe_join(&root, "/ok.txt").is_some());
        assert!(safe_join(&root, "/../ok.txt").is_none());
        assert!(safe_join(&root, "/a/../../ok.txt").is_none());
        assert!(safe_join(&root, "/C:/Windows/win.ini").is_none());
        assert!(safe_join(&root, "/\\\\server\\share").is_none());
        assert!(safe_join(&root, "/%2e%2e/ok.txt").is_none());
    }

    /// 深链接必须能落到 index.html：`safe_join` 对不存在的路径返回 `None`，
    /// 而 `safe_rel_path` 只做词法判定，两者不能混用。
    #[test]
    fn deep_link_does_not_require_an_existing_file() {
        let root = std::env::temp_dir();
        let root = root.canonicalize().unwrap();
        assert!(safe_join(&root, "/no-such-spa-route").is_none());
        assert_eq!(
            safe_rel_path(&root, "/no-such-spa-route"),
            Some(root.join("no-such-spa-route"))
        );
        assert_eq!(safe_rel_path(&root, "/library"), Some(root.join("library")));
        // 越界仍然拒绝
        assert!(safe_rel_path(&root, "/../secret").is_none());
        assert!(safe_rel_path(&root, "/C:/Windows/win.ini").is_none());
        assert!(safe_rel_path(&root, "/x:secret").is_none());
        // UNC 写法会被"洗"成 root 下的普通相对路径，落在 root 内即安全
        let unc = safe_rel_path(&root, "\\\\server\\share").unwrap();
        assert!(unc.starts_with(&root), "{unc:?}");
    }

    #[test]
    fn redirect_must_stay_on_site() {
        assert_eq!(safe_redirect(Some("/library")), "/library");
        assert_eq!(safe_redirect(Some("/library?x=1")), "/library?x=1");
        assert_eq!(safe_redirect(Some("//evil.com")), "/");
        assert_eq!(safe_redirect(Some("https://evil.com")), "/");
        assert_eq!(safe_redirect(Some("/a\\b")), "/");
        assert_eq!(safe_redirect(None), "/");
    }

    #[test]
    fn token_query_is_limited_to_opds_and_pages() {
        assert!(token_query_allowed("/api/opds/v1.2/library/series"));
        assert!(token_query_allowed("/api/v1/manga/1/chapter/2/page/3"));
        assert!(!token_query_allowed("/api/graphql"));
        assert!(!token_query_allowed("/api/v1/backup/export"));
    }

    /// 静态托管的路径判定：先解码再判，`%2f` 写法不能蒙混过关。
    #[test]
    fn public_path_is_decoded_before_the_safety_check() {
        let root = std::env::temp_dir();
        let root = root.canonicalize().unwrap();
        // SPA 深链接（不存在）仍要能落到 index.html
        assert_eq!(safe_public_path(&root, "/library"), Some(root.join("library")));
        // 编码过的穿越必须被识破
        assert!(safe_public_path(&root, "/..%2f..%2fetc%2fpasswd").is_none());
        assert!(safe_public_path(&root, "/%2e%2e/%2e%2e/secret").is_none());
        assert!(safe_public_path(&root, "/..%5csecret").is_none());
        assert!(safe_public_path(&root, "/C%3A/Windows/win.ini").is_none());
        // 正常转义能命中真实文件名
        let file = root.join("a b.txt");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(safe_public_path(&root, "/a%20b.txt"), Some(file));
    }

    #[test]
    fn form_decoding_handles_plus_and_percent() {
        let params = QueryParams::parse(Some("user=a+b&pass=p%40ss"));
        assert_eq!(params.get("user"), Some("a b"));
        assert_eq!(params.get("pass"), Some("p@ss"));
    }
}
