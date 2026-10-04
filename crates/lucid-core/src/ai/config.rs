//! 用户填写的中转站配置。远程地址必须是 HTTPS，本机开发地址才允许 HTTP。

use serde::{Deserialize, Serialize};

use super::AiClientError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiProtocol {
    #[serde(rename = "open_ai_compatible", alias = "openAICompatible")]
    OpenAiCompatible,
    #[serde(rename = "anthropic_compatible", alias = "anthropicCompatible")]
    AnthropicCompatible,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiConfiguration {
    pub api_protocol: AiProtocol,
    pub base_url: String,
    pub model: String,
    pub request_timeout_seconds: u64,
}

impl AiConfiguration {
    pub fn new(
        api_protocol: AiProtocol,
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            api_protocol,
            base_url: base_url.into(),
            model: model.into(),
            request_timeout_seconds: 30,
        }
    }

    pub fn validate(&self, require_model: bool) -> Result<(), AiClientError> {
        let url = self.base_url.trim();
        let Some((scheme, rest)) = url.split_once("://") else {
            return Err(AiClientError::InvalidConfiguration(
                "请输入有效的 AI 服务地址。".to_owned(),
            ));
        };
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "https" && scheme != "http" {
            return Err(AiClientError::InvalidConfiguration(
                "AI 服务地址必须使用 HTTP 或 HTTPS。".to_owned(),
            ));
        }
        let host = rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .trim_matches(['[', ']'])
            .split(':')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if host.is_empty() {
            return Err(AiClientError::InvalidConfiguration(
                "AI 服务地址缺少主机名".to_owned(),
            ));
        }
        let local = host == "localhost" || host == "127.0.0.1" || host == "::1";
        if scheme == "http" && !local {
            return Err(AiClientError::InvalidConfiguration(
                "远程 AI 服务必须使用 HTTPS。".to_owned(),
            ));
        }
        if require_model && self.model.trim().is_empty() {
            return Err(AiClientError::InvalidConfiguration(
                "请选择一个模型".to_owned(),
            ));
        }
        if !(1..=120).contains(&self.request_timeout_seconds) {
            return Err(AiClientError::InvalidConfiguration(
                "请求超时时间必须在 1 到 120 秒之间".to_owned(),
            ));
        }
        Ok(())
    }
}
