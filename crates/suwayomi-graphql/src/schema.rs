//! Schema construction + axum handlers — mirrors
//! `graphql/server/GraphQLServer.kt` + `GraphQLController.kt`.

use async_graphql::http::ALL_WEBSOCKET_PROTOCOLS;
use async_graphql::{MergedObject, Schema};
use async_graphql_axum::{GraphQL, GraphQLProtocol, GraphQLWebSocket};
use axum::Router;
use axum::extract::{FromRequestParts, Request, State, ws::WebSocketUpgrade};
use axum::http::header;
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};

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

/// Mirrors `GraphQL.defineEndpoints()`: POST/GET `/graphql` under `/api`.
/// State flows through the schema itself, so the router only needs a state
/// type parameter to nest into the app router.
///
/// WebSocket 升级由 [`graphql_ws_middleware`] 先行接管，其余请求原样交给
/// `async-graphql-axum` 的 HTTP 服务（POST 查询、GET playground 都保持原状）。
pub fn graphql_router<S: Clone + Send + Sync + 'static>(schema: GraphQLSchema) -> Router<S> {
    Router::<S>::new()
        .route_service("/graphql", GraphQL::new(schema.clone()))
        .route_layer(from_fn_with_state(schema, graphql_ws_middleware))
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
/// 这里用到的 `axum::extract::ws` 依赖 `ws` 特性，由 `async-graphql-axum` 对
/// `axum` 的依赖顺带打开（见 `cargo tree -e features`）。
async fn graphql_ws_middleware(
    State(schema): State<GraphQLSchema>,
    request: Request,
    next: Next,
) -> Response {
    let (mut parts, body) = request.into_parts();

    let is_upgrade = parts
        .headers
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    if !is_upgrade {
        return next.run(Request::from_parts(parts, body)).await;
    }

    let upgrade = match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(upgrade) => upgrade,
        Err(rejection) => return rejection.into_response(),
    };
    // 客户端没带可识别的子协议时这里会给出 400，比原样回 200 更好定位。
    let protocol = match GraphQLProtocol::from_request_parts(&mut parts, &()).await {
        Ok(protocol) => protocol,
        Err(status) => return status.into_response(),
    };

    upgrade
        .protocols(ALL_WEBSOCKET_PROTOCOLS)
        .on_upgrade(move |stream| GraphQLWebSocket::new(stream, schema, protocol).serve())
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
