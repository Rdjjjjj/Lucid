//! 注册、启用并选择 Lucid 输入源。
//!
//! macOS 会缓存 TIS 输入源列表；安装脚本只调用这里的命令行入口，避免把
//! `target/`、旧副本或输入法自己的设置 App 留在「添加输入法」列表里。

use std::ffi::{c_char, c_void};
use std::ptr;

const BUNDLE_ID: &str = "io.github.rdj.inputmethod.lucid";
const MODE_ID: &str = "io.github.rdj.inputmethod.lucid.english";
const UTF8_ENCODING: u32 = 0x0800_0100;

type Boolean = u8;
type CFIndex = isize;
type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFArrayRef = *const c_void;
type CFUrlRef = *const c_void;
type TISInputSourceRef = *const c_void;
type OSStatus = i32;

unsafe extern "C" {
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: *const c_void,
        buffer: *const u8,
        length: CFIndex,
        is_directory: Boolean,
    ) -> CFUrlRef;
    fn CFStringGetCString(
        string: CFStringRef,
        buffer: *mut c_char,
        buffer_size: CFIndex,
        encoding: u32,
    ) -> Boolean;
    fn CFBooleanGetValue(value: CFTypeRef) -> Boolean;
    fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, index: CFIndex) -> *const c_void;
    fn CFRetain(value: CFTypeRef) -> CFTypeRef;
    fn CFRelease(value: CFTypeRef);
    static kTISPropertyInputSourceID: CFStringRef;
    static kTISPropertyInputSourceIsEnabled: CFStringRef;
    static kTISPropertyInputSourceIsSelectCapable: CFStringRef;

    fn TISRegisterInputSource(url: CFUrlRef) -> OSStatus;
    fn TISCreateInputSourceList(
        properties: CFTypeRef,
        include_all_installed: Boolean,
    ) -> CFArrayRef;
    fn TISGetInputSourceProperty(source: TISInputSourceRef, key: CFStringRef) -> CFTypeRef;
    fn TISEnableInputSource(source: TISInputSourceRef) -> OSStatus;
    fn TISDisableInputSource(source: TISInputSourceRef) -> OSStatus;
    fn TISSelectInputSource(source: TISInputSourceRef) -> OSStatus;
}

fn source_id(source: TISInputSourceRef, id_key: CFStringRef) -> Option<String> {
    let value = unsafe { TISGetInputSourceProperty(source, id_key) };
    if value.is_null() {
        return None;
    }
    let mut buffer = vec![0 as c_char; 512];
    let ok = unsafe {
        CFStringGetCString(
            value,
            buffer.as_mut_ptr(),
            buffer.len() as CFIndex,
            UTF8_ENCODING,
        )
    };
    if ok == 0 {
        return None;
    }
    let bytes = buffer
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect::<Vec<_>>();
    String::from_utf8(bytes).ok()
}

fn source_bool(source: TISInputSourceRef, key: CFStringRef) -> bool {
    let value = unsafe { TISGetInputSourceProperty(source, key) };
    !value.is_null() && unsafe { CFBooleanGetValue(value) } != 0
}

fn sources() -> Vec<TISInputSourceRef> {
    let list = unsafe { TISCreateInputSourceList(ptr::null(), 1) };
    if list.is_null() {
        return Vec::new();
    }
    let count = unsafe { CFArrayGetCount(list) };
    // The pointers returned by CFArrayGetValueAtIndex are borrowed.  Keeping
    // the array alive only until this function returns used to leave dangling
    // TISInputSource pointers and made `--install` terminate with SIGTRAP on
    // some macOS versions.  Retain each source before releasing the array;
    // callers release the two matches after they finish with them.
    let values = (0..count)
        .map(|index| unsafe { CFArrayGetValueAtIndex(list, index) })
        .filter(|value| !value.is_null())
        .map(|value| unsafe { CFRetain(value as CFTypeRef) as TISInputSourceRef })
        .collect::<Vec<_>>();
    unsafe { CFRelease(list) };
    values
}

fn release_sources(sources: impl IntoIterator<Item = Option<TISInputSourceRef>>) {
    for source in sources.into_iter().flatten() {
        unsafe { CFRelease(source as CFTypeRef) };
    }
}

fn find_sources() -> (Option<TISInputSourceRef>, Option<TISInputSourceRef>) {
    // TIS exposes IDs, not a public bundle-path property. Install scripts keep
    // exactly one canonical bundle registered; do not query invented CF keys.
    let id_key = unsafe { kTISPropertyInputSourceID };
    let mut parent = None;
    let mut child = None;
    for source in sources() {
        let Some(id) = source_id(source, id_key) else {
            unsafe { CFRelease(source as CFTypeRef) };
            continue;
        };
        let slot = if id == BUNDLE_ID {
            &mut parent
        } else if id == MODE_ID {
            &mut child
        } else {
            unsafe { CFRelease(source as CFTypeRef) };
            continue;
        };
        if slot.is_none() {
            if let Some(previous) = slot.replace(source) {
                unsafe { CFRelease(previous as CFTypeRef) };
            }
        } else {
            unsafe { CFRelease(source as CFTypeRef) };
        }
    }
    (parent, child)
}

fn register_bundle() -> Result<(), String> {
    let bundle = objc2_foundation::NSBundle::mainBundle();
    let path = bundle
        .bundleURL()
        .path()
        .map(|value| value.to_string())
        .unwrap_or_default();
    let system_path = "/Library/Input Methods/LucidInputMethod.app";
    let user_path = std::env::var("HOME")
        .map(|home| format!("{home}/Library/Input Methods/LucidInputMethod.app"))
        .unwrap_or_default();
    if path != system_path && path != user_path {
        return Err("只能注册正式安装目录中的 Lucid 输入法，不能注册构建副本或设置 App".to_owned());
    }
    let bytes = path.as_bytes();
    let url = unsafe {
        CFURLCreateFromFileSystemRepresentation(
            ptr::null(),
            bytes.as_ptr(),
            bytes.len() as CFIndex,
            1,
        )
    };
    if url.is_null() {
        return Err("无法创建输入法包地址".to_owned());
    }
    let status = unsafe { TISRegisterInputSource(url) };
    unsafe { CFRelease(url) };
    if status != 0 {
        return Err(format!("注册输入源失败，状态码 {status}"));
    }
    Ok(())
}

pub fn disable() {
    let (parent, child) = find_sources();
    for source in [child, parent].into_iter().flatten() {
        unsafe { TISDisableInputSource(source) };
    }
    release_sources([parent, child]);
}

// A mode-less keyboard input method is itself selectable. The legacy child is
// only a fallback while macOS is migrating the old registration.
fn choose_source<T: Copy>(parent: Option<(T, bool)>, child: Option<(T, bool)>) -> Option<T> {
    parent
        .filter(|(_, selectable)| *selectable)
        .or_else(|| child.filter(|(_, selectable)| *selectable))
        .map(|(source, _)| source)
}

fn enable_if_needed(enabled: bool, enable: impl FnOnce() -> OSStatus) -> OSStatus {
    if enabled { 0 } else { enable() }
}

fn ensure_enabled(source: TISInputSourceRef) -> Result<(), String> {
    let enabled = source_bool(source, unsafe { kTISPropertyInputSourceIsEnabled });
    let status = enable_if_needed(enabled, || unsafe { TISEnableInputSource(source) });
    if status == 0 {
        Ok(())
    } else {
        Err(format!("启用 Lucid 输入源失败，状态码 {status}"))
    }
}

pub fn enable_and_select() -> Result<(), String> {
    // Only the explicit installer entry registers a bundle. Regular launches
    // and selections must never register/enable sources repeatedly.
    register_bundle()?;
    select()
}

pub fn select() -> Result<(), String> {
    let (parent, child) = find_sources();
    let selectable =
        |source| source_bool(source, unsafe { kTISPropertyInputSourceIsSelectCapable });
    let selected = choose_source(
        parent.map(|s| (s, selectable(s))),
        child.map(|s| (s, selectable(s))),
    );
    let result = (|| {
        let source =
            selected.ok_or_else(|| "没有找到可选择的 Lucid 输入源，请先安装输入法。".to_owned())?;
        if Some(source) == child {
            if let Some(parent) = parent {
                ensure_enabled(parent)?;
            }
        }
        ensure_enabled(source)?;
        let status = unsafe { TISSelectInputSource(source) };
        if status != 0 {
            return Err(format!("系统拒绝选择 Lucid，状态码 {status}"));
        }
        Ok(())
    })();
    release_sources([parent, child]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_source_wins_over_stale_legacy_child() {
        assert_eq!(choose_source(Some((1, true)), Some((2, true))), Some(1));
    }

    #[test]
    fn legacy_mode_requires_selectable_child() {
        assert_eq!(choose_source(Some((1, false)), Some((2, true))), Some(2));
        assert_eq!(choose_source(Some((1, false)), None), None);
        assert_eq!(choose_source(None::<(i32, bool)>, None), None);
    }

    #[test]
    fn single_source_does_not_require_child() {
        assert_eq!(choose_source(Some((1, true)), None), Some(1));
    }

    #[test]
    fn enabled_source_is_not_enabled_again() {
        assert_eq!(enable_if_needed(true, || panic!("must not re-enable")), 0);
        assert_eq!(enable_if_needed(false, || -50), -50);
        assert_eq!(enable_if_needed(false, || 0), 0);
    }
}
