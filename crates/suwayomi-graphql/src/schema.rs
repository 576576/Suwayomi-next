//! Schema construction + axum handlers — mirrors
//! `graphql/server/GraphQLServer.kt` + `GraphQLController.kt`.
//!
//! 模块级放行 `useless_let_if_seq`：`async-graphql-derive` 的 `MergedObject` 展开里
//! 有一处 clippy 不认的 `let mut`（见下方 `RootMutation`），属于上游宏实现，用
//! item 上的 `#[allow]` 盖不住。

#![allow(clippy::useless_let_if_seq)]

use std::collections::BTreeMap;
use std::sync::Arc;

use async_graphql::http::ALL_WEBSOCKET_PROTOCOLS;
use async_graphql::{Data, MergedObject, Schema};
use async_graphql_axum::{GraphQL, GraphQLProtocol, GraphQLWebSocket};
use axum::Router;
use axum::body::Body;
use axum::extract::{FromRequestParts, Request, State, ws::WebSocketUpgrade};
use axum::http::{Method, StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use suwayomi_core::auth::{AuthContext, Principal, now};

use crate::mutation::MutationRoot;
use crate::mutation_b4::MutationRootB4;
use crate::query::QueryRoot;
use crate::state::GraphQLState;
use crate::subscription::SubscriptionRoot;

// `async-graphql-derive` 的 `MergedObject` 展开里有一处 clippy 不认的 `let mut`，
// 属于上游宏实现，与本 crate 无关。
#[derive(MergedObject, Default)]
#[graphql(name = "Mutation")]
pub struct RootMutation(pub MutationRoot, pub MutationRootB4);

pub type GraphQLSchema = Schema<QueryRoot, RootMutation, SubscriptionRoot>;

/// Builds the schema with runtime state injected (accessible via `ctx.data`).
pub fn build_schema(state: GraphQLState) -> GraphQLSchema {
    Schema::build(QueryRoot, RootMutation::default(), SubscriptionRoot).data(state).finish()
}

/// 唯二允许匿名调用的根字段——没有它们谁也登不进来。
const PUBLIC_ROOT_FIELDS: [&str; 2] = ["login", "refreshToken"];

/// HTTP 请求体的检查上限。GraphQL query 远小于此；超过说明不是正常客户端。
const MAX_INSPECT_BYTES: usize = 512 * 1024;

#[derive(Clone)]
struct RouteState {
    schema: GraphQLSchema,
    auth: Arc<AuthContext>,
}

/// Mirrors `GraphQL.defineEndpoints()`: POST/GET `/graphql` under `/api`.
///
/// 两层：按操作判定的授权层在最外，WebSocket 升级层在内。`login` /
/// `refreshToken` 必须匿名可达，而这是纯粹按路径判不出来的——所以授权这一层
/// 只有放在这里。
pub fn graphql_router<S: Clone + Send + Sync + 'static>(schema: GraphQLSchema, auth: Arc<AuthContext>) -> Router<S> {
    let state = RouteState { schema, auth };
    Router::<S>::new()
        .route_service("/graphql", GraphQL::new(state.schema.clone()))
        .route_layer(from_fn_with_state(state.clone(), graphql_ws_middleware))
        .route_layer(from_fn_with_state(state, graphql_auth_middleware))
}

/// 未认证时返回的形状：HTTP 401 + GraphQL 错误体。
///
/// 错误文案里带上 `UnauthorizedException` 是**兼容要求**——WebUI 的
/// `isAuthError` 就是靠这个名字识别「该刷新 token 了」的（见
/// `GraphQLClient.ts`），换掉它前端的登录跳转就不触发。
fn unauthorized_graphql() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"errors":[{"message":"suwayomi.tachidesk.server.user.UnauthorizedException: Unauthorized"}]}"#,
    )
        .into_response()
}

fn is_ws_upgrade(headers: &header::HeaderMap) -> bool {
    headers
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
}

/// `/api/graphql` 的按操作授权。
///
/// 主体由外层的认证中间件解析并注入；这一层只回答一个问题：**这次请求的操作
/// 是不是「匿名也可调用」的那两个**。判定不了的一律拒绝。
async fn graphql_auth_middleware(State(state): State<RouteState>, req: Request, next: Next) -> Response {
    if state.auth.is_disabled() {
        return next.run(req).await;
    }
    // WS 的凭据在 connection_init 里（浏览器无法给 WebSocket 加自定义头），
    // 由下面的 ws 层负责
    if is_ws_upgrade(req.headers()) {
        return next.run(req).await;
    }
    if principal_of(&req).is_authenticated() {
        return next.run(req).await;
    }

    let (parts, body) = req.into_parts();
    if parts.method == Method::GET || parts.method == Method::HEAD {
        if parts.uri.query().is_some_and(|q| {
            query_param(q, "query")
                .is_some_and(|query| operation_is_public(&query, query_param(q, "operationName").as_deref()))
        }) {
            return next.run(Request::from_parts(parts, body)).await;
        }
        return unauthorized_graphql();
    }

    let is_json = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    if !is_json {
        // multipart 上传、表单等一律需要凭据
        return unauthorized_graphql();
    }

    let Ok(bytes) = axum::body::to_bytes(body, MAX_INSPECT_BYTES).await else {
        return unauthorized_graphql();
    };
    if json_request_is_public(&bytes) {
        return next.run(Request::from_parts(parts, Body::from(bytes))).await;
    }
    unauthorized_graphql()
}

fn principal_of(req: &Request) -> Principal {
    req.extensions().get::<Principal>().copied().unwrap_or(Principal::Anonymous)
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some((&b, tail)) = rest.split_first() {
        // `%XX` only when two hex digits actually follow; `tail.get(..2)` is `None`
        // otherwise, which is exactly the old `i + 2 < bytes.len()` guard.
        let hex = if b == b'%' { tail.get(..2) } else { None };
        if let Some(byte) = hex.and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
            out.push(byte);
            rest = tail.get(2..).unwrap_or_default();
        } else {
            out.push(if b == b'+' { b' ' } else { b });
            rest = tail;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 单个 JSON 请求（或 batch）是否全部只由公开操作组成。
fn json_request_is_public(bytes: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return false;
    };
    let requests: Vec<&serde_json::Value> = match &value {
        serde_json::Value::Array(items) => items.iter().collect(),
        other => vec![other],
    };
    !requests.is_empty()
        && requests.iter().all(|request| {
            let query = request.get("query").and_then(|v| v.as_str());
            let operation_name = request.get("operationName").and_then(|v| v.as_str());
            query.is_some_and(|query| operation_is_public(query, operation_name))
        })
}

/// 解析文档，判断这次执行的操作是否只选了公开根字段。
///
/// 任何解析不出来的情况（根级 fragment、多操作且未指定 `operationName`）都返回
/// `false`——判定不了就拒绝。
fn operation_is_public(query: &str, operation_name: Option<&str>) -> bool {
    let Ok(document) = async_graphql::parser::parse_query(query) else {
        return false;
    };
    let mut candidates = document.operations.iter().filter(|(name, _)| match (operation_name, name) {
        (Some(wanted), Some(name)) => name.as_str() == wanted,
        (Some(_), None) => false,
        (None, _) => true,
    });
    let Some((_, operation)) = candidates.next() else {
        return false;
    };
    // 多操作又没说执行哪个：由服务端按 `operationName` 之外的方式决定，不猜
    if operation_name.is_none() && candidates.next().is_some() {
        return false;
    }
    root_fields_are_public(&operation.node)
}

fn root_fields_are_public(operation: &async_graphql::parser::types::OperationDefinition) -> bool {
    let mut count = 0usize;
    for selection in &operation.selection_set.node.items {
        // 根级 fragment spread / inline fragment 无法静态判定，拒绝
        let async_graphql::parser::types::Selection::Field(field) = &selection.node else {
            return false;
        };
        count += 1;
        if !PUBLIC_ROOT_FIELDS.contains(&field.node.name.node.as_str()) {
            return false;
        }
    }
    count > 0
}

/// 把 `/graphql` 上的 WebSocket 升级请求交给 `async-graphql` 的订阅传输。
///
/// `async-graphql-axum` 把两种传输拆开了：`GraphQL` 只认 HTTP，订阅要用
/// `GraphQLWebSocket`（子协议 `graphql-transport-ws` / `graphql-ws`）。两者必须在
/// 同一个路径上共存（WebUI 的 `graphql-ws` 客户端连的就是 `ws://<host>/api/graphql`），
/// 所以按请求头分流：带 `Upgrade: websocket` 才走订阅，其余原样放行。
///
/// 没有这一层时 schema 里的 `Subscription` 永远收不到数据 —— HTTP 服务会拿普通
/// 响应回掉握手（客户端看到 `Unexpected response code: 200`）并无限重连，
/// 下载进度 / 更新状态 / 同步状态的实时推送全是死的。
///
/// 认证在 `connection_init` 里做：浏览器没法给 WebSocket 加自定义请求头，所以
/// WebUI 把 token 放在 `connection_init` 的 payload 里（键名 `Authorization`，
/// 可能带也可能不带 `Bearer ` 前缀）；握手请求本身带 cookie 的情况下也认。
async fn graphql_ws_middleware(State(state): State<RouteState>, request: Request, next: Next) -> Response {
    if !is_ws_upgrade(request.headers()) {
        return next.run(request).await;
    }
    // 握手请求本身带的凭据（cookie / Authorization）由外层认证中间件注入
    let handshake_authenticated = principal_of(&request).is_authenticated();
    // WS 握手没有请求体
    let (mut parts, _body) = request.into_parts();
    let auth = state.auth.clone();

    let upgrade = match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(upgrade) => upgrade,
        Err(rejection) => return rejection.into_response(),
    };
    // 客户端没带可识别的子协议时这里会给出 400，比原样回 200 更好定位。
    let protocol = match GraphQLProtocol::from_request_parts(&mut parts, &()).await {
        Ok(protocol) => protocol,
        Err(status) => return status.into_response(),
    };
    let schema = state.schema.clone();

    upgrade
        .protocols(ALL_WEBSOCKET_PROTOCOLS)
        .on_upgrade(move |stream| {
            GraphQLWebSocket::new(stream, schema, protocol)
                .on_connection_init(move |payload: serde_json::Value| async move {
                    if auth.is_disabled() || handshake_authenticated {
                        return Ok(Data::default());
                    }
                    let token = payload
                        .get("Authorization")
                        .and_then(|value| value.as_str())
                        .map(|value| value.strip_prefix("Bearer ").unwrap_or(value).trim());
                    match token {
                        Some(token) if auth.verify_token(token, "access", now()) => Ok(Data::default()),
                        _ => Err(async_graphql::Error::new(
                            "suwayomi.tachidesk.server.user.UnauthorizedException: Unauthorized",
                        )),
                    }
                })
                .serve()
        })
        .into_response()
}

/// Schema SDL without runtime state (for compatibility checks).
pub fn schema_sdl() -> String {
    let schema = Schema::build(QueryRoot, RootMutation::default(), SubscriptionRoot).finish();
    schema.sdl()
}

/// SDL 顶层类型定义的六种前缀（GraphQL 规范里 `union` 也是类型）。
///
/// `directive` / `schema` / `extend` 不算：基线文件里正好有 3 个 `directive`、0 个 `extend`，
/// 把 `directive` 计进来会让总数凭空多 3，与 `docs/graphql/README.md` 的 359 对不上。
const TYPE_KINDS: [&str; 6] = ["type", "input", "enum", "scalar", "interface", "union"];

/// 把 SDL 拆成「顶层定义名 → 种类」。
///
/// 只认**行首**前缀（字段/枚举值都带缩进，不会被误收），首段再按分隔符切出名字：
/// `type Foo implements Bar {` → `Foo`、`union Filter = A | B` → `Filter`、
/// `scalar LongString` → `LongString`。
fn top_level_defs(sdl: &str) -> BTreeMap<String, &'static str> {
    sdl.lines()
        .filter_map(|line| {
            TYPE_KINDS.iter().copied().find_map(|kind| {
                let rest = line.strip_prefix(kind)?.strip_prefix(' ')?;
                let name = rest.split([' ', '{', '(', ':', '=']).next().unwrap_or_default();
                (!name.is_empty()).then_some((name.to_owned(), kind))
            })
        })
        .collect()
}

/// 顶层类型定义数，与 `docs/graphql/README.md` 的「359 个类型定义」**同口径**
/// （含 `union`、不含 `directive`）。
///
/// 这个数字同时进启动日志（`suwayomi-server/src/lib.rs`）。口径能否对齐基线由
/// `tests::schema_matches_baseline` 断言锁住 —— 改了统计方式而不更新差分，测试会红。
pub fn schema_type_count() -> usize {
    top_level_defs(&schema_sdl()).len()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// Kotlin 版 introspection 导出的基线 SDL。`include_str!` 在**编译期**把文件内容
    /// 嵌进测试二进制 → 断言不依赖运行时工作目录，也不会因为 `cargo test` 的 cwd 不同
    /// 而静默跳过。代价是路径必须相对 crate 目录固定（本仓库不 `cargo publish`，可接受）。
    const BASELINE_SDL: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/graphql/schema-baseline.graphql"));

    /// `docs/graphql/README.md` 的对外口径：359 个类型定义 / 3033 行 SDL。
    const BASELINE_TOTAL: usize = 359;

    /// 基线按种类的构成 —— 359 = 167 + 145 + 34 + 4 + 6 + 3。
    ///
    /// 分开断言是为了让失败信息能指出**哪一类**漂了，而不是只报一个总数。
    const BASELINE_BY_KIND: [(&str, usize); 6] =
        [("type", 167), ("input", 145), ("enum", 34), ("scalar", 4), ("interface", 6), ("union", 3)];

    /// 基线有、本 fork 尚未实现的 17 个类型。
    ///
    /// 集中在四块：① 分页的 `Node`/`Edge` 家族 —— 基线里 `Node`/`Edge`/`NodeList`/`MetaType`
    /// 四个 `interface` 加各自的实现类型，本 fork 侧**一个 `interface` 都没注册**；
    /// ② `Settings` 家族；③ 5 个 `WebUIUpdate*`；④ `union Node`（本 fork 只注册了
    /// `Filter` 与 `Preference` 两个 union，第三个在基线里，这里少了）。
    const KNOWN_MISSING: [&str; 17] = [
        "DownloadEdge",
        "DownloadNodeList",
        "DownloadUpdate",
        "DownloadUpdateType",
        "Edge",
        "MetaType",
        "Node",
        "NodeList",
        "PartialSettingsType",
        "Settings",
        "SettingsDownloadConversion",
        "SettingsDownloadConversionHeader",
        "UpdateState",
        "WebUIUpdateInfo",
        "WebUIUpdateInput",
        "WebUIUpdatePayload",
        "WebUIUpdateStatus",
    ];

    /// 本 fork 自有、基线里没有的 8 个类型（分页索引重建、tracker OAuth、章节重排）。
    const KNOWN_EXTRA: [&str; 8] = [
        "RebuildDownloadIndexInput",
        "RebuildDownloadIndexPayload",
        "RefreshTrackerUserInput",
        "RefreshTrackerUserPayload",
        "ReorderChapterDownloadsPayload",
        "TrackerOAuthAppType",
        "UpdateTrackerOAuthAppInput",
        "UpdateTrackerOAuthAppPayload",
    ];

    fn names(defs: &BTreeMap<String, &'static str>) -> BTreeSet<String> {
        defs.keys().cloned().collect()
    }

    /// 基线文件自身没被改坏。否则下面的差分断言会给出误导性的结果 ——
    /// 比如基线被截断时，"缺 16 个"会一路涨上去，看不出是文件的问题。
    #[test]
    fn baseline_file_matches_documented_total() {
        let defs = top_level_defs(BASELINE_SDL);
        assert_eq!(defs.len(), BASELINE_TOTAL, "基线 SDL 的顶层类型定义总数");
        for (kind, expected) in BASELINE_BY_KIND {
            let actual = defs.values().filter(|k| **k == kind).count();
            assert_eq!(actual, expected, "基线里 {kind} 的个数");
        }
    }

    /// 契约断言：本 crate 产出的 schema 与基线的差异必须**恰好**是已知的那些。
    ///
    /// 这条把 `docs/graphql/README.md` 的 359 从「文档里的说法」变成「测试钉住的契约」：
    /// - 实现了 `KNOWN_MISSING` 里的类型 → 从数组里删掉它，并同步 README 的现状描述；
    /// - 新增/删除了本 fork 的类型 → 更新 `KNOWN_EXTRA`。
    ///
    /// 断言的是**集合全量**而不是单个数字：失败时直接看到多了/少了哪几个名字，
    /// 不用再写一遍探针脚本去比。
    #[test]
    fn schema_matches_baseline() {
        let rust = top_level_defs(&schema_sdl());
        let baseline = top_level_defs(BASELINE_SDL);

        let rust_names = names(&rust);
        let baseline_names = names(&baseline);

        let missing: Vec<&str> = baseline_names.difference(&rust_names).map(String::as_str).collect();
        let extra: Vec<&str> = rust_names.difference(&baseline_names).map(String::as_str).collect();

        assert_eq!(missing, KNOWN_MISSING, "基线有、本 fork 未实现的类型");
        assert_eq!(extra, KNOWN_EXTRA, "本 fork 自有、基线没有的类型");

        // 总数关系由上面两个数组推出，不写死数字：359 − 17 + 8 = 350。
        assert_eq!(
            rust.len(),
            BASELINE_TOTAL - KNOWN_MISSING.len() + KNOWN_EXTRA.len(),
            "本 fork 的类型总数应等于「基线 − 未实现 + 自有」"
        );
        // 进启动日志的那个数字与这里解析出的结果必须同口径。
        assert_eq!(schema_type_count(), rust.len(), "schema_type_count() 应与解析结果同口径");
    }
}
