//! 主 App 和输入法共用同一份用户默认值。Key 不进钥匙串。

use lucid_core::{AiConfiguration, AiProtocol};
use lucid_platform::{API_KEY_KEY, CONFIGURATION_KEY, DEFAULTS_SUITE};
use objc2::AnyThread;
use objc2_foundation::{NSData, NSString, NSUserDefaults};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredConfiguration {
    #[serde(alias = "apiProtocol")]
    api_protocol: AiProtocol,
    #[serde(alias = "baseURL")]
    base_url: UrlValue,
    model: String,
    #[serde(default = "default_timeout", alias = "requestTimeout")]
    request_timeout_seconds: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum UrlValue {
    Text(String),
    Object {
        url: Option<String>,
        relative: Option<String>,
    },
}

impl UrlValue {
    fn as_text(&self) -> String {
        match self {
            Self::Text(value) => value.clone(),
            Self::Object { relative, .. } => relative.clone().unwrap_or_default(),
        }
    }
}

fn default_timeout() -> u64 {
    30
}

pub struct Settings {
    defaults: objc2::rc::Retained<NSUserDefaults>,
}

impl Settings {
    pub fn shared() -> Self {
        let defaults = unsafe {
            NSUserDefaults::initWithSuiteName(
                NSUserDefaults::alloc(),
                Some(&NSString::from_str(DEFAULTS_SUITE)),
            )
        };
        Self {
            defaults: defaults.expect("无法打开 Lucid 设置"),
        }
    }

    pub fn configuration(&self) -> Option<AiConfiguration> {
        let data = self
            .defaults
            .dataForKey(&NSString::from_str(CONFIGURATION_KEY))?;
        let bytes = data.to_vec();
        let stored = serde_json::from_slice::<StoredConfiguration>(&bytes).ok()?;
        let mut configuration =
            AiConfiguration::new(stored.api_protocol, stored.base_url.as_text(), stored.model);
        configuration.request_timeout_seconds = stored.request_timeout_seconds;
        configuration.validate(true).ok()?;
        Some(configuration)
    }

    pub fn save_configuration(&self, configuration: &AiConfiguration) -> Result<(), String> {
        configuration
            .validate(false)
            .map_err(|error| error.to_string())?;
        let stored = StoredConfiguration {
            api_protocol: configuration.api_protocol,
            base_url: UrlValue::Text(configuration.base_url.clone()),
            model: configuration.model.clone(),
            request_timeout_seconds: configuration.request_timeout_seconds,
        };
        let bytes = serde_json::to_vec(&stored).map_err(|error| error.to_string())?;
        let data = NSData::from_vec(bytes);
        unsafe {
            self.defaults
                .setObject_forKey(Some(&data), &NSString::from_str(CONFIGURATION_KEY));
        }
        let _ = self.defaults.synchronize();
        Ok(())
    }

    pub fn api_key(&self) -> Option<String> {
        self.defaults
            .stringForKey(&NSString::from_str(API_KEY_KEY))
            .map(|value| value.to_string())
            .filter(|value| !value.is_empty())
    }

    pub fn save_api_key(&self, api_key: &str) {
        unsafe {
            self.defaults.setObject_forKey(
                Some(&NSString::from_str(api_key.trim())),
                &NSString::from_str(API_KEY_KEY),
            );
        }
        let _ = self.defaults.synchronize();
    }
}
