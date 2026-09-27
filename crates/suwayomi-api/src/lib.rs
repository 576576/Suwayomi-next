//! 接口层的公共部分：应用状态与认证。
//!
//! 这两个模块原先长在 `suwayomi-rest` 里，但它们都不是 REST 专属的：
//!
//! * [`AppState`] 由 `suwayomi-server` 装配一次、交给每个协议层当 axum 的 state
//!   类型；`suwayomi-opds` 甚至只为取这个类型而依赖过 `suwayomi-rest`。
//! * [`auth`] 是**全站**中间件——连 WebUI 静态托管、`/api/v1/shutdown` 的门禁都
//!   走它，只有 GraphQL 因为要按 operation 判定而自己再做一层。
//!
//! 把平台级的东西放在某个协议实现里，依赖方向就会倒过来（OPDS → REST 就是这么
//! 产生的）。抽到这一层之后，三个协议 crate 平级，都只依赖 `suwayomi-domain`。
//!
//! **错误映射不在这里**：REST 的 `{"message": …}` + 状态码与 GraphQL 的 errors
//! 数组是各自协议的契约形状（WebUI 还靠 GraphQL 侧那句 `UnauthorizedException`
//! 判断该刷新 token），共享只会让两边互相牵制。

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

pub mod auth;
pub mod state;

pub use state::AppState;
