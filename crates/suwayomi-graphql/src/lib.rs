//! GraphQL API — mirrors `suwayomi.graphql.*` on async-graphql.
//! Compatibility target: `docs/graphql/schema-baseline.graphql` (359 types).

pub mod autobackup;
pub mod mutation;
pub mod mutation_b4;
pub mod query;
pub mod scalars;
pub mod schema;
pub mod settings;
pub mod state;
pub mod subscription;
pub mod track;
pub mod types;
pub mod updater;

pub use state::GraphQLState;
