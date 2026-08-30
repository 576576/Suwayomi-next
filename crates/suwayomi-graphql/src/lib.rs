//! GraphQL API — mirrors `suwayomi.tachidesk.graphql.*` on async-graphql.
//! Compatibility target: `docs/graphql/schema-baseline.graphql` (359 types).

pub mod query;
pub mod scalars;
pub mod schema;
pub mod settings;
pub mod state;
pub mod types;

pub use state::GraphQLState;
