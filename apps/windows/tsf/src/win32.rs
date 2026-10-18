//! TSF 壳使用的 Windows 小工具：按键翻译、时间戳和 COM 初始化。

use windows::Win32::{
    System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
    UI::Input::KeyboardAndMouse::{GetKeyState, ToUnicode, VK_CONTROL},
};

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
