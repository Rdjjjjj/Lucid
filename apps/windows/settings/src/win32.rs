//! 本机设置和按键字符。文字写入不经过这里。

use windows::{
    Win32::{
        System::{
            Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
            Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
                RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW,
            },
        },
        UI::{
            Input::KeyboardAndMouse::{GetKeyState, ToUnicode, VK_CONTROL},
            WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
        },
    },
    core::{HSTRING, PCWSTR},
};

use crate::settings::{SETTINGS_KEY_PATH, WindowsSettings};

pub fn character_from_key(virtual_key: u32, scan_code: u32) -> Option<char> {
    let mut buffer = [0u16; 8];
    let written = unsafe { ToUnicode(virtual_key, scan_code, None, &mut buffer, 0) };
    if written <= 0 {
        return None;
    }
    char::decode_utf16(buffer.into_iter().take(written as usize))
        .next()?
        .ok()
}

pub fn control_is_down() -> bool {
    let state = unsafe { GetKeyState(i32::from(VK_CONTROL.0)) };
    state < 0
}

pub fn monotonic_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub fn initialize_com() -> windows::core::Result<()> {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() }
}

pub fn load_settings() -> Result<WindowsSettings, String> {
    let configuration = read_string(WindowsSettings::configuration_key())?;
    let api_key = read_string(WindowsSettings::api_key_key())?;
    WindowsSettings::from_values(configuration.as_deref(), api_key.as_deref())
}

pub fn save_settings(settings: &WindowsSettings) -> Result<(), String> {
    if let Some(configuration) = &settings.configuration {
        write_string(
            WindowsSettings::configuration_key(),
            &WindowsSettings::configuration_json(configuration)?,
        )?;
    }
    if let Some(api_key) = &settings.api_key {
        write_string(WindowsSettings::api_key_key(), api_key)?;
    }
    Ok(())
}

fn read_string(name: &str) -> Result<Option<String>, String> {
    let subkey = HSTRING::from(SETTINGS_KEY_PATH);
    let value = HSTRING::from(name);
    let mut bytes = vec![0u8; 8192];
    let mut size = bytes.len() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(bytes.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status.0 == 2 {
        return Ok(None);
    }
    if status.0 != 0 {
        return Err("无法读取 Lucid 设置。".to_owned());
    }
    let units = (size as usize / 2).min(bytes.len() / 2);
    let wide = unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<u16>(), units) };
    Ok(Some(
        String::from_utf16_lossy(wide)
            .trim_end_matches('\0')
            .to_owned(),
    ))
}

fn write_string(name: &str, value: &str) -> Result<(), String> {
    let subkey = HSTRING::from(SETTINGS_KEY_PATH);
    let mut key = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_READ | KEY_WRITE,
            None,
            &mut key,
            None,
        )
    };
    if status.0 != 0 {
        return Err("无法保存 Lucid 设置。".to_owned());
    }
    let data = wide(value);
    let bytes = unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2) };
    let name = HSTRING::from(name);
    let written = unsafe { RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes)) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    if written.0 != 0 {
        return Err("无法保存 Lucid 设置。".to_owned());
    }
    Ok(())
}

pub fn show_error(error: &str) {
    let text = wide(error);
    let title = wide("Lucid");
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
