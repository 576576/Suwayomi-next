//! GraphQL API — mirrors `suwayomi.graphql.*` on async-graphql.
//! Compatibility target: `docs/graphql/schema-baseline.graphql` (359 types).
//! 本 fork 当前产出 350（少 17 个未实现、多 8 个自有），差分由
//! `schema::tests::schema_matches_baseline` 断言锁住，构成见 `docs/graphql/README.md`。

// 测试代码允许 panic：unwrap / expect / panic! 在断言里是常规写法，
// 逐个改成 `?` 传播只会让失败信息更难读。生产代码不受这条影响
// （`cfg_attr(test, ...)`）。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::todo,
        clippy::indexing_slicing
    )
)]

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

pub use state::GraphQLState;
