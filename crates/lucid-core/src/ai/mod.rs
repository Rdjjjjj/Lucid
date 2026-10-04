//! AI 协议适配。OpenAI 与 Anthropic 的差异停在这里，句末检测和替换不认识服务商。

mod client;
mod config;
mod error;
mod models;
mod parser;

pub use client::HttpCorrectionService;
pub use config::{AiConfiguration, AiProtocol};
pub use error::AiClientError;
pub use models::{CorrectionRequest, CorrectionResult};
pub use parser::ResponseParser;

pub trait CorrectionService {
    fn correct(
        &self,
        request: &CorrectionRequest,
    ) -> impl Future<Output = Result<CorrectionResult, AiClientError>> + Send;
}
