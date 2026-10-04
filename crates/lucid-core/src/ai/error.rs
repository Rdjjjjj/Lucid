//! AI 请求失败时给界面的说明。原文必须保留，错误不能触发替换。

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AiClientError {
    #[error("{0}")]
    InvalidConfiguration(String),
    #[error("请先在设置中配置 API Key。")]
    MissingApiKey,
    #[error("AI 服务返回了无法识别的结果。")]
    InvalidResponse,
    #[error("AI 服务请求失败（HTTP {0}）。")]
    ServiceFailure(u16),
    #[error("{0}")]
    TransportFailure(String),
}
