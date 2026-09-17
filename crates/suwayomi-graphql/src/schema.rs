//! Schema construction + axum handlers — mirrors
//! `graphql/server/GraphQLServer.kt` + `GraphQLController.kt`.

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
pub fn graphql_router<S: Clone + Send + Sync + 'static>(
    schema: GraphQLSchema,
    auth: Arc<AuthContext>,
) -> Router<S> {
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
async fn graphql_auth_middleware(
    State(state): State<RouteState>,
    req: Request,
    next: Next,
) -> Response {
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
        if parts.uri.query().is_some_and(|q| query_param(q, "query").is_some_and(|query| operation_is_public(&query, query_param(q, "operationName").as_deref()))) {
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
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
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

/// Quick compatibility probe: number of schema type definitions.
pub fn schema_type_count() -> usize {
    schema_sdl()
        .lines()
        .filter(|l| {
            l.starts_with("type ")
                || l.starts_with("enum ")
                || l.starts_with("input ")
                || l.starts_with("scalar ")
                || l.starts_with("interface ")
        })
        .count()
}
