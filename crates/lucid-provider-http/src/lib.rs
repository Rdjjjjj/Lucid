//! Lucid 的 HTTP AI Provider。
//!
//! 这里负责网络请求、OpenAI/Anthropic 兼容协议和响应解析；核心层只处理
//! 句子、建议和替换，不依赖 HTTP 客户端或具体服务商。

mod client;
mod parser;

pub use client::HttpCorrectionService;
pub use parser::ResponseParser;
