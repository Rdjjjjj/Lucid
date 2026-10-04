//! 主 App 和输入法共用的设置。Key 跟配置放在同一份用户默认值里，不进钥匙串。

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
}
