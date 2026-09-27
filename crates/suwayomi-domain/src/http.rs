//! 共享的 HTTP 客户端构造。

use reqwest::Client;

/// 收尾 `ClientBuilder`，失败时退回默认客户端并告警。
///
/// `ClientBuilder::build()` 只在底层 TLS 后端初始化失败时返回 `Err`（不是网络错误），
/// 而 `Client::new()` 走同一套后端但有兜底。以前这里一律写 `.expect("reqwest client")`：
/// 一旦 TLS 后端起不来，整个服务（含不联网的那部分）直接起不来。改成降级后，
/// 只是"发不出去请求"，其余功能照常可用，日志里也留得下原因。
pub(crate) fn build_client(builder: reqwest::ClientBuilder) -> Client {
    match builder.build() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("reqwest client build failed ({e}); falling back to the default client");
            Client::new()
        }
    }
}
