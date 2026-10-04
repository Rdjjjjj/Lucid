//! WeChat compatibility without synthetic keys or clipboard changes.
//! Only the exact focused editor captured for this suggestion may be changed.
//! All text stays in memory; permissions are queried, never granted here.

use std::ffi::c_void;
use std::ptr;

use lucid_core::{ReplacementOutcome, Utf16Range};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::NSString;

use crate::imk::{is_authentication_host, secure_input_enabled};

type Ref = *const c_void;
const RANGE_TYPE: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Range {
    location: isize,
    length: isize,
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> Ref;
    fn AXUIElementCopyAttributeValue(element: Ref, name: Ref, value: *mut Ref) -> i32;
    fn AXUIElementSetAttributeValue(element: Ref, name: Ref, value: Ref) -> i32;
    fn AXUIElementIsAttributeSettable(element: Ref, name: Ref, settable: *mut u8) -> i32;
    fn AXUIElementSetMessagingTimeout(element: Ref, timeout: f32) -> i32;
    fn AXUIElementGetTypeID() -> usize;
    fn AXUIElementGetPid(element: Ref, pid: *mut i32) -> i32;
    fn AXUIElementCopyAttributeValues(
        element: Ref,
        name: Ref,
        index: isize,
        max: isize,
        values: *mut Ref,
    ) -> i32;
    fn AXValueCreate(kind: u32, value: *const c_void) -> Ref;
    fn AXValueGetValue(value: Ref, kind: u32, output: *mut c_void) -> u8;
    fn AXValueGetTypeID() -> usize;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: Ref);
    fn CFRetain(value: Ref) -> Ref;
    fn CFArrayGetCount(array: Ref) -> isize;
    fn CFArrayGetValueAtIndex(array: Ref, index: isize) -> Ref;
    fn CFEqual(a: Ref, b: Ref) -> u8;
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFStringGetTypeID() -> usize;
}

struct Owned(Ref);
impl Owned {
    fn new(value: Ref) -> Option<Self> {
        // Do not use `then_some(Self(value))` here: `then_some` eagerly
        // evaluates its argument, so a null AX out-parameter would construct
        // an Owned value and immediately drop it via CFRelease(NULL).
        if value.is_null() {
            None
        } else {
            Some(Self(value))
        }
    }
    fn string(&self) -> Option<String> {
        if unsafe { CFGetTypeID(self.0) != CFStringGetTypeID() } {
            return None;
        }
        // CFString and NSString are toll-free bridged. The owning CF reference
        // remains alive for the duration of the borrow.
        Some(unsafe { &*self.0.cast::<NSString>() }.to_string())
    }
    fn range(&self) -> Option<Range> {
        if unsafe { CFGetTypeID(self.0) != AXValueGetTypeID() } {
            return None;
        }
        let mut range = Range::default();
        let ok = unsafe { AXValueGetValue(self.0, RANGE_TYPE, (&mut range as *mut Range).cast()) };
        (ok != 0 && range.location >= 0 && range.length >= 0).then_some(range)
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        // AX APIs are C APIs and can legally leave the out-parameter null on
        // an error. Keep the guard here as a final line of defense: a null
        // CFRelease aborts the entire input-method process on macOS.
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

fn string_ref(value: &NSString) -> Ref {
    (value as *const NSString).cast()
}
fn attribute(element: Ref, name: &str) -> Option<Owned> {
    let name = NSString::from_str(name);
    let mut value = ptr::null();
    let error = unsafe { AXUIElementCopyAttributeValue(element, string_ref(&name), &mut value) };
    // Only take ownership after both checks pass. In particular, do not wrap
    // an out-parameter returned alongside an AX error: some hosts leave a
    // stale/null value there, and dropping it would call CFRelease(NULL) or
    // release memory that we do not own.
    if error != 0 || value.is_null() {
        return None;
    }
    Some(Owned(value))
}
fn children(element: Ref) -> Vec<Owned> {
    let name = NSString::from_str("AXChildren");
    let mut value = ptr::null();
    let error =
        unsafe { AXUIElementCopyAttributeValues(element, string_ref(&name), 0, 80, &mut value) };
    if error != 0 || value.is_null() {
        return Vec::new();
    }
    let owned = Owned(value);
    let count = unsafe { CFArrayGetCount(owned.0) };
    (0..count)
        .filter_map(|index| {
            let ptr = unsafe { CFArrayGetValueAtIndex(owned.0, index) };
            if ptr.is_null() || unsafe { CFGetTypeID(ptr) != AXUIElementGetTypeID() } {
                return None;
            }
            unsafe { CFRetain(ptr) };
            Some(Owned(ptr))
        })
        .collect()
}

fn focused_or_text_editor(app: Ref) -> Option<Owned> {
    if let Some(focused) = attribute(app, "AXFocusedUIElement") {
        return Some(focused);
    }
    let windows = children(app);
    tracing::info!(windows = windows.len(), "微信没有焦点元素，改为搜索窗口");
    fn walk(element: Ref, depth: usize, budget: &mut usize) -> Option<Owned> {
        if depth > 8 || *budget == 0 {
            return None;
        }
        *budget -= 1;
        let role = attribute(element, "AXRole").and_then(|v| v.string());
        let focused = attribute(element, "AXFocused").and_then(|v| v.string());
        if matches!(
            role.as_deref(),
            Some("AXTextArea" | "AXTextField" | "AXComboBox")
        ) && focused.as_deref() == Some("1")
        {
            unsafe { CFRetain(element) };
            return Some(Owned(element));
        }
        for child in children(element) {
            if let Some(found) = walk(child.0, depth + 1, budget) {
                return Some(found);
            }
        }
        None
    }
    let mut budget = 300usize;
    for window in windows {
        if let Some(found) = walk(window.0, 0, &mut budget) {
            return Some(found);
        }
    }
    None
}

fn set_attribute(element: Ref, name: &str, value: Ref) -> bool {
    let name = NSString::from_str(name);
    unsafe { AXUIElementSetAttributeValue(element, string_ref(&name), value) == 0 }
}
fn settable(element: Ref, name: &str) -> bool {
    let name = NSString::from_str(name);
    let mut writable = 0;
    unsafe {
        AXUIElementIsAttributeSettable(element, string_ref(&name), &mut writable) == 0
            && writable != 0
    }
}
fn range_value(range: Range) -> Option<Owned> {
    Owned::new(unsafe { AXValueCreate(RANGE_TYPE, (&range as *const Range).cast()) })
}

pub fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

pub struct Editor {
    app: Owned,
    element: Owned,
    host: String,
    pid: i32,
}
impl Editor {
    pub fn capture(host: &str) -> Option<Self> {
        if !trusted() {
            tracing::info!("AX capture 失败：LucidInputMethod 未获得辅助功能信任");
            return None;
        }
        if secure_input_enabled() || is_authentication_host(host) {
            tracing::info!("AX capture 失败：安全输入或认证窗口");
            return None;
        }
        let front = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let front_host = front.bundleIdentifier()?.to_string();
        if front_host != host {
            tracing::info!(expected = %host, actual = %front_host, "AX capture 失败：微信不是当前前台应用");
            return None;
        }
        let pid = front.processIdentifier();
        let app = Owned::new(unsafe { AXUIElementCreateApplication(pid) })?;
        // Bound each AX round trip; do not stall an input callback indefinitely.
        unsafe { AXUIElementSetMessagingTimeout(app.0, 0.2) };
        let Some(element) = focused_or_text_editor(app.0) else {
            tracing::info!("AX capture 失败：微信没有返回焦点输入框");
            return None;
        };
        if unsafe { CFGetTypeID(element.0) != AXUIElementGetTypeID() } {
            tracing::info!("AX capture 失败：AXFocusedUIElement 类型不正确");
            return None;
        }
        let mut element_pid = 0;
        let pid_error = unsafe { AXUIElementGetPid(element.0, &mut element_pid) };
        if pid_error != 0 || element_pid != pid {
            tracing::info!(
                pid_error,
                element_pid,
                expected_pid = pid,
                "AX capture 失败：焦点元素不属于微信"
            );
            return None;
        }
        let role = attribute(element.0, "AXRole").and_then(|v| v.string());
        let subrole = attribute(element.0, "AXSubrole").and_then(|v| v.string());
        tracing::info!(
            ?role,
            ?subrole,
            value_writable = settable(element.0, "AXValue"),
            "AX 捕获微信焦点元素"
        );
        // WeChat versions expose the editor as AXTextArea, AXTextField,
        // AXWebArea, or AXGroup depending on whether the conversation view is
        // native or WebKit-backed. Do not reject a valid focused element just
        // because its role is not one of the two standard text roles.
        if subrole.as_deref() == Some("AXSecureTextField") {
            tracing::info!("AX capture 失败：安全文本框");
            return None;
        }
        Some(Self {
            app,
            element,
            host: host.to_owned(),
            pid,
        })
    }

    fn still_focused(&self) -> bool {
        if !trusted() || secure_input_enabled() {
            return false;
        }
        let Some(front) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
            return false;
        };
        if front.processIdentifier() != self.pid
            || front.bundleIdentifier().map(|v| v.to_string()).as_deref() != Some(&self.host)
        {
            return false;
        }
        attribute(self.app.0, "AXFocusedUIElement")
            .is_some_and(|current| unsafe { CFEqual(current.0, self.element.0) != 0 })
    }

    pub fn value_before_caret(&self, caret: usize) -> Option<String> {
        let value = attribute(self.element.0, "AXValue").and_then(|item| item.string())?;
        let units = value.encode_utf16().collect::<Vec<_>>();
        if caret > units.len() {
            tracing::info!(caret, units = units.len(), "AX 光标超出微信文本");
            return None;
        }
        let start = caret.saturating_sub(512);
        let mut sentence_start = start;
        for (index, unit) in units
            .iter()
            .enumerate()
            .take(caret.saturating_sub(1))
            .skip(start)
        {
            if matches!(
                *unit,
                0x000A
                    | 0x000D
                    | 0x2028
                    | 0x2029
                    | 0x002E
                    | 0x003F
                    | 0x0021
                    | 0x3002
                    | 0xFF01
                    | 0xFF1F
            ) {
                sentence_start = index + 1;
            }
        }
        let sentence = String::from_utf16_lossy(&units[sentence_start..caret])
            .trim()
            .to_owned();
        tracing::info!(
            caret,
            units = sentence.encode_utf16().count(),
            "AX 读回微信句末文本"
        );
        (!sentence.is_empty()).then_some(sentence)
    }

    pub fn replace(
        &self,
        original: &str,
        replacement: &str,
        range: Option<Utf16Range>,
    ) -> ReplacementOutcome {
        if !self.still_focused() {
            tracing::info!("AX 替换失败：焦点已离开微信输入框");
            return ReplacementOutcome::Stale;
        }

        let Some(before) = attribute(self.element.0, "AXValue").and_then(|v| v.string()) else {
            tracing::info!("AX 替换失败：微信没有提供 AXValue");
            return ReplacementOutcome::Unsupported;
        };
        let previous = attribute(self.element.0, "AXSelectedTextRange");
        let caret = previous
            .as_ref()
            .and_then(|value| value.range())
            .map(|selection| {
                Utf16Range::new(selection.location as usize, selection.length as usize)
            });
        let behind =
            lucid_core::replacement::range_behind_caret(original.encode_utf16().count(), caret);
        // No search across an entire conversation; only the saved range or the
        // sentence immediately preceding the current caret is eligible.
        let target = lucid_core::replacement::preferred_range(original, &[range, behind], |r| {
            slice(&before, r)
        });
        let Some(target) = target else {
            tracing::info!(
                before_chars = before.chars().count(),
                "AX 替换失败：原文范围不匹配"
            );
            return ReplacementOutcome::Stale;
        };
        let Some(expected) = replaced_value(&before, target, replacement) else {
            tracing::info!(?target, "AX 替换失败：无法计算替换后的完整文本");
            return ReplacementOutcome::Stale;
        };

        // WeChat exposes a normal editable AXValue, but its AXSelectedText is
        // commonly read-only. The previous implementation required both
        // attributes to be writable, so it always returned Unsupported even
        // with Accessibility permission enabled. Writing AXValue once after
        // an exact read/focus check is the stable path and avoids synthetic
        // key events or clipboard access.
        if settable(self.element.0, "AXValue") {
            if !self.still_focused()
                || attribute(self.element.0, "AXValue")
                    .and_then(|v| v.string())
                    .as_deref()
                    != Some(&before)
            {
                tracing::info!("AX 替换取消：写入前微信内容或焦点已变化");
                return ReplacementOutcome::Stale;
            }
            let text = NSString::from_str(&expected);
            let accepted = set_attribute(self.element.0, "AXValue", string_ref(&text));
            let after = attribute(self.element.0, "AXValue").and_then(|v| v.string());
            tracing::info!(
                accepted,
                verified = after.as_deref() == Some(&expected),
                "AXValue 替换结果"
            );
            return if accepted && after.as_deref() == Some(&expected) {
                ReplacementOutcome::Replaced
            } else {
                ReplacementOutcome::Unverified
            };
        }

        // Compatibility fallback for hosts that do not make AXValue writable
        // but do expose a writable selected-text API.
        let Some(previous) = previous else {
            tracing::info!("AX 替换失败：AXValue 和选区接口都不可写");
            return ReplacementOutcome::Unsupported;
        };
        if !settable(self.element.0, "AXSelectedTextRange")
            || !settable(self.element.0, "AXSelectedText")
        {
            tracing::info!("AX 替换失败：AXValue、AXSelectedText 均不可写");
            return ReplacementOutcome::Unsupported;
        }
        let Ok(location) = isize::try_from(target.location) else {
            return ReplacementOutcome::Stale;
        };
        let Ok(length) = isize::try_from(target.length) else {
            return ReplacementOutcome::Stale;
        };
        let Some(selected) = range_value(Range { location, length }) else {
            return ReplacementOutcome::Unsupported;
        };
        if !self.still_focused()
            || !set_attribute(self.element.0, "AXSelectedTextRange", selected.0)
        {
            return ReplacementOutcome::Unsupported;
        }
        let verified = attribute(self.element.0, "AXSelectedTextRange").and_then(|v| v.range())
            == Some(Range { location, length })
            && attribute(self.element.0, "AXSelectedText").and_then(|v| v.string())
                == slice(&before, target)
            && attribute(self.element.0, "AXValue")
                .and_then(|v| v.string())
                .as_deref()
                == Some(&before)
            && self.still_focused();
        if !verified {
            if self.still_focused() {
                set_attribute(self.element.0, "AXSelectedTextRange", previous.0);
            }
            return ReplacementOutcome::Stale;
        }
        let text = NSString::from_str(replacement);
        let accepted = set_attribute(self.element.0, "AXSelectedText", string_ref(&text));
        let after = attribute(self.element.0, "AXValue").and_then(|v| v.string());
        if accepted && after.as_deref() == Some(&expected) {
            ReplacementOutcome::Replaced
        } else {
            if after.as_deref() == Some(&before) && self.still_focused() {
                set_attribute(self.element.0, "AXSelectedTextRange", previous.0);
            }
            ReplacementOutcome::Unverified
        }
    }
}

fn slice(text: &str, range: Utf16Range) -> Option<String> {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16(units.get(range.location..range.location.checked_add(range.length)?)?).ok()
}
fn replaced_value(text: &str, range: Utf16Range, replacement: &str) -> Option<String> {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    let end = range.location.checked_add(range.length)?;
    // Reject ranges that split surrogate pairs.
    slice(text, range)?;
    String::from_utf16(units.get(..range.location)?).ok()?;
    String::from_utf16(units.get(end..)?).ok()?;
    units.splice(range.location..end, replacement.encode_utf16());
    String::from_utf16(&units).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_replacement_preserves_prefix_suffix_and_emoji() {
        let text = "😀 nihao. tail";
        assert_eq!(
            replaced_value(text, Utf16Range::new(3, 6), "Hello."),
            Some("😀 Hello. tail".into())
        );
        assert_eq!(replaced_value(text, Utf16Range::new(1, 1), "x"), None);
        assert_eq!(
            replaced_value(text, Utf16Range::new(usize::MAX, 6), "x"),
            None
        );
    }
}
