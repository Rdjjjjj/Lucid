//! 把整理请求发给用户配置的中转站。请求不经过 Lucid 的服务器。

use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Value, json};

use crate::prompt::CLEANUP_INSTRUCTION;

use super::{
    AiClientError, AiConfiguration, AiProtocol, CorrectionRequest, CorrectionResult, ResponseParser,
};

pub struct HttpCorrectionService {
    configuration: AiConfiguration,
    api_key: String,
    client: reqwest::Client,
}

impl HttpCorrectionService {
    pub fn new(
        configuration: AiConfiguration,
        api_key: impl Into<String>,
    ) -> Result<Self, AiClientError> {
        configuration.validate(false)?;
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(AiClientError::MissingApiKey);
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(configuration.request_timeout_seconds))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| AiClientError::TransportFailure("无法创建 AI 请求。".to_owned()))?;
        Ok(Self {
            configuration,
            api_key,
            client,
        })
    }

    pub async fn correct(
        &self,
        request: &CorrectionRequest,
    ) -> Result<CorrectionResult, AiClientError> {
        self.configuration.validate(true)?;
        let response = self
            .client
            .post(self.endpoint(self.completion_suffix())?)
            .headers(self.headers()?)
            .json(&self.request_body(&request.sentence))
            .send()
            .await
            .map_err(|_| {
                AiClientError::TransportFailure(
                    "无法连接 AI 服务，请检查网络和 AI 服务设置后重试。".to_owned(),
                )
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(AiClientError::ServiceFailure(status.as_u16()));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| AiClientError::InvalidResponse)?;
        ResponseParser::sentence(&bytes)
            .map(CorrectionResult::new)
            .ok_or(AiClientError::InvalidResponse)
    }

    pub async fn fetch_models(&self) -> Result<Vec<String>, AiClientError> {
        self.configuration.validate(false)?;
        let response = self
            .client
            .get(self.endpoint("models")?)
            .headers(self.headers()?)
            .send()
            .await
            .map_err(|_| {
                AiClientError::TransportFailure(
                    "无法获取模型列表，请检查网络和 AI 服务设置后重试。".to_owned(),
                )
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(AiClientError::ServiceFailure(status.as_u16()));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| AiClientError::InvalidResponse)?;
        ResponseParser::model_ids(&bytes).ok_or(AiClientError::InvalidResponse)
    }

    fn completion_suffix(&self) -> &'static str {
        match self.configuration.api_protocol {
            AiProtocol::OpenAiCompatible => "chat/completions",
            AiProtocol::AnthropicCompatible => "messages",
        }
    }

    fn endpoint(&self, suffix: &str) -> Result<String, AiClientError> {
        let base = self.configuration.base_url.trim().trim_end_matches('/');
        let path = base.split("://").nth(1).unwrap_or(base);
        let path = path.split(['?', '#']).next().unwrap_or(path);
        let path = path.split_once('/').map(|(_, path)| path).unwrap_or("");
        let has_version = path == "v1" || path.ends_with("/v1");
        let suffix = if has_version {
            suffix.to_owned()
        } else {
            format!("v1/{suffix}")
        };
        Ok(format!("{base}/{suffix}"))
    }

    fn headers(&self) -> Result<HeaderMap, AiClientError> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        match self.configuration.api_protocol {
            AiProtocol::OpenAiCompatible => {
                let value = HeaderValue::from_str(&format!("Bearer {}", self.api_key.trim()))
                    .map_err(|_| {
                        AiClientError::InvalidConfiguration(
                            "API Key 含有无法放入请求头的字符。".to_owned(),
                        )
                    })?;
                headers.insert(AUTHORIZATION, value);
            }
            AiProtocol::AnthropicCompatible => {
                let value = HeaderValue::from_str(self.api_key.trim()).map_err(|_| {
                    AiClientError::InvalidConfiguration(
                        "API Key 含有无法放入请求头的字符。".to_owned(),
                    )
                })?;
                headers.insert("x-api-key", value);
                headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
            }
        }
        Ok(headers)
    }

    fn request_body(&self, sentence: &str) -> Value {
        match self.configuration.api_protocol {
            AiProtocol::OpenAiCompatible => json!({
                "model": self.configuration.model,
                "temperature": 0,
                "max_tokens": 1024,
                "thinking": {"type": "disabled"},
                "enable_thinking": false,
                "reasoning": {"effort": "none"},
                "messages": [
                    {"role": "system", "content": CLEANUP_INSTRUCTION},
                    {"role": "user", "content": sentence}
                ]
            }),
            AiProtocol::AnthropicCompatible => json!({
                "model": self.configuration.model,
                "max_tokens": 1024,
                "temperature": 0,
                "system": CLEANUP_INSTRUCTION,
                "messages": [{"role": "user", "content": sentence}]
            }),
        }
    }
}
