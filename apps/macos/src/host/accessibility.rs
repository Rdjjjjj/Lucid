//! WeChat compatibility without synthetic keys or clipboard changes.
//! Only the exact focused editor captured for this suggestion may be changed.
//! All text stays in memory; permissions are queried, never granted here.

use std::ffi::c_void;
use std::ptr;

use lucid_core::{ReplacementOutcome, Utf16Range};
use objc2::msg_send;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::NSString;

use crate::imk::{is_authentication_host, secure_input_enabled};

type Ref = *const c_void;
const POINT_TYPE: u32 = 1;
const SIZE_TYPE: u32 = 2;
const RECT_TYPE: u32 = 3;
const RANGE_TYPE: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
struct Size {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
struct Rect {
    origin: Point,
    size: Size,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Range {
    location: isize,
    length: isize,
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: Ref) -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> Ref;
    fn AXUIElementCopyAttributeValue(element: Ref, name: Ref, value: *mut Ref) -> i32;
    fn AXUIElementCopyParameterizedAttributeValue(
        element: Ref,
        parameterizedAttribute: Ref,
        parameter: Ref,
        result: *mut Ref,
    ) -> i32;
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
    #[allow(dead_code)]
    fn CFEqual(a: Ref, b: Ref) -> u8;
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFStringGetTypeID() -> usize;
    #[allow(dead_code)]
    fn CFBooleanGetTypeID() -> usize;
    #[allow(dead_code)]
    fn CFBooleanGetValue(boolean: Ref) -> u8;
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
    #[allow(dead_code)]
    fn boolean(&self) -> Option<bool> {
        if unsafe { CFGetTypeID(self.0) != CFBooleanGetTypeID() } {
            return None;
        }
        Some(unsafe { CFBooleanGetValue(self.0) != 0 })
    }
    fn range(&self) -> Option<Range> {
        if unsafe { CFGetTypeID(self.0) != AXValueGetTypeID() } {
            return None;
        }
        let mut range = Range::default();
        let ok = unsafe { AXValueGetValue(self.0, RANGE_TYPE, (&mut range as *mut Range).cast()) };
        (ok != 0 && range.location >= 0 && range.length >= 0).then_some(range)
    }
    fn point(&self) -> Option<objc2_foundation::NSPoint> {
        if unsafe { CFGetTypeID(self.0) != AXValueGetTypeID() } {
            return None;
        }
        let mut point = Point::default();
        let ok = unsafe { AXValueGetValue(self.0, POINT_TYPE, (&mut point as *mut Point).cast()) };
        (ok != 0).then(|| objc2_foundation::NSPoint::new(point.x, point.y))
    }
    fn size(&self) -> Option<objc2_foundation::NSSize> {
        if unsafe { CFGetTypeID(self.0) != AXValueGetTypeID() } {
            return None;
        }
        let mut size = Size::default();
        let ok = unsafe { AXValueGetValue(self.0, SIZE_TYPE, (&mut size as *mut Size).cast()) };
        (ok != 0 && size.width > 0.0 && size.height > 0.0)
            .then(|| objc2_foundation::NSSize::new(size.width, size.height))
    }
    fn rect(&self) -> Option<objc2_foundation::NSRect> {
        if unsafe { CFGetTypeID(self.0) != AXValueGetTypeID() } {
            return None;
        }
        let mut rect = Rect::default();
        let ok = unsafe { AXValueGetValue(self.0, RECT_TYPE, (&mut rect as *mut Rect).cast()) };
        (ok != 0 && (rect.size.width > 0.0 || rect.size.height > 0.0)).then(|| {
            objc2_foundation::NSRect::new(
                objc2_foundation::NSPoint::new(rect.origin.x, rect.origin.y),
                objc2_foundation::NSSize::new(rect.size.width, rect.size.height),
            )
        })
    }
}
unsafe impl Send for Owned {}
unsafe impl Sync for Owned {}

impl Clone for Owned {
    fn clone(&self) -> Self {
        if !self.0.is_null() {
            unsafe { CFRetain(self.0) };
        }
        Owned(self.0)
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

fn parameterized_attribute(element: Ref, name: &str, parameter: Ref) -> Option<Owned> {
    let name = NSString::from_str(name);
    let mut value = ptr::null();
    let error = unsafe {
        AXUIElementCopyParameterizedAttributeValue(element, string_ref(&name), parameter, &mut value)
    };
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
    // 1. 尝试直接获取应用级焦点元素
    if let Some(focused) = attribute(app, "AXFocusedUIElement") {
        return Some(focused);
    }
    // 2. 尝试从应用当前焦点窗口获取焦点元素
    if let Some(window) = attribute(app, "AXFocusedWindow") {
        if let Some(focused) = attribute(window.0, "AXFocusedUIElement") {
            return Some(focused);
        }
    }
    // 3. 遍历所有顶层窗口获取焦点元素
    let windows = children(app);
    for window in &windows {
        if let Some(focused) = attribute(window.0, "AXFocusedUIElement") {
            return Some(focused);
        }
    }
    tracing::info!(windows = windows.len(), "微信未直接提供焦点元素，深度搜索文本控件");
    fn walk(element: Ref, depth: usize, budget: &mut usize) -> Option<Owned> {
        if depth > 8 || *budget == 0 {
            return None;
        }
        *budget -= 1;
        let role = attribute(element, "AXRole").and_then(|v| v.string());
        let is_focused = attribute(element, "AXFocused")
            .and_then(|v| v.string())
            .as_deref()
            == Some("1");
        let is_text_role = matches!(
            role.as_deref(),
            Some("AXTextArea" | "AXTextField" | "AXComboBox" | "AXWebArea" | "AXGroup")
        );
        if is_focused && is_text_role {
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

pub fn check_or_prompt_permission() -> bool {
    unsafe {
        if AXIsProcessTrusted() != 0 {
            return true;
        }
        let key = NSString::from_str("AXTrustedCheckOptionPrompt");
        let value = objc2_foundation::NSNumber::numberWithBool(true);
        let dict: *mut c_void =
            msg_send![objc2::class!(NSDictionary), dictionaryWithObject: &*value, forKey: &*key];
        if dict.is_null() {
            return false;
        }
        AXIsProcessTrustedWithOptions(dict.cast()) != 0
    }
}

#[derive(Clone)]
pub struct Editor {
    #[allow(dead_code)]
    app: Owned,
    element: Owned,
    #[allow(dead_code)]
    host: String,
    pid: i32,
}
unsafe impl Send for Editor {}
unsafe impl Sync for Editor {}

static ACTIVE_EDITOR: std::sync::Mutex<Option<Editor>> = std::sync::Mutex::new(None);

pub fn remember_active_editor(editor: Editor) {
    if let Ok(mut slot) = ACTIVE_EDITOR.lock() {
        *slot = Some(editor);
    }
}

pub fn current_active_editor() -> Option<Editor> {
    if let Ok(slot) = ACTIVE_EDITOR.lock() {
        slot.clone()
    } else {
        None
    }
}

pub fn take_active_editor() -> Option<Editor> {
    if let Ok(mut slot) = ACTIVE_EDITOR.lock() {
        slot.take()
    } else {
        None
    }
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
        let target_app = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .filter(|app| app.bundleIdentifier().map(|v| v.to_string()).as_deref() == Some(host))
            .or_else(|| {
                NSWorkspace::sharedWorkspace()
                    .runningApplications()
                    .iter()
                    .find(|app| {
                        app.bundleIdentifier().map(|v| v.to_string()).as_deref() == Some(host)
                    })
            });
        let Some(target_app) = target_app else {
            tracing::info!(expected = %host, "AX capture 失败：未找到目标应用进程");
            return None;
        };
        let pid = target_app.processIdentifier();
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

    #[allow(dead_code)]
    pub fn pid(&self) -> i32 {
        self.pid
    }

    #[allow(dead_code)]
    pub fn current_value(&self) -> Option<String> {
        attribute(self.element.0, "AXValue").and_then(|v| v.string())
    }

    pub fn select_range(&self, range: Utf16Range) -> bool {
        let Ok(location) = isize::try_from(range.location) else {
            return false;
        };
        let Ok(length) = isize::try_from(range.length) else {
            return false;
        };
        let Some(selected) = range_value(Range { location, length }) else {
            return false;
        };
        if !set_attribute(self.element.0, "AXSelectedTextRange", selected.0) {
            return false;
        }
        // 校验选区是否真实生效
        let current = attribute(self.element.0, "AXSelectedTextRange")
            .and_then(|v| v.range());
        current == Some(Range { location, length })
    }

    pub fn activate_host(&self) {
        let apps = NSWorkspace::sharedWorkspace().runningApplications();
        for app in apps.iter() {
            if app.processIdentifier() == self.pid {
                #[allow(deprecated)]
                app.activateWithOptions(
                    objc2_app_kit::NSApplicationActivationOptions::ActivateIgnoringOtherApps,
                );
                break;
            }
        }
    }

    fn is_valid_target(&self) -> bool {
        if !trusted() || secure_input_enabled() {
            return false;
        }
        let mut element_pid = 0;
        let err = unsafe { AXUIElementGetPid(self.element.0, &mut element_pid) };
        err == 0 && element_pid == self.pid
    }

    /// 从输入框中可靠恢复当前句末标点所结束的完整句子。
    /// 自动优先读取 AXSelectedTextRange，对无效或缺失的光标以输入框全部文本末尾兜底。
    pub fn recover_sentence(&self, hint_caret: Option<usize>) -> Option<(String, Utf16Range)> {
        let value = attribute(self.element.0, "AXValue").and_then(|item| item.string())?;
        let units = value.encode_utf16().collect::<Vec<_>>();
        if units.is_empty() {
            return None;
        }

        // 优先从 AX 自身的 AXSelectedTextRange 属性获取真实光标
        let ax_caret = attribute(self.element.0, "AXSelectedTextRange")
            .and_then(|v| v.range())
            .map(|r| (r.location + r.length) as usize);

        // 确定有效光标位置：
        // 1. AXSelectedTextRange（若在 1..=units.len() 范围内）
        // 2. hint_caret（若在 1..=units.len() 范围内）
        // 3. 兜底取输入框全部文本末尾 units.len()
        let caret = ax_caret
            .filter(|&c| c > 0 && c <= units.len())
            .or_else(|| hint_caret.filter(|&c| c > 0 && c <= units.len()))
            .unwrap_or(units.len());

        let start = caret.saturating_sub(512);
        let slice = &units[start..caret];
        let (sentence, offset_in_slice, length) = super::extract_sentence_from_units(slice)?;
        let range = Utf16Range::new(start + offset_in_slice, length);
        tracing::info!(
            caret,
            total_units = units.len(),
            sentence = %sentence,
            ?range,
            "AX 成功恢复完整句子"
        );
        Some((sentence, range))
    }

    #[allow(dead_code)]
    pub fn value_before_caret(&self, caret: usize) -> Option<String> {
        self.recover_sentence(Some(caret)).map(|(s, _)| s)
    }
}

fn primary_screen_height() -> f64 {
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return 900.0;
    };
    objc2_app_kit::NSScreen::screens(mtm)
        .firstObject()
        .map(|s| s.frame().size.height)
        .unwrap_or(900.0)
}

fn ax_to_cocoa_rect(
    ax_origin: objc2_foundation::NSPoint,
    size: objc2_foundation::NSSize,
) -> objc2_foundation::NSRect {
    let screen_height = primary_screen_height();
    let cocoa_y = screen_height - ax_origin.y - size.height;
    objc2_foundation::NSRect::new(
        objc2_foundation::NSPoint::new(ax_origin.x, cocoa_y),
        size,
    )
}

/// 判定候选矩形是否属于微信右下角聊天输入区域。
/// 杜绝左侧导航栏及会话列表顶部搜索框区域的伪光标。
pub fn is_plausible_wechat_input_rect(
    rect: objc2_foundation::NSRect,
    win_rect: objc2_foundation::NSRect,
) -> bool {
    // 微信最左侧导航图标条固定约 50~60px。聊天输入框位于导航条右侧。
    let min_x = win_rect.origin.x + 55.0;
    let max_x = win_rect.origin.x + win_rect.size.width - 20.0;
    if rect.origin.x < min_x || rect.origin.x > max_x {
        return false;
    }
    // 垂直方向：Cocoa 坐标系下，原点在屏幕底部，Y 向上递增。
    // 微信顶部搜索栏和标题栏 Y 靠近窗口顶部（Y > win_rect.origin.y + win_rect.size.height - 90.0）。
    // 聊天输入框位于窗口下半部分（底部向上 10px ~ 400px 区间内）。
    let min_y = win_rect.origin.y + 10.0;
    let max_y = (win_rect.origin.y + 400.0).min(win_rect.origin.y + win_rect.size.height - 90.0);
    if rect.origin.y < min_y || rect.origin.y > max_y {
        return false;
    }
    true
}

impl Editor {
    /// 获取焦点元素所属窗口在 Cocoa 屏幕坐标系下的完整矩形。
    pub fn window_rect(&self) -> Option<objc2_foundation::NSRect> {
        let win = attribute(self.element.0, "AXWindow")
            .or_else(|| attribute(self.app.0, "AXFocusedWindow"))
            .or_else(|| {
                let wins = children(self.app.0);
                wins.into_iter().next()
            })?;
        let pos = attribute(win.0, "AXPosition")?.point()?;
        let size = attribute(win.0, "AXSize")?.size()?;
        if size.width > 200.0 && size.height > 200.0 {
            Some(ax_to_cocoa_rect(pos, size))
        } else {
            None
        }
    }

    /// 通过 AXBoundsForRange 精确获取指定文字范围在 Cocoa 屏幕坐标系下的物理包围盒。
    pub fn caret_rect_for_range(&self, range: Utf16Range) -> Option<objc2_foundation::NSRect> {
        let Ok(location) = isize::try_from(range.location) else {
            return None;
        };
        let Ok(length) = isize::try_from(range.length) else {
            return None;
        };
        let range_val = range_value(Range { location, length })?;
        let bounds_val = parameterized_attribute(self.element.0, "AXBoundsForRange", range_val.0)?;
        let rect = bounds_val.rect()?;
        tracing::info!(?range, ?rect, "AX 获得文字物理包围盒");
        Some(ax_to_cocoa_rect(rect.origin, rect.size))
    }

    /// 获取焦点输入框在 Cocoa 屏幕坐标系下的包围矩形，用于精准定位建议卡片。
    pub fn element_rect(&self) -> Option<objc2_foundation::NSRect> {
        let pos = attribute(self.element.0, "AXPosition")?.point()?;
        let size = attribute(self.element.0, "AXSize")?.size()?;
        // 过滤超大容器（例如全窗口级别的 AXGroup 或 AXWindow），防止弹窗错位到窗口左上角
        if size.height > 400.0 && size.width > 600.0 {
            tracing::info!(?size, "忽略过大的容器元素，不作为输入框定位矩形");
            return None;
        }
        Some(ax_to_cocoa_rect(pos, size))
    }

    /// 获取建议卡片的最佳视觉锚点矩形。
    /// 针对微信进行几何净化，严禁将弹窗放置在左上角搜索栏或会话列表上。
    pub fn visual_anchor_rect(&self, range: Option<Utf16Range>) -> Option<objc2_foundation::NSRect> {
        let is_wechat = self.host == "com.tencent.xinWeChat";
        let win_rect = self.window_rect();

        // 1. 优先尝试从文字/光标范围获取物理包围盒
        if let Some(r) = range {
            if let Some(caret) = self.caret_rect_for_range(r) {
                if !is_wechat || win_rect.map_or(true, |w| is_plausible_wechat_input_rect(caret, w)) {
                    tracing::info!(?caret, "采用精准文字/光标物理包围盒定位");
                    return Some(caret);
                } else {
                    tracing::warn!(?caret, ?win_rect, "微信文字包围盒未落在聊天输入区域，已剔除");
                }
            }
        }

        // 2. 尝试从输入框元素本身获取物理外框并精确转换为首行文字光标
        if let Some(elem) = self.element_rect() {
            if !is_wechat || win_rect.map_or(true, |w| is_plausible_wechat_input_rect(elem, w)) {
                tracing::info!(?elem, "采用真实输入框外框定位");
                // 输入框首行文字：水平距左内边距约 8px，垂直距顶边向下约 20px
                let text_x = elem.origin.x + 8.0;
                let text_y = elem.origin.y + elem.size.height - 20.0;
                let text_rect = objc2_foundation::NSRect::new(
                    objc2_foundation::NSPoint::new(text_x, text_y),
                    objc2_foundation::NSSize::new(20.0, 18.0),
                );
                return Some(text_rect);
            } else {
                tracing::warn!(?elem, ?win_rect, "微信元素外框未落在聊天输入区域，已剔除");
            }
        }

        // 3. 若微信返回的元素位于虚假区域，但窗口坐标真实，
        // 根据微信标准三栏工业布局推导右下角聊天输入区首行文字光标（精准对齐笑脸图标上方的文字起点）
        if is_wechat {
            if let Some(w) = win_rect {
                // 导航条(~55px) + 会话列表通常在窗口左侧约 160~200px 处，文字起点紧跟其后约 215px
                let text_x = (w.origin.x + 215.0).min(w.origin.x + w.size.width - 240.0);
                // 输入框文字通常在窗口底部向上约 160px 处，确保弹窗完全收纳在微信输入区内不露桌面壁纸
                let text_y = w.origin.y + 160.0;
                let simulated_caret = objc2_foundation::NSRect::new(
                    objc2_foundation::NSPoint::new(text_x, text_y),
                    objc2_foundation::NSSize::new(20.0, 18.0),
                );
                tracing::info!(?simulated_caret, ?w, "微信主窗口输入区几何推导定位生效");
                return Some(simulated_caret);
            }
        }

        None
    }

    pub fn replace(
        &self,
        original: &str,
        replacement: &str,
        range: Option<Utf16Range>,
    ) -> ReplacementOutcome {
        if !self.is_valid_target() {
            tracing::info!("AX 替换失败：目标应用进程已无效或处于安全输入状态");
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

        // 直接尝试通过 AXValue 进行原子替换（不预设 settable 限制，以实际写入结果为准）
        if attribute(self.element.0, "AXValue")
            .and_then(|v| v.string())
            .as_deref()
            != Some(&before)
        {
            tracing::info!("AX 替换取消：写入前微信内容已变化");
            return ReplacementOutcome::Stale;
        }
        let text = NSString::from_str(&expected);
        let accepted = set_attribute(self.element.0, "AXValue", string_ref(&text));
        let after = attribute(self.element.0, "AXValue").and_then(|v| v.string());
        tracing::info!(
            accepted,
            verified = after.as_deref() == Some(&expected),
            "AXValue 写入结果"
        );
        if accepted && after.as_deref() == Some(&expected) {
            let new_caret = target.location + replacement.encode_utf16().count();
            if let Some(new_range) = range_value(Range {
                location: new_caret as isize,
                length: 0,
            }) {
                let _ = set_attribute(self.element.0, "AXSelectedTextRange", new_range.0);
            }
            self.activate_host();
            return ReplacementOutcome::Replaced;
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
        if !set_attribute(self.element.0, "AXSelectedTextRange", selected.0) {
            return ReplacementOutcome::Unsupported;
        }
        let verified = attribute(self.element.0, "AXSelectedTextRange").and_then(|v| v.range())
            == Some(Range { location, length })
            && attribute(self.element.0, "AXSelectedText").and_then(|v| v.string())
                == slice(&before, target)
            && attribute(self.element.0, "AXValue")
                .and_then(|v| v.string())
                .as_deref()
                == Some(&before);
        if !verified {
            set_attribute(self.element.0, "AXSelectedTextRange", previous.0);
            return ReplacementOutcome::Stale;
        }
        let text = NSString::from_str(replacement);
        let accepted = set_attribute(self.element.0, "AXSelectedText", string_ref(&text));
        let after = attribute(self.element.0, "AXValue").and_then(|v| v.string());
        if accepted && after.as_deref() == Some(&expected) {
            self.activate_host();
            ReplacementOutcome::Replaced
        } else {
            if after.as_deref() == Some(&before) {
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

#[allow(dead_code)]
pub fn wechat_pid() -> Option<i32> {
    NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .find(|app| {
            app.bundleIdentifier()
                .map(|v| v.to_string())
                .as_deref()
                == Some("com.tencent.xinWeChat")
        })
        .map(|app| app.processIdentifier())
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

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

    #[test]
    fn test_wechat_input_rect_rejection_and_acceptance() {
        // 假设微信主窗口位于屏幕 (100.0, 100.0)，尺寸为 1000x700
        let win = NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(1000.0, 700.0));

        // 1. 虚假坐标：会话列表顶部搜索框（位于左侧 X=168，垂直 Y=750 靠近窗口顶部）
        let search_box_caret = NSRect::new(NSPoint::new(168.0, 750.0), NSSize::new(2.0, 18.0));
        assert!(
            !is_plausible_wechat_input_rect(search_box_caret, win),
            "搜索框或窗口左上角伪光标必须被坚决拒绝"
        );

        // 2. 虚假坐标：导航条（X=130，小于 155）或会话列表上半部（Y=620，超出下半部）
        let nav_caret = NSRect::new(NSPoint::new(130.0, 300.0), NSSize::new(20.0, 20.0));
        assert!(
            !is_plausible_wechat_input_rect(nav_caret, win),
            "最左侧导航栏图标伪坐标必须被拒绝"
        );
        let chat_item = NSRect::new(NSPoint::new(180.0, 620.0), NSSize::new(200.0, 50.0));
        assert!(
            !is_plausible_wechat_input_rect(chat_item, win),
            "会话列表中上部列表项必须被拒绝"
        );

        // 3. 真实坐标：右下角输入框内首行文字（X=280，垂直 Y=215 在窗口底部上方 115px 处）
        let real_input_caret = NSRect::new(NSPoint::new(280.0, 215.0), NSSize::new(20.0, 18.0));
        assert!(
            is_plausible_wechat_input_rect(real_input_caret, win),
            "右下角聊天输入区真实文字光标必须被采纳"
        );

        // 4. 真实坐标：整个输入框外框（X=380，垂直 Y=140 在窗口底部上方 40px~150px 处）
        let real_input_box = NSRect::new(NSPoint::new(380.0, 140.0), NSSize::new(600.0, 100.0));
        assert!(
            is_plausible_wechat_input_rect(real_input_box, win),
            "右下角聊天输入框整体外框必须被采纳"
        );
    }
}
