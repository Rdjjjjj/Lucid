//! 主 App、Server 和输入法共用的设置与协议数据。
//!
//! 配置只描述 AI 服务，不保存句子正文。Windows 壳把它映射到当前用户注册表，
//! macOS 壳把它映射到自己的用户默认值存储。

use lucid_core::{AiConfiguration, AiProtocol};
use serde::{Deserialize, Serialize};

pub const CONFIGURATION_KEY: &str = lucid_core::config::CONFIGURATION_KEY;
pub const API_KEY_KEY: &str = lucid_core::config::API_KEY_KEY;
pub const DEFAULTS_SUITE: &str = "group.io.github.rdj.lucid";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedSettings {
    pub api_protocol: AiProtocol,
    pub base_url: String,
    pub model: String,
    pub request_timeout_seconds: u64,
    pub api_key: Option<String>,
}

impl SharedSettings {
    pub fn configuration(&self) -> AiConfiguration {
        let mut configuration =
            AiConfiguration::new(self.api_protocol, &self.base_url, &self.model);
        configuration.request_timeout_seconds = self.request_timeout_seconds;
        configuration
    }

    pub fn from_values(
        configuration_json: Option<&str>,
        api_key: Option<&str>,
    ) -> Result<Self, String> {
        let configuration = match configuration_json
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            Some(text) => decode_configuration(text)?,
            None => return Ok(Self::default_without_configuration(api_key)),
        };
        Ok(Self {
            api_protocol: configuration.api_protocol,
            base_url: configuration.base_url,
            model: configuration.model,
            request_timeout_seconds: configuration.request_timeout_seconds,
            api_key: clean_secret(api_key),
        })
    }

    pub fn configuration_json(configuration: &AiConfiguration) -> Result<String, String> {
        configuration
            .validate(false)
            .map_err(|error| error.to_string())?;
        let stored = StoredConfiguration {
            api_protocol: configuration.api_protocol,
            base_url: configuration.base_url.trim().to_owned(),
            model: configuration.model.trim().to_owned(),
            request_timeout_seconds: configuration.request_timeout_seconds,
        };
        serde_json::to_string(&stored).map_err(|error| error.to_string())
    }

    fn default_without_configuration(api_key: Option<&str>) -> Self {
        Self {
            api_protocol: AiProtocol::OpenAiCompatible,
            base_url: String::new(),
            model: String::new(),
            request_timeout_seconds: 30,
            api_key: clean_secret(api_key),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct StoredConfiguration {
    api_protocol: AiProtocol,
    base_url: String,
    model: String,
    #[serde(default = "default_timeout")]
    request_timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    30
}

fn clean_secret(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn decode_configuration(text: &str) -> Result<AiConfiguration, String> {
    let stored: StoredConfiguration =
        serde_json::from_str(text).map_err(|_| "AI 设置无法读取。".to_owned())?;
    let mut configuration =
        AiConfiguration::new(stored.api_protocol, stored.base_url, stored.model);
    configuration.request_timeout_seconds = stored.request_timeout_seconds;
    configuration
        .validate(false)
        .map_err(|error| error.to_string())?;
    Ok(configuration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_configuration_round_trips_without_sentences() {
        let mut configuration = AiConfiguration::new(
            AiProtocol::AnthropicCompatible,
            "https://api.example.com",
            "claude",
        );
        configuration.request_timeout_seconds = 20;
        let encoded = SharedSettings::configuration_json(&configuration).unwrap();
        let loaded = SharedSettings::from_values(Some(&encoded), Some("secret")).unwrap();
        assert_eq!(loaded.configuration(), configuration);
        assert_eq!(loaded.api_key.as_deref(), Some("secret"));
        assert!(!encoded.contains("sentence"));
    }
}
