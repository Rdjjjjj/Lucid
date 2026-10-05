//! 对 IMK 文本客户端的薄封装。绑定没有生成 IMKTextInput，这里直接发消息。

use std::ffi::CString;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::msg_send;
use objc2_foundation::{NSAttributedString, NSNotFound, NSRange, NSRect, NSString};

use lucid_core::Utf16Range;

/// `{NSNotFound, 0}`：不替换任何已有文本，插到当前光标位置。
/// 之前误写成 `{NSNotFound, NSNotFound}`，很多宿主会直接忽略这次插入，表现为切到 Lucid 后打不出字。
const NO_REPLACEMENT: NSRange = NSRange::new(NSNotFound as usize, 0);

#[derive(Clone, Copy)]
pub struct TextClient<'a> {
    object: &'a AnyObject,
}

impl<'a> TextClient<'a> {
    pub fn new(object: &'a AnyObject) -> Self {
        Self { object }
    }

    /// Never collect or translate credentials, even if a host accidentally
    /// delivers keys to IMK while secure input is enabled.
    pub fn is_sensitive_input(&self) -> bool {
        secure_input_enabled() || is_authentication_host(&self.bundle_identifier())
    }

    pub fn object(&self) -> &'a AnyObject {
        self.object
    }

    pub fn bundle_identifier(&self) -> String {
        let bundle: Option<Retained<NSString>> =
            unsafe { msg_send![self.object, bundleIdentifier] };
        bundle
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_owned())
    }

    pub fn insert_text(&self, text: &str, replacement: Option<Utf16Range>) {
        let string = NSString::from_str(text);
        let range = replacement.map(ns_range).unwrap_or(NO_REPLACEMENT);
        let responds = self.responds_to("insertText:replacementRange:");
        let before = self.selected_range();
        tracing::info!(
            responds,
            ?before,
            loc = range.location,
            len = range.length,
            "insert_text 调用前"
        );
        unsafe {
            let _: () = msg_send![self.object, insertText: &*string, replacementRange: range];
        }
        let after = self.selected_range();
        tracing::info!(?after, "insert_text 调用后");
    }

    pub fn set_marked_text(&self, text: &str, selection: Utf16Range, replacement: Utf16Range) {
        let string = NSString::from_str(text);
        unsafe {
            let _: () = msg_send![
                self.object,
                setMarkedText: &*string,
                selectionRange: ns_range(selection),
                replacementRange: ns_range(replacement)
            ];
        }
    }

    pub fn selected_range(&self) -> Option<Utf16Range> {
        let range: NSRange = unsafe { msg_send![self.object, selectedRange] };
        usable_range(range)
    }

    pub fn marked_range(&self) -> Option<Utf16Range> {
        let range: NSRange = unsafe { msg_send![self.object, markedRange] };
        usable_range(range)
    }

    pub fn substring(&self, range: Utf16Range) -> Option<String> {
        let text: Option<Retained<NSAttributedString>> =
            unsafe { msg_send![self.object, attributedSubstringFromRange: ns_range(range)] };
        text.map(|value| value.string().to_string())
    }

    pub fn responds_to(&self, selector: &str) -> bool {
        let name = CString::new(selector).expect("selector");
        let selector = objc2::runtime::Sel::register(&name);
        unsafe { msg_send![self.object, respondsToSelector: selector] }
    }

    /// Sends the standard responder action with its required sender argument.
    ///
    /// Native NSTextView clients usually expose `deleteBackward:` directly.
    /// WeChat's legacy IMK wrapper does not, but it does expose
    /// `doCommandBySelector:`; sending the command through that protocol is the
    /// only way to make the wrapper delete the already-committed pinyin. If we
    /// skip this fallback, `insertText(_:replacementRange:)` is accepted by
    /// WeChat as an append, producing `nihao.Hello.` instead of a replacement.
    pub fn delete_backward(&self) {
        if !self.do_command_by_selector("deleteBackward:") {
            self.send_edit_command("deleteBackward:");
        }
    }

    /// Prefer deleting an active selection. WeChat's IMK proxy can report
    /// `deleteBackward:` while ignoring that message, but still forwards `delete:`
    /// once the original sentence is selected.
    pub fn delete_selection_or_backward(&self) {
        if self.selected_range().is_some_and(|range| range.length > 0) {
            if self.do_command_by_selector("delete:") {
                return;
            }
            if self.responds_to("delete:") {
                self.send_edit_command("delete:");
                return;
            }
        }
        self.delete_backward();
    }

    pub fn do_command_by_selector(&self, command: &str) -> bool {
        if !self.responds_to("doCommandBySelector:") {
            return false;
        }
        let name = CString::new(command).expect("selector");
        let command_sel = objc2::runtime::Sel::register(&name);
        unsafe {
            let _: () = msg_send![self.object, doCommandBySelector: command_sel];
        }
        true
    }

    fn send_edit_command(&self, command: &str) {
        let before = self.selected_range();
        let name = CString::new(command).expect("selector");
        let command_sel = objc2::runtime::Sel::register(&name);
        if self.responds_to("doCommandBySelector:") {
            unsafe {
                let _: () = msg_send![self.object, doCommandBySelector: command_sel];
            }
        } else if self.responds_to(command) {
            let sent = unsafe {
                match command {
                    "delete:" => {
                        let _: () = msg_send![self.object, delete: None::<&AnyObject>];
                        true
                    }
                    "deleteBackward:" => {
                        let _: () = msg_send![self.object, deleteBackward: None::<&AnyObject>];
                        true
                    }
                    _ => false,
                }
            };
            if !sent {
                tracing::info!(command, ?before, "未识别的宿主删除命令");
                return;
            }
        } else {
            tracing::info!(command, ?before, "宿主不支持删除命令");
            return;
        }
        let after = self.selected_range();
        tracing::info!(command, ?before, ?after, "调用宿主删除命令");
    }

    /// Ask the host which editing selectors its IMK proxy actually implements.
    /// WeChat's legacy wrapper answers this differently from a normal text view,
    /// so the replacement path logs the result instead of guessing.
    pub fn selector_support(&self, selectors: &[&str]) -> String {
        selectors
            .iter()
            .map(|selector| format!("{selector}={}", self.responds_to(selector)))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Turn already-committed text into a composition, then commit the English
    /// over that composition. This is the path used by real input methods and
    /// does not depend on `setSelectedRange:` or `deleteBackward:`, both of
    /// which WeChat's legacy proxy does not implement.
    pub fn commit_marked_replacement(
        &self,
        _original: &str,
        _replacement: &str,
        range: lucid_core::Utf16Range,
    ) -> bool {
        // Probe only. WeChat's legacy proxy reports setMarkedText support, but
        // a live test showed that it ignores the requested range and inserts a
        // second copy after the committed sentence. Do not call it again until
        // a separate, verified cleanup path exists.
        tracing::info!(?range, "微信组字替换已跳过：该代理会把组字追加到原文后面");
        false
    }

    pub fn set_selected_range(&self, range: Utf16Range) -> bool {
        if !self.responds_to("setSelectedRange:") {
            return false;
        }
        // NSRange is a C struct argument, not an object argument. Calling
        // `performSelector:withObject:` here passes an NSValue and silently
        // fails in several Cocoa/WebKit hosts, so the deletion replacement path
        // never reaches the original sentence.
        unsafe {
            let _: () = msg_send![self.object, setSelectedRange: ns_range(range)];
        }
        self.selected_range() == Some(range)
    }

    /// Returns the screen-space caret rectangle supplied by NSTextInputClient.
    /// This is intentionally queried only after the async result is ready: doing
    /// it inside a key callback can deadlock editors such as Sublime Text.
    /// Legacy IMK clients (including some WeChat releases) expose caret
    /// geometry through this protocol instead of firstRectForCharacterRange.
    pub fn line_height_rect_for_character(&self, index: usize) -> Option<NSRect> {
        if !self.responds_to("attributesForCharacterIndex:lineHeightRectangle:") {
            return None;
        }
        let mut rect = NSRect::new(
            objc2_foundation::NSPoint::new(0.0, 0.0),
            objc2_foundation::NSSize::new(0.0, 0.0),
        );
        let _: Option<Retained<AnyObject>> = unsafe {
            msg_send![
                self.object,
                attributesForCharacterIndex: index,
                lineHeightRectangle: &mut rect
            ]
        };
        valid_rect(rect).then_some(rect)
    }

    pub fn first_rect_for_character_range(&self, range: Utf16Range) -> Option<NSRect> {
        if !self.responds_to("firstRectForCharacterRange:actualRange:") {
            return None;
        }
        let mut actual = ns_range(range);
        let rect: NSRect = unsafe {
            msg_send![
                self.object,
                firstRectForCharacterRange: ns_range(range),
                actualRange: &mut actual
            ]
        };
        valid_rect(rect).then_some(rect)
    }
}

fn valid_rect(rect: NSRect) -> bool {
    rect.size.width.is_finite()
        && rect.size.height.is_finite()
        && rect.origin.x.is_finite()
        && rect.origin.y.is_finite()
        && (rect.size.width > 0.0 || rect.size.height > 0.0)
}

fn ns_range(range: Utf16Range) -> NSRange {
    NSRange::new(range.location, range.length)
}

fn usable_range(range: NSRange) -> Option<Utf16Range> {
    if range.location == NSNotFound as usize {
        None
    } else {
        Some(Utf16Range::new(range.location, range.length))
    }
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

pub fn secure_input_enabled() -> bool {
    unsafe { IsSecureEventInputEnabled() != 0 }
}

pub fn is_authentication_host(host: &str) -> bool {
    matches!(
        host,
        "com.apple.SecurityAgent" | "com.apple.loginwindow" | "com.apple.AuthorizationHost"
    )
}

#[cfg(test)]
mod privacy_tests {
    use super::*;

    #[test]
    fn authentication_hosts_are_excluded() {
        for host in [
            "com.apple.SecurityAgent",
            "com.apple.loginwindow",
            "com.apple.AuthorizationHost",
        ] {
            assert!(is_authentication_host(host));
        }
        assert!(!is_authentication_host("com.sublimetext.4"));
        assert!(!is_authentication_host("com.tencent.xinWeChat"));
    }
}
