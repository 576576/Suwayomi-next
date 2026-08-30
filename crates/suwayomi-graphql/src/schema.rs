//! Schema construction + axum handlers — mirrors
//! `graphql/server/TachideskGraphQLServer.kt` + `GraphQLController.kt`.

use async_graphql::{EmptySubscription, Schema};
use async_graphql_axum::GraphQL;
use axum::Router;

use crate::mutation::MutationRoot;
use crate::query::QueryRoot;
use crate::state::GraphQLState;

pub type GraphQLSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

/// Builds the schema with runtime state injected (accessible via `ctx.data`).
pub fn build_schema(state: GraphQLState) -> GraphQLSchema {
    Schema::build(QueryRoot, MutationRoot, EmptySubscription).data(state).finish()
}

/// Mirrors `GraphQL.defineEndpoints()`: POST/GET `/graphql` under `/api`.
/// State flows through the schema itself, so the router only needs a state
/// type parameter to nest into the app router.
pub fn graphql_router<S: Clone + Send + Sync + 'static>(schema: GraphQLSchema) -> Router<S> {
    Router::<S>::new().route_service("/graphql", GraphQL::new(schema))
}

/// Schema SDL without runtime state (for compatibility checks).
pub fn schema_sdl() -> String {
    let schema = Schema::build(QueryRoot, MutationRoot, EmptySubscription).finish();
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
