//! 设置的读写边界。具体存到哪里由平台壳决定，Core 只规定字段。

use serde::{Deserialize, Serialize};

use crate::ai::AiConfiguration;

pub const CONFIGURATION_KEY: &str = "ai.configuration.v1";
pub const API_KEY_KEY: &str = "ai.apiKey.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSettings {
    pub configuration: Option<AiConfiguration>,
    pub api_key: Option<String>,
}

pub trait SettingsStore {
    fn load(&self) -> Result<StoredSettings, String>;
    fn save(&self, settings: &StoredSettings) -> Result<(), String>;
}
