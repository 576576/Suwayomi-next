//! Domain-layer error type.

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("database error: {0}")]
    Db(#[from] suwayomi_db::Error),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    /// 取源内容失败。消息会被原样送到界面上（GraphQL 直接把 `Display` 当错误文案），
    /// 所以不加 `source error:` 这类前缀 —— 用户看到的第一句就该是能照着办的事。
    #[error("{0}")]
    Source(String),
    #[error("sandbox http error: {0}")]
    Sandbox(String),
    /// 追踪器（站点 API）失败。消息原样送到界面，所以带站点名和站点返回的原文。
    #[error("{0}")]
    Tracker(String),
    /// `TrackerManager` 里没有这个 id。
    #[error("tracker {0} not found")]
    TrackerNotFound(i32),
    /// 对应上游 `TokenExpired`：要重新登录。
    #[error("{0}: 登录已过期，需要重新登录")]
    TokenExpired(String),
}

impl From<reqwest::Error> for DomainError {
    fn from(e: reqwest::Error) -> Self {
        Self::Sandbox(e.to_string())
    }
}

impl DomainError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }

    pub fn tracker(msg: impl Into<String>) -> Self {
        Self::Tracker(msg.into())
    }

    pub fn token_expired(tracker: impl Into<String>) -> Self {
        Self::TokenExpired(tracker.into())
    }
}

pub type Result<T> = std::result::Result<T, DomainError>;
