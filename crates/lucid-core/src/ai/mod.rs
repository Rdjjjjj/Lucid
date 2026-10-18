//! AI 请求的共享模型。网络和具体服务商协议由独立 Provider crate 负责。

mod config;
mod error;
mod models;

pub use config::{AiConfiguration, AiProtocol};
pub use error::AiClientError;
pub use models::{CorrectionRequest, CorrectionResult};

pub trait CorrectionProvider {
    fn correct(
        &self,
        request: &CorrectionRequest,
    ) -> impl Future<Output = Result<CorrectionResult, AiClientError>> + Send;
}

// 兼容早期内部调用方，正式架构名称是 CorrectionProvider。
pub use CorrectionProvider as CorrectionService;
