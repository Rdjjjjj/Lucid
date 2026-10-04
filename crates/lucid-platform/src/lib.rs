//! 各平台壳共用的配置键和建议帧。
//!
//! 这里没有 AppKit、没有输入法框架。macOS 壳读取这些类型，Windows 壳以后也读同一份。

pub mod config;

pub use config::{API_KEY_KEY, CONFIGURATION_KEY, DEFAULTS_SUITE, SharedSettings};
