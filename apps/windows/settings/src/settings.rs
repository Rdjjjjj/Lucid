//! Windows 设置存在当前用户注册表，不进凭据管理器，也不写输入历史。

use lucid_core::{AiConfiguration, AiProtocol};
use lucid_platform::{API_KEY_KEY, CONFIGURATION_KEY, DEFAULTS_SUITE};
use serde::{Deserialize, Serialize};

pub const SETTINGS_KEY_PATH: &str = r"Software\Lucid";

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsSettings {
    pub configuration: Option<AiConfiguration>,
    pub api_key: Option<String>,
}

impl WindowsSettings {
    pub fn configuration_key() -> &'static str {
        CONFIGURATION_KEY
    }

    pub fn api_key_key() -> &'static str {
        API_KEY_KEY
    }

    pub fn suite() -> &'static str {
        DEFAULTS_SUITE
    }

    pub fn from_values(
        configuration_json: Option<&str>,
        api_key: Option<&str>,
    ) -> Result<Self, String> {
        let configuration = match configuration_json
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            Some(text) => Some(decode_configuration(text)?),
            None => None,
        };
        let api_key = api_key
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned);
        Ok(Self {
            configuration,
            api_key,
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
    fn settings_use_the_shared_suite_and_reject_remote_http() {
        assert_eq!(WindowsSettings::suite(), "group.io.github.rdj.lucid");
        assert_eq!(WindowsSettings::configuration_key(), "ai.configuration.v1");
        assert_eq!(WindowsSettings::api_key_key(), "ai.apiKey.v1");
        let configuration = AiConfiguration::new(
            AiProtocol::OpenAiCompatible,
            "http://example.com/v1",
            "model",
        );
        assert!(WindowsSettings::configuration_json(&configuration).is_err());
    }

    #[test]
    fn local_https_configuration_round_trips_without_storing_sentences() {
        let mut configuration = AiConfiguration::new(
            AiProtocol::AnthropicCompatible,
            "https://api.example.com",
            "claude",
        );
        configuration.request_timeout_seconds = 20;
        let encoded = WindowsSettings::configuration_json(&configuration).unwrap();
        let loaded = WindowsSettings::from_values(Some(&encoded), Some("secret")).unwrap();
        assert_eq!(loaded.configuration.unwrap(), configuration);
        assert_eq!(loaded.api_key.as_deref(), Some("secret"));
        assert!(!encoded.contains("sentence"));
    }
}
