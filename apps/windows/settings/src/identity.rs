//! Lucid Windows 文本服务的稳定标识。

#![allow(dead_code)]
//!
//! CLSID 和 Profile GUID 一旦发布就不能改：已安装的输入法靠它们找到 DLL。

use std::fmt::Write as _;

/// `{7C4E2A11-6B0D-4E58-9A31-1D5C8B0E4F27}`
pub const CLSID_TEXT_SERVICE: [u8; 16] = [
    0x7c, 0x4e, 0x2a, 0x11, 0x6b, 0x0d, 0x4e, 0x58, 0x9a, 0x31, 0x1d, 0x5c, 0x8b, 0x0e, 0x4f, 0x27,
];

/// `{A91F6C42-3D18-4B77-8E05-6C2A9D14B8E3}`
pub const PROFILE_GUID: [u8; 16] = [
    0xa9, 0x1f, 0x6c, 0x42, 0x3d, 0x18, 0x4b, 0x77, 0x8e, 0x05, 0x6c, 0x2a, 0x9d, 0x14, 0xb8, 0xe3,
];

/// 英语（美国）。Lucid 只注册这一个语言配置。
pub const LANG_EN_US: u16 = 0x0409;

pub const PROFILE_DESCRIPTION: &str = "Lucid";
pub const DLL_FILE_NAME: &str = "LucidTextService.dll";
pub const SETTINGS_FILE_NAME: &str = "Lucid.exe";

pub fn clsid_brace() -> String {
    format!("{{{}}}", guid_string(&CLSID_TEXT_SERVICE))
}

pub fn profile_brace() -> String {
    format!("{{{}}}", guid_string(&PROFILE_GUID))
}

pub fn guid_string(bytes: &[u8; 16]) -> String {
    let mut text = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            text.push('-');
        }
        let _ = write!(text, "{byte:02X}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_keep_their_published_text() {
        assert_eq!(clsid_brace(), "{7C4E2A11-6B0D-4E58-9A31-1D5C8B0E4F27}");
        assert_eq!(profile_brace(), "{A91F6C42-3D18-4B77-8E05-6C2A9D14B8E3}");
        assert_eq!(LANG_EN_US, 0x0409);
    }
}
