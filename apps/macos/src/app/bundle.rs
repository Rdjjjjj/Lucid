//! 从 Info.plist 读取输入法身份。连接名必须是 `<bundle id>_Connection`。

use objc2_foundation::{NSBundle, NSString};

pub struct BundleInfo {
    pub identifier: String,
    pub connection_name: String,
}

impl BundleInfo {
    pub fn from_main_bundle() -> Self {
        let bundle = NSBundle::mainBundle();
        let identifier = bundle
            .bundleIdentifier()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "io.github.rdj.inputmethod.lucid".to_owned());
        let connection_name = bundle
            .objectForInfoDictionaryKey(&NSString::from_str("InputMethodConnectionName"))
            .and_then(|value| value.downcast::<NSString>().ok())
            .map(|value| value.to_string())
            .unwrap_or_else(|| format!("{identifier}_Connection"));
        Self {
            identifier,
            connection_name,
        }
    }
}
