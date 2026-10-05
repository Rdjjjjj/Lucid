//! 输入法进程里的请求调度。网络不在按键回调里等待。

use std::cell::RefCell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadOnly, define_class, msg_send, sel};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSTimer};

use lucid_core::ai::HttpCorrectionService;
use lucid_core::{CorrectionRequest, Utf16Range};

use crate::app::settings::Settings;
use crate::suggestion::SuggestionPanel;

mod accessibility;

static ACTIVE_REQUEST: AtomicU64 = AtomicU64::new(0);
thread_local! {
    static CURRENT_CLIENT: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    // IMK may hand us a fresh proxy object for the same text client when a
    // panel becomes key. Pointer equality is therefore not a reliable way to
    // detect an app switch; the bundle id remains stable across those proxies.
    static CURRENT_HOST_ID: RefCell<Option<String>> = const { RefCell::new(None) };
    static CURRENT_HOST_PID: RefCell<Option<i32>> = const { RefCell::new(None) };
}
static PENDING_RANGE: Mutex<Option<lucid_core::Utf16Range>> = Mutex::new(None);
static PAUSE_FLUSH: Mutex<Option<fn()>> = Mutex::new(None);
static SHARED_SESSION: Mutex<Option<crate::imk::InputSession>> = Mutex::new(None);

#[allow(dead_code)]
pub fn start_pause_timer() {}

pub fn request_correction(sentence: String, request_id: u64, host: String) {
    if crate::imk::is_authentication_host(&host) || crate::imk::secure_input_enabled() {
        reset_active_context();
        SuggestionPanel::hide();
        return;
    }
    ACTIVE_REQUEST.store(request_id, Ordering::SeqCst);
    tracing::info!(host = %host, chars = sentence.chars().count(), request_id, "开始 AI 改写");
    let settings = Settings::shared();
    let Some(configuration) = settings.configuration() else {
        SuggestionPanel::show_status(
            "还没配置 AI。请打开 Lucid 填写服务地址和 Key，获取模型列表后选择一个模型。",
        );
        return;
    };
    let Some(api_key) = settings.api_key() else {
        SuggestionPanel::show_status(
            "读取不了 API Key，原文已保留。请打开 Lucid，重新填写 Key 并保存。",
        );
        return;
    };
    SuggestionPanel::show_status("正在改写成英文…");
    let service = match HttpCorrectionService::new(configuration, api_key) {
        Ok(service) => service,
        Err(error) => {
            SuggestionPanel::show_status(&format!("改写失败：{error}"));
            return;
        }
    };
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(_) => return,
        };
        let result = runtime.block_on(service.correct(&CorrectionRequest::new(sentence.clone())));
        if ACTIVE_REQUEST.load(Ordering::SeqCst) != request_id {
            tracing::info!(host = %host, "丢弃过期的改写结果");
            return;
        }
        match result {
            Ok(result) => {
                tracing::info!(host = %host, chars = result.corrected_text.chars().count(), "AI 改写完成");
                let rewritten = result.corrected_text.trim().to_owned();
                if rewritten.is_empty() {
                    SuggestionPanel::show_status("模型没有返回英文句子，原文已保留。");
                } else if rewritten == sentence {
                    SuggestionPanel::hide();
                } else {
                    SuggestionPanel::show_suggestion(&sentence, &rewritten);
                }
            }
            Err(error) => {
                tracing::error!(host = %host, error = %error, "AI 改写失败");
                SuggestionPanel::show_status(&format!("改写失败：{error}"));
            }
        }
    });
}

#[allow(dead_code)]
pub fn replacement_range(original: &str, selected: Option<Utf16Range>) -> Option<Utf16Range> {
    lucid_core::replacement::range_behind_caret(original.encode_utf16().count(), selected)
}

/// 丢弃上一个文本框的会话和异步请求，避免把 A 应用里的原文拿去替换 B 应用。
pub fn reset_active_context() {
    ACTIVE_REQUEST.fetch_add(1, Ordering::SeqCst);
    accessibility::take_active_editor();
    if let Ok(mut slot) = SHARED_SESSION.lock() {
        *slot = None;
    }
    if let Ok(mut slot) = PENDING_RANGE.lock() {
        *slot = None;
    }
    CURRENT_CLIENT.with(|slot| {
        *slot.borrow_mut() = None;
    });
    CURRENT_HOST_ID.with(|slot| {
        *slot.borrow_mut() = None;
    });
    CURRENT_HOST_PID.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

/// IMK may deactivate and immediately reactivate the same text client after a
/// committed character. That is not an app switch and must not cancel the
/// request that is already being generated for the sentence just typed. Only a
/// genuinely different client invalidates the old request.
pub fn activate_client(client: &objc2::runtime::AnyObject) {
    // IMK sometimes calls activateServer: with the input method's own server
    // client immediately after a printable character. That callback is not an
    // app switch. Treating it as one invalidates the in-flight AI request, so
    // the eventual result is logged as stale and no suggestion is shown.
    let bundle_id = crate::imk::TextClient::new(client).bundle_identifier();
    if bundle_id == "io.github.rdj.inputmethod.lucid" || bundle_id == "io.github.rdj.lucid" {
        tracing::debug!(host = %bundle_id, "忽略输入法自身的 activateServer 回调");
        return;
    }

    let effective_host = if bundle_id != "unknown" {
        bundle_id
    } else {
        objc2_app_kit::NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .and_then(|app| app.bundleIdentifier().map(|v| v.to_string()))
            .unwrap_or_else(|| "unknown".to_owned())
    };

    let same_host =
        CURRENT_HOST_ID.with(|slot| slot.borrow().as_deref() == Some(effective_host.as_str()));
    if !same_host {
        reset_active_context();
        SuggestionPanel::hide();
    }
    remember_client(client, None);
}

pub fn remember_client(client: &objc2::runtime::AnyObject, range: Option<lucid_core::Utf16Range>) {
    let pointer = client as *const objc2::runtime::AnyObject as *mut objc2::runtime::AnyObject;
    let retained = unsafe { Retained::retain(pointer) };
    let client_host = crate::imk::TextClient::new(client).bundle_identifier();
    let (host_id, host_pid) = if client_host != "unknown" {
        let pid = objc2_app_kit::NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .find(|app| {
                app.bundleIdentifier()
                    .map(|v| v.to_string())
                    .as_deref()
                    == Some(&client_host)
            })
            .map(|app| app.processIdentifier());
        (client_host, pid)
    } else {
        let front = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication();
        let bid = front
            .as_ref()
            .and_then(|app| app.bundleIdentifier().map(|v| v.to_string()))
            .unwrap_or_else(|| "unknown".to_owned());
        let pid = front.as_ref().map(|app| app.processIdentifier());
        (bid, pid)
    };
    CURRENT_CLIENT.with(|slot| {
        *slot.borrow_mut() = retained;
    });
    CURRENT_HOST_ID.with(|slot| {
        *slot.borrow_mut() = Some(host_id);
    });
    CURRENT_HOST_PID.with(|slot| {
        *slot.borrow_mut() = host_pid;
    });
    if let Some(range) = range {
        if let Ok(mut slot) = PENDING_RANGE.lock() {
            *slot = Some(range);
        }
    }
}

pub fn apply_current_suggestion() {
    tracing::info!("用户点击使用英文");
    let Some((original, replacement)) = SuggestionPanel::take_current() else {
        tracing::warn!("点击使用英文时没有可用的建议");
        return;
    };
    SuggestionPanel::hide();
    activate_host();

    let range = PENDING_RANGE.lock().ok().and_then(|slot| *slot);
    let _ = DispatchQueue::main().after(DispatchTime::NOW.time(100_000_000), move || {
        apply_saved_suggestion(original, replacement, range);
    });
}

fn activate_host() {
    let host_id = CURRENT_HOST_ID.with(|slot| slot.borrow().clone());
    let host_pid = CURRENT_HOST_PID.with(|slot| slot.borrow().clone());
    let apps = objc2_app_kit::NSWorkspace::sharedWorkspace().runningApplications();
    for app in apps.iter() {
        let matches_pid = host_pid.is_some_and(|p| app.processIdentifier() == p);
        let matches_id = host_id.as_deref().is_some_and(|id| {
            id != "unknown" && app.bundleIdentifier().map(|v| v.to_string()).as_deref() == Some(id)
        });
        if matches_pid || matches_id {
            #[allow(deprecated)]
            app.activateWithOptions(
                objc2_app_kit::NSApplicationActivationOptions::ActivateIgnoringOtherApps,
            );
            return;
        }
    }
    // 兜底：如果微信正在运行且当前是微信输入，激活微信
    for app in apps.iter() {
        if app.bundleIdentifier().map(|v| v.to_string()).as_deref() == Some("com.tencent.xinWeChat") {
            #[allow(deprecated)]
            app.activateWithOptions(
                objc2_app_kit::NSApplicationActivationOptions::ActivateIgnoringOtherApps,
            );
            return;
        }
    }
}

fn apply_saved_suggestion(
    original: String,
    replacement: String,
    range: Option<lucid_core::Utf16Range>,
) {
    let client = CURRENT_CLIENT.with(|slot| slot.borrow().clone());
    let Some(client) = client else {
        tracing::warn!("替换失败：没有找到当前输入框");
        SuggestionPanel::show_status("没有找到当前输入框，原文已保留。");
        return;
    };
    let text_client = crate::imk::TextClient::new(&client);
    let host_id = CURRENT_HOST_ID
        .with(|slot| slot.borrow().clone())
        .unwrap_or_default();
    let text_client_host = text_client.bundle_identifier();
    let front_id = objc2_app_kit::NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .and_then(|app| app.bundleIdentifier().map(|v| v.to_string()))
        .unwrap_or_default();
    let is_wechat = host_id == "com.tencent.xinWeChat"
        || text_client_host == "com.tencent.xinWeChat"
        || front_id == "com.tencent.xinWeChat";
    let outcome = if is_wechat {
        replace_in_wechat(&text_client, &original, &replacement, range)
    } else {
        replace_with_client(&text_client, &original, &replacement, range)
    };
    tracing::info!(?outcome, is_wechat, "替换结果");
    match outcome {
        lucid_core::ReplacementOutcome::Replaced => {
            tracing::info!("已替换");
        }
        lucid_core::ReplacementOutcome::Stale => {
            SuggestionPanel::show_status("原文已经被修改，所以没有替换。");
        }
        lucid_core::ReplacementOutcome::Unverified => {
            SuggestionPanel::show_status(
                "替换结果无法确认，Lucid 已停止继续写入，请检查原文后重试。",
            );
        }
        lucid_core::ReplacementOutcome::Unsupported => {
            // The platform-specific path already explains missing permissions
            // or focus. Keep this branch silent so it cannot overwrite the
            // more useful message with a generic retry prompt.
        }
    }
}

pub fn keep_original() {
    tracing::info!("用户保留原文");
    // Invalidate the network result and clear the saved range immediately.
    // A late callback must not resurrect this card or write into the editor.
    ACTIVE_REQUEST.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut slot) = SHARED_SESSION.lock() {
        if let Some(session) = slot.as_mut() {
            session.clear_pending();
        }
    }
    remember_range(None);
    SuggestionPanel::hide();
}

/// Finds a screen-space anchor for the suggestion card. NSTextInputClient owns
/// the real caret geometry, so using it keeps the card next to the sentence in
/// Notes, WeChat, browsers, and other hosts instead of pinning it to a screen
/// corner. The call happens on the main queue after the network request, never
fn is_plausible_caret_rect(rect: NSRect) -> bool {
    rect.size.width.is_finite()
        && rect.size.height.is_finite()
        && rect.origin.x.is_finite()
        && rect.origin.y.is_finite()
        && rect.size.height >= 8.0
        && rect.size.height <= 45.0
        && rect.size.width <= 150.0
}

/// inside a key callback.
pub fn suggestion_origin(width: f64, height: f64) -> Option<NSPoint> {
    let client = CURRENT_CLIENT.with(|slot| slot.borrow().clone())?;
    let text_client = crate::imk::TextClient::new(&client);
    let host = text_client.bundle_identifier();
    let range = PENDING_RANGE.lock().ok().and_then(|slot| *slot);
    let is_wechat = host == "com.tencent.xinWeChat";

    // 1. 对于微信，坚决不信任其 IMK 暴露的伪光标（微信在无 markedText 时返回左上角搜索栏伪坐标）；
    // 对于其他原生或标准宿主（如备忘录 Notes、Safari 等），优先采用 IMK 光标矩形。
    let imk_caret = if is_wechat {
        None
    } else {
        range.and_then(|range| {
            let caret = Utf16Range::new(range.end(), 0);
            text_client
                .first_rect_for_character_range(caret)
                .filter(|r| is_plausible_caret_rect(*r))
                .or_else(|| {
                    text_client
                        .line_height_rect_for_character(caret.location)
                        .filter(|r| is_plausible_caret_rect(*r))
                })
                .or_else(|| {
                    if range.length > 0 {
                        let index = range.end().saturating_sub(1);
                        text_client
                            .first_rect_for_character_range(Utf16Range::new(index, 1))
                            .filter(|r| is_plausible_caret_rect(*r))
                    } else {
                        None
                    }
                })
        })
    };

    // 2. 结合辅助功能 AX 精准定位（含微信三栏几何净化与右下角输入区推导）
    let caret_rect = imk_caret.or_else(|| {
        if accessibility::trusted() {
            let active = accessibility::current_active_editor()
                .or_else(|| accessibility::Editor::capture(&host));
            if let Some(editor) = active {
                editor.visual_anchor_rect(range)
            } else if is_wechat {
                accessibility::Editor::capture(&host).and_then(|e| e.visual_anchor_rect(range))
            } else {
                None
            }
        } else {
            None
        }
    });

    if let Some(caret_rect) = caret_rect {
        let origin = panel_origin_near_rect(caret_rect, width, height);
        tracing::info!(?caret_rect, ?origin, "建议卡片定位到目标矩形");
        return Some(origin);
    }

    // 3. 兜底定位：放在鼠标所在屏幕的下方偏居中位置
    let mtm = objc2::MainThreadMarker::new()?;
    let mouse = objc2_app_kit::NSEvent::mouseLocation();
    let screen = objc2_app_kit::NSScreen::screens(mtm)
        .iter()
        .find(|screen| contains(screen.frame(), mouse))
        .or_else(|| objc2_app_kit::NSScreen::mainScreen(mtm))?;
    let visible = screen.visibleFrame();
    let x = (visible.origin.x + (visible.size.width - width) / 2.0).clamp(
        visible.origin.x + 12.0,
        visible.origin.x + visible.size.width - width - 12.0,
    );
    let y = visible.origin.y + 160.0;
    Some(NSPoint::new(x, y))
}

fn contains(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.y >= rect.origin.y
        && point.x <= rect.origin.x + rect.size.width
        && point.y <= rect.origin.y + rect.size.height
}

fn panel_origin_near_rect(rect: NSRect, width: f64, height: f64) -> NSPoint {
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return NSPoint::new(rect.origin.x, rect.origin.y - height - 6.0);
    };
    let mouse = objc2_app_kit::NSEvent::mouseLocation();
    let screen = objc2_app_kit::NSScreen::screens(mtm)
        .iter()
        .find(|screen| {
            let frame = screen.frame();
            contains(frame, rect.origin) || contains(frame, mouse)
        })
        .or_else(|| objc2_app_kit::NSScreen::mainScreen(mtm));
    let visible = screen
        .as_ref()
        .map(|screen| screen.visibleFrame())
        .unwrap_or(NSRect::new(
            NSPoint::new(80.0, 80.0),
            NSSize::new(800.0, 600.0),
        ));
    let margin = 10.0;

    // 水平位置：卡片左侧对齐正在输入的文字起始位并微偏右（约4px），
    // 紧紧托住输入文字，与用户图1期望效果完全一致。
    let target_x = rect.origin.x + 4.0;
    let max_x = (visible.origin.x + visible.size.width - width - margin).max(visible.origin.x + margin);
    let x = target_x.clamp(visible.origin.x + margin, max_x);

    // 垂直位置：
    // 若 rect 是单行文字光标（height <= 45.0）：
    // 紧贴在文字/光标的正下方 4 像素处！完全符合图1贴合效果
    let is_single_line_caret = rect.size.height <= 45.0;
    let (below, above) = if is_single_line_caret {
        (
            rect.origin.y - height - 4.0,
            rect.origin.y + rect.size.height + 4.0,
        )
    } else {
        (
            rect.origin.y + rect.size.height - 22.0 - height - 4.0,
            rect.origin.y + rect.size.height + 4.0,
        )
    };

    let bottom_limit = visible.origin.y + margin;
    let top_limit = visible.origin.y + visible.size.height - height - margin;

    let y = if below >= bottom_limit {
        below
    } else if above <= top_limit {
        above
    } else {
        below.max(bottom_limit)
    };

    NSPoint::new(x, y)
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceCreate(state_id: i32) -> *mut std::ffi::c_void;
    fn CGEventCreateKeyboardEvent(
        source: *const std::ffi::c_void,
        virtual_key: u16,
        key_down: bool,
    ) -> *mut std::ffi::c_void;
    fn CGEventSetFlags(event: *mut std::ffi::c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut std::ffi::c_void);
    #[allow(dead_code)]
    fn CGEventPostToPid(pid: i32, event: *mut std::ffi::c_void);
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const std::ffi::c_void);
}

const VK_A: u16 = 0;
const VK_V: u16 = 9;
const VK_COMMAND: u16 = 55;
const FLAG_COMMAND: u64 = 1 << 20;

fn post_key_with_command(keycode: u16) {
    unsafe {
        let source = CGEventSourceCreate(0 /* CombinedSessionState */);

        // 1. Command 键按下
        let cmd_down = CGEventCreateKeyboardEvent(source, VK_COMMAND, true);
        if !cmd_down.is_null() {
            CGEventSetFlags(cmd_down, FLAG_COMMAND);
            CGEventPost(0 /* kCGHIDEventTap */, cmd_down);
            CFRelease(cmd_down.cast());
        }
        std::thread::sleep(std::time::Duration::from_millis(15));

        // 2. 目标按键按下（附带 Command 修饰符）
        let key_down = CGEventCreateKeyboardEvent(source, keycode, true);
        if !key_down.is_null() {
            CGEventSetFlags(key_down, FLAG_COMMAND);
            CGEventPost(0 /* kCGHIDEventTap */, key_down);
            CFRelease(key_down.cast());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));

        // 3. 目标按键抬起（附带 Command 修饰符）
        let key_up = CGEventCreateKeyboardEvent(source, keycode, false);
        if !key_up.is_null() {
            CGEventSetFlags(key_up, FLAG_COMMAND);
            CGEventPost(0 /* kCGHIDEventTap */, key_up);
            CFRelease(key_up.cast());
        }
        std::thread::sleep(std::time::Duration::from_millis(15));

        // 4. Command 键抬起
        let cmd_up = CGEventCreateKeyboardEvent(source, VK_COMMAND, false);
        if !cmd_up.is_null() {
            CGEventSetFlags(cmd_up, 0);
            CGEventPost(0 /* kCGHIDEventTap */, cmd_up);
            CFRelease(cmd_up.cast());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));

        if !source.is_null() {
            CFRelease(source.cast());
        }
    }
}

fn post_select_all() {
    post_key_with_command(VK_A);
}

fn post_paste() {
    post_key_with_command(VK_V);
}

fn set_pasteboard_string(text: &str) -> Option<String> {
    unsafe {
        let pboard: *mut objc2::runtime::AnyObject =
            msg_send![objc2::class!(NSPasteboard), generalPasteboard];
        if pboard.is_null() {
            return None;
        }
        let type_string: *mut objc2::runtime::AnyObject =
            msg_send![objc2::class!(NSString), stringWithUTF8String: c"public.utf8-plain-text".as_ptr()];
        let old_str: Option<objc2::rc::Retained<objc2_foundation::NSString>> =
            msg_send![pboard, stringForType: type_string];
        let old_text = old_str.map(|s| s.to_string());

        let _: () = msg_send![pboard, clearContents];
        let new_str = objc2_foundation::NSString::from_str(text);
        let _: bool = msg_send![pboard, setString: &*new_str, forType: type_string];
        old_text
    }
}

fn restore_pasteboard_string(old_text: Option<String>) {
    if let Some(text) = old_text {
        unsafe {
            let pboard: *mut objc2::runtime::AnyObject =
                msg_send![objc2::class!(NSPasteboard), generalPasteboard];
            if !pboard.is_null() {
                let type_string: *mut objc2::runtime::AnyObject =
                    msg_send![objc2::class!(NSString), stringWithUTF8String: c"public.utf8-plain-text".as_ptr()];
                let _: () = msg_send![pboard, clearContents];
                let new_str = objc2_foundation::NSString::from_str(&text);
                let _: bool = msg_send![pboard, setString: &*new_str, forType: type_string];
            }
        }
    }
}

/// WeChat's IMK proxy reports a selection but ignores the replacement range
/// passed to `insertText:replacementRange:`.  Sending the replacement directly
/// therefore appends `Hello.` after `nihao.`.  Select the matched sentence and
/// commit over that selection; if the proxy only accepts editing commands,
/// delete exactly that sentence and insert at the verified caret.
fn replace_in_wechat(
    _client: &crate::imk::TextClient<'_>,
    original: &str,
    replacement: &str,
    range: Option<lucid_core::Utf16Range>,
) -> lucid_core::ReplacementOutcome {
    // 0. 检查辅助功能权限：如果未开启，主动唤起系统权限弹窗并提示用户，绝不强行追加
    if !accessibility::trusted() {
        tracing::warn!("微信替换需要辅助功能权限，尝试唤起系统授权");
        accessibility::check_or_prompt_permission();
        SuggestionPanel::show_status("微信替换需要辅助功能权限，请在弹出的系统设置中开启。");
        return lucid_core::ReplacementOutcome::Unsupported;
    }

    // 1. 优先复用句末读回时精确捕获的微信输入框进行 AX 原生替换
    let editor = accessibility::take_active_editor()
        .or_else(|| accessibility::Editor::capture("com.tencent.xinWeChat"));
    if let Some(editor) = editor.as_ref() {
        let ax_outcome = editor.replace(original, replacement, range);
        if ax_outcome == lucid_core::ReplacementOutcome::Replaced {
            return ax_outcome;
        }
        tracing::info!(?ax_outcome, "微信 AX 原生写入未完成，转入选区直接覆盖替换流程");
    }

    // 2. 将翻译文本写入系统剪贴板，并备份旧剪贴板内容
    let old_clipboard = set_pasteboard_string(replacement);

    // 等待微信窗口焦点完全稳定
    std::thread::sleep(std::time::Duration::from_millis(50));

    // 3. 直接覆盖（彻底废弃逐字退格，绝不再发生漏删首字母的 bug）：
    // 分支 A：若拥有捕获的 editor 且有确定有效范围，通过 AX 精确高亮选中原文范围，接着直接粘贴覆盖！
    let mut covered = false;
    if let (Some(editor), Some(target_range)) = (editor.as_ref(), range) {
        if editor.select_range(target_range) {
            tracing::info!(?target_range, "微信通过 AX 成功精确高亮选中原文范围，直接覆盖粘贴");
            std::thread::sleep(std::time::Duration::from_millis(30));
            post_paste();
            covered = true;
        }
    }

    // 分支 B：若精确选区未生效，直接全选整个输入框（Cmd+A），接着粘贴覆盖（Cmd+V）！
    if !covered {
        tracing::info!(original, "微信执行 Cmd+A 全选直接覆盖粘贴");
        post_select_all();
        std::thread::sleep(std::time::Duration::from_millis(30));
        post_paste();
    }

    // 4. 500ms 后在后台异步将旧剪贴板内容无缝还原，用户完全无感
    let _ = DispatchQueue::main().after(DispatchTime::NOW.time(500_000_000), move || {
        restore_pasteboard_string(old_clipboard);
    });

    tracing::info!(replacement, "微信直接覆盖替换完成");
    lucid_core::ReplacementOutcome::Replaced
}

/// Replace a committed range by selecting it in the IMK client and committing
/// the English at the active selection.  WeChat ignores the explicit
/// `replacementRange` argument, but its editor selection is still authoritative
/// when this call is made while the IMK connection is focused.
#[allow(dead_code)]
fn try_replace_by_active_selection(
    client: &crate::imk::TextClient<'_>,
    target: lucid_core::Utf16Range,
    replacement: &str,
) -> bool {
    let before = client.substring(target);
    tracing::info!(?target, ?before, "微信尝试活动选区替换");
    if before.is_none() || !client.set_selected_range(target) {
        tracing::info!(?target, "微信活动选区替换失败：无法选中原文");
        return false;
    }
    let Some(selected) = client.selected_range() else {
        tracing::info!(?target, "微信活动选区替换失败：无法读取选区");
        return false;
    };
    if selected != target {
        tracing::info!(?target, ?selected, "微信活动选区替换失败：选区未生效");
        return false;
    }

    client.insert_text(replacement, None);
    let expected = lucid_core::Utf16Range::new(target.location, replacement.encode_utf16().count());
    let after = client.substring(expected);
    let selected_after = client.selected_range();
    let replaced = after.as_deref() == Some(replacement)
        && selected_after == Some(lucid_core::Utf16Range::new(expected.end(), 0));
    tracing::info!(
        ?target,
        ?expected,
        ?after,
        ?selected_after,
        replaced,
        "微信活动选区替换结果"
    );
    if replaced {
        return true;
    }

    // If this client appended instead of replacing, remove only the exact copy
    // that this attempt could have created.  If cleanup cannot be verified, stop
    // and leave the result explicitly unverified rather than writing again.
    let appended = lucid_core::Utf16Range::new(target.end(), replacement.encode_utf16().count());
    if client.substring(appended).as_deref() == Some(replacement) {
        if client.set_selected_range(appended) && client.selected_range() == Some(appended) {
            client.insert_text("", None);
            let restored = client.substring(target).as_deref() == before.as_deref()
                && client
                    .substring(appended)
                    .is_none_or(|text| text != replacement);
            tracing::info!(?appended, restored, "微信活动选区替换失败后的追加文本清理");
        }
    }
    false
}

/// Delete the selected original with WeChat's own editing command, then insert
/// once.  A command that does not move the caret is abandoned immediately so
/// the original cannot be partially damaged or duplicated.
#[allow(dead_code)]
fn try_replace_by_verified_delete(
    client: &crate::imk::TextClient<'_>,
    target: lucid_core::Utf16Range,
    replacement: &str,
) -> bool {
    let caret = lucid_core::Utf16Range::new(target.end(), 0);
    if client.selected_range() != Some(caret) && !client.set_selected_range(caret) {
        tracing::info!(?caret, "微信删除替换失败：无法定位到原文末尾");
        return false;
    }
    let Some(selected) = client.selected_range() else {
        return false;
    };
    if selected != caret {
        tracing::info!(?selected, ?caret, "微信删除替换失败：光标没有回到原文末尾");
        return false;
    }

    let before = selected;
    client.delete_selection_or_backward();
    let Some(after) = client.selected_range() else {
        tracing::info!(?before, "微信删除替换失败：删除后无法读取光标");
        return false;
    };
    let remaining = client.substring(target);
    let original_gone = remaining
        .as_deref()
        .is_none_or(|text| text.encode_utf16().count() < target.length);
    let moved_to_start = after == lucid_core::Utf16Range::new(target.location, 0);
    tracing::info!(
        ?before,
        ?after,
        original_gone,
        moved_to_start,
        "微信删除命令结果"
    );
    if !moved_to_start && !original_gone {
        return false;
    }
    if !moved_to_start {
        tracing::info!(?after, ?target, "微信删除替换停止：删除范围不完整");
        return false;
    }

    client.insert_text(replacement, None);
    let expected = lucid_core::Utf16Range::new(target.location, replacement.encode_utf16().count());
    let verified = client.substring(expected).as_deref() == Some(replacement)
        && client.selected_range() == Some(lucid_core::Utf16Range::new(expected.end(), 0));
    tracing::info!(?target, ?expected, verified, "微信删除后插入结果");
    verified
}

#[allow(dead_code)]
fn client_replacement_range(
    client: &crate::imk::TextClient<'_>,
    original: &str,
    saved: Option<lucid_core::Utf16Range>,
) -> Option<lucid_core::Utf16Range> {
    let selected = client
        .selected_range()
        .filter(|range| !(range.location == 0 && range.length == 0));
    let searched =
        lucid_core::replacement::range_searching_backwards(original, selected, 1024, |range| {
            client.substring(range)
        });
    let behind =
        lucid_core::replacement::range_behind_caret(original.encode_utf16().count(), selected);
    lucid_core::replacement::preferred_range(original, &[searched, behind, saved], |range| {
        client.substring(range)
    })
}

fn replace_with_client(
    client: &crate::imk::TextClient<'_>,
    original: &str,
    replacement: &str,
    range: Option<lucid_core::Utf16Range>,
) -> lucid_core::ReplacementOutcome {
    use lucid_core::{CommittedTextReplacement, HostClient};
    let selected = client
        .selected_range()
        .filter(|range| !(range.location == 0 && range.length == 0));
    let searched =
        lucid_core::replacement::range_searching_backwards(original, selected, 1024, |range| {
            client.substring(range)
        });
    let behind =
        lucid_core::replacement::range_behind_caret(original.encode_utf16().count(), selected);
    let range =
        lucid_core::replacement::preferred_range(original, &[searched, behind, range], |range| {
            client.substring(range)
        });
    let Some(range) = range else {
        tracing::warn!("替换失败：没有找到原文范围");
        return lucid_core::ReplacementOutcome::Unverified;
    };
    tracing::info!(?range, ?selected, pending = ?range, "开始替换原文");
    if client.substring(range).as_deref() != Some(original)
        && !CommittedTextReplacement::matches_loosely(client.substring(range).as_deref(), original)
    {
        return lucid_core::ReplacementOutcome::Stale;
    }
    let mut host = HostClient {
        read_text: |range| client.substring(range),
        selected_range: || client.selected_range(),
        set_marked_text: Some(|text: &str, selection, replacement_range| {
            client.set_marked_text(text, selection, replacement_range)
        }),
        marked_range: Some(|| client.marked_range()),
        insert_text: |text: &str, replacement_range| {
            let range = if replacement_range.location == usize::MAX {
                None
            } else {
                Some(replacement_range)
            };
            client.insert_text(text, range);
        },
        delete_backward: Some(|| client.delete_backward()),
        set_selection: Some(|range| {
            let _ = client.set_selected_range(range);
        }),
    };
    CommittedTextReplacement::replace_range(original, replacement, range, &mut host)
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "LucidPauseTimer"]
    #[ivars = ()]
    struct PauseTimer;

    impl PauseTimer {
        #[unsafe(method(tick:))]
        fn tick(&self, _timer: Option<&NSTimer>) {
            flush_paused_sentence();
        }
    }

    unsafe impl NSObjectProtocol for PauseTimer {}
);

pub fn schedule_pause_timer(mtm: objc2::MainThreadMarker) {
    let target = PauseTimer::alloc(mtm).set_ivars(());
    let target: Retained<PauseTimer> = unsafe { msg_send![super(target), init] };
    let _timer = unsafe {
        NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
            0.4,
            &target,
            sel!(tick:),
            None,
            true,
        )
    };
    // 计时器持有 target。这里再留一份，避免提前释放。
    std::mem::forget(target);
}

fn flush_paused_sentence() {
    if let Ok(slot) = PAUSE_FLUSH.lock()
        && let Some(flush) = *slot
    {
        flush();
    }
}

pub fn set_pause_flush(flush: fn()) {
    if let Ok(mut slot) = PAUSE_FLUSH.lock() {
        *slot = Some(flush);
    }
}

pub fn note_inserted(
    text: &str,
    host: &str,
    selected_before_insert: Option<lucid_core::Utf16Range>,
) -> bool {
    // 遇到句末标点（如英文句号、感叹号、问号及中文句末标点）时，优先从输入框读回光标前当前整句。
    // 无论前序文本是用其它输入法输入还是先在别处输入后切回 Lucid，都完整翻译句号前所有内容。
    let is_terminator = text.chars().any(|character| ".!?。？！".contains(character));
    if is_terminator
        && let Some(client) = CURRENT_CLIENT.with(|slot| slot.borrow().clone())
    {
        let text_client = crate::imk::TextClient::new(&client);
        if let Some(recovered) = recover_sentence(&text_client, host, selected_before_insert, text) {
            let chars = recovered.text.chars().count();
            tracing::info!(host, chars, ?recovered.range, "句末读回完整原文，开始请求英文建议");
            if let Ok(mut slot) = SHARED_SESSION.lock() {
                let session = slot.get_or_insert_with(crate::imk::InputSession::new);
                session.remember_original(&recovered.text, Some(recovered.range));
                session.reset_tracker();
            }
            remember_range(Some(recovered.range));
            request_correction(recovered.text, next_shared_request(), host.to_owned());
            return true;
        }
        tracing::info!(?selected_before_insert, "句末读回未成功，改用已跟踪文本");
    }

    let completed = {
        let Ok(mut slot) = SHARED_SESSION.lock() else {
            tracing::error!("无法锁定输入会话");
            return false;
        };
        slot.get_or_insert_with(crate::imk::InputSession::new)
            .note_inserted(text, selected_before_insert)
    };
    tracing::debug!(
        host,
        chars = text.chars().count(),
        completed = completed.len(),
        "记录输入"
    );
    for sentence in completed {
        let range = if let Ok(slot) = SHARED_SESSION.lock() {
            slot.as_ref().map(|session| {
                session.absolute_range(lucid_core::Utf16Range::new(
                    sentence.utf16_range.location,
                    sentence.utf16_range.length,
                ))
            })
        } else {
            None
        };
        if let Ok(mut slot) = SHARED_SESSION.lock() {
            slot.get_or_insert_with(crate::imk::InputSession::new)
                .remember_original(&sentence.text, range);
        }
        remember_range(range);
        tracing::info!(
            host,
            chars = sentence.text.chars().count(),
            "句子完成，开始请求英文建议"
        );
        request_correction(sentence.text, next_shared_request(), host.to_owned());
        return true;
    }
    false
}

struct RecoveredSentence {
    text: String,
    range: lucid_core::Utf16Range,
}

/// 尝试恢复光标前的完整句子。对于微信优先走经过适配的 AX，对于其他应用优先走 IMK 文本读取。
fn recover_sentence(
    client: &crate::imk::TextClient<'_>,
    host: &str,
    selected_before_insert: Option<lucid_core::Utf16Range>,
    inserted: &str,
) -> Option<RecoveredSentence> {
    // 微信优先使用 AX 读回完整原文
    if host == "com.tencent.xinWeChat" {
        if accessibility::trusted() {
            if let Some(editor) = accessibility::Editor::capture(host) {
                let hint_caret = selected_before_insert
                    .filter(|range| !(range.location == 0 && range.length == 0))
                    .map(|range| range.location.saturating_add(inserted.encode_utf16().count()));
                if let Some((text, range)) = editor.recover_sentence(hint_caret) {
                    tracing::info!(host, chars = text.chars().count(), ?range, "微信 AX 读回完整原文");
                    accessibility::remember_active_editor(editor);
                    return Some(RecoveredSentence { text, range });
                }
            }
        }
    }

    // 标准 NSTextInputClient 路径（支持备忘录 Notes、TextEdit、Safari、Pages 等绝大多数标准应用）
    if let Some(sentence) = sentence_before_caret(client, selected_before_insert, inserted) {
        return Some(sentence);
    }

    // 非微信宿主若 IMK substring 无法读取，且拥有 AX 权限，再尝试 AX 捕获
    if host != "com.tencent.xinWeChat" && accessibility::trusted() {
        if let Some(editor) = accessibility::Editor::capture(host) {
            let hint_caret = selected_before_insert
                .filter(|range| !(range.location == 0 && range.length == 0))
                .map(|range| range.location.saturating_add(inserted.encode_utf16().count()));
            if let Some((text, range)) = editor.recover_sentence(hint_caret) {
                tracing::info!(host, chars = text.chars().count(), ?range, "通用 AX 读回完整原文");
                accessibility::remember_active_editor(editor);
                return Some(RecoveredSentence { text, range });
            }
        }
    }

    None
}

/// 安全读取光标前的文本，遇到越界返回 nil 的宿主（如微信）自动通过倍增与二分探测其真实有效范围，绝不引发越界错误。
fn safe_read_text_before_caret(
    client: &crate::imk::TextClient<'_>,
    caret: Option<usize>,
) -> Option<(String, usize)> {
    // 如果光标已知且非0，先尝试常规单次读取
    if let Some(caret) = caret.filter(|&c| c > 0) {
        let start = caret.saturating_sub(512);
        let window = lucid_core::Utf16Range::new(start, caret - start);
        if let Some(text) = client.substring(window) {
            if !text.is_empty() {
                return Some((text, caret));
            }
        }
    }

    // 探测模式：从文档开头安全探测有效文本范围（专门适配微信等无法自动截断越界请求的宿主）
    // 1. 先验证 (0, 1) 是否有效
    let first = client.substring(lucid_core::Utf16Range::new(0, 1))?;
    if first.is_empty() {
        return None;
    }

    let mut low = 1usize;
    let mut current_text = first;
    let mut high = None;

    // 倍增探测上限（2, 4, 8, 16, 32, 64, 128, 256, 512）
    for &target in &[2, 4, 8, 16, 32, 64, 128, 256, 512] {
        if let Some(text) = client.substring(lucid_core::Utf16Range::new(0, target)) {
            let actual_len = text.encode_utf16().count();
            current_text = text;
            low = actual_len;
            if actual_len < target {
                // 宿主自动截断到了实际末尾，探测直接完成
                return Some((current_text, low));
            }
        } else {
            // 越界了，确定上限
            high = Some(target);
            break;
        }
    }

    // 若确定了越界上限，在 [low, high] 之间二分查找精确长度
    if let Some(mut hi) = high {
        while low + 1 < hi {
            let mid = (low + hi) / 2;
            if let Some(text) = client.substring(lucid_core::Utf16Range::new(0, mid)) {
                let actual_len = text.encode_utf16().count();
                current_text = text;
                low = actual_len;
                if actual_len < mid {
                    return Some((current_text, low));
                }
            } else {
                hi = mid;
            }
        }
    }

    Some((current_text, low))
}

/// 读取光标前直到本句起点的完整文本。即使前面内容是由其他输入法输入或已存在的文本，也能完整读回。
fn sentence_before_caret(
    client: &crate::imk::TextClient<'_>,
    selected_before_insert: Option<lucid_core::Utf16Range>,
    inserted: &str,
) -> Option<RecoveredSentence> {
    let inserted_units = inserted.encode_utf16().count();
    let caret = selected_before_insert
        .filter(|range| !(range.location == 0 && range.length == 0))
        .map(|range| range.location.saturating_add(inserted_units))
        .or_else(|| {
            client
                .selected_range()
                .filter(|range| !(range.location == 0 && range.length == 0))
                .map(|range| range.location)
        });

    let (text, caret) = safe_read_text_before_caret(client, caret)?;

    let units = text.encode_utf16().collect::<Vec<_>>();
    let actual_start = caret.saturating_sub(units.len());
    let (sentence, offset_in_units, length) = extract_sentence_from_units(&units)?;
    let range = lucid_core::Utf16Range::new(actual_start + offset_in_units, length);
    tracing::info!(
        caret,
        read_units = units.len(),
        sentence_units = length,
        ?range,
        "句末读回完整原文范围"
    );
    Some(RecoveredSentence {
        range,
        text: sentence,
    })
}

/// 从光标前的 UTF-16 代码单元中提取当前句末标点所结束的完整句子。
/// 返回 `(句子文本, 在 units 中的起始偏移, 句子在 units 中的 UTF-16 长度)`。
pub fn extract_sentence_from_units(units: &[u16]) -> Option<(String, usize, usize)> {
    if units.is_empty() {
        return None;
    }
    let end = units.len();
    let mut sentence_start = 0usize;

    // 寻找上一句的句末标点或换行符。take(end.saturating_sub(1)) 确保不把当前刚输入的这最后一个标点当成上一句的结束符。
    for (index, _) in units.iter().enumerate().take(end.saturating_sub(1)) {
        if is_sentence_delimiter(units, index) {
            sentence_start = index + 1;
        }
    }

    if sentence_start >= end {
        return None;
    }

    let slice = &units[sentence_start..end];

    // 计算前导和后置空白（包括半角空格、制表符、换行、全角空格、段落分隔符等）
    let leading = slice
        .iter()
        .take_while(|unit| is_whitespace_unit(**unit))
        .count();

    if leading >= slice.len() {
        return None;
    }

    let trailing = slice[leading..]
        .iter()
        .rev()
        .take_while(|unit| is_whitespace_unit(**unit))
        .count();

    let valid_len = slice.len() - leading - trailing;
    if valid_len == 0 {
        return None;
    }

    let trimmed_slice = &slice[leading..leading + valid_len];
    let sentence = String::from_utf16_lossy(trimmed_slice);

    // 过滤掉仅有标点或长度过短的碎片（至少2个字符）
    if sentence.chars().count() < 2 {
        return None;
    }

    Some((sentence, sentence_start + leading, valid_len))
}

fn is_whitespace_unit(unit: u16) -> bool {
    matches!(
        unit,
        0x0020 | 0x0009 | 0x000A | 0x000D | 0x3000 | 0x2028 | 0x2029
    )
}

fn is_sentence_delimiter(units: &[u16], index: usize) -> bool {
    let unit = units[index];
    match unit {
        // 换行符、段落分隔符、问号、感叹号、中文标点（句号、感叹号、问号）
        0x000A | 0x000D | 0x2028 | 0x2029 | 0x003F | 0x0021 | 0x3002 | 0xFF01 | 0xFF1F => true,
        // 英文句号 '.'
        0x002E => {
            // 如果点前后都是数字（例如 3.14），则视为数字小数点，不作为句子分隔符
            let is_prev_digit = index > 0 && (units[index - 1] as u8).is_ascii_digit();
            let is_next_digit = index + 1 < units.len() && (units[index + 1] as u8).is_ascii_digit();
            if is_prev_digit && is_next_digit {
                return false;
            }
            true
        }
        _ => false,
    }
}

pub fn delete_shared_backward() {
    if let Ok(mut slot) = SHARED_SESSION.lock() {
        slot.get_or_insert_with(crate::imk::InputSession::new)
            .delete_backward();
    }
}

pub fn cancel_suggestion() {
    ACTIVE_REQUEST.fetch_add(1, Ordering::SeqCst);
    SuggestionPanel::hide();
}

pub fn reset_shared() {
    if let Ok(mut slot) = SHARED_SESSION.lock() {
        if let Some(session) = slot.as_mut() {
            session.reset();
        }
    }
}

pub fn flush_shared_pause() {
    let sentence = {
        let Ok(mut slot) = SHARED_SESSION.lock() else {
            return;
        };
        let Some(session) = slot.as_mut() else { return };
        session.take_pause_sentence()
    };
    let Some(sentence) = sentence else { return };
    let range = if let Ok(slot) = SHARED_SESSION.lock() {
        slot.as_ref().map(|session| {
            session.absolute_range(lucid_core::Utf16Range::new(
                sentence.utf16_range.location,
                sentence.utf16_range.length,
            ))
        })
    } else {
        None
    };
    if let Ok(mut slot) = SHARED_SESSION.lock() {
        slot.get_or_insert_with(crate::imk::InputSession::new)
            .remember_original(&sentence.text, range);
    }
    remember_range(range);
    request_correction(sentence.text, next_shared_request(), "pause".to_owned());
}

fn remember_range(range: Option<lucid_core::Utf16Range>) {
    if let Ok(mut slot) = PENDING_RANGE.lock() {
        *slot = range;
    }
}

fn next_shared_request() -> u64 {
    let next = ACTIVE_REQUEST.load(Ordering::SeqCst).wrapping_add(1);
    ACTIVE_REQUEST.store(next, Ordering::SeqCst);
    next
}

pub fn self_check_replacement() -> bool {
    use lucid_core::{CommittedTextReplacement, HostClient, ReplacementOutcome, Utf16Range};
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Document {
        text: String,
        caret: usize,
    }
    let original = "I want to mai coffee.";
    let document = Rc::new(RefCell::new(Document {
        text: original.to_owned(),
        caret: original.encode_utf16().count(),
    }));
    let read_doc = Rc::clone(&document);
    let selected_doc = Rc::clone(&document);
    let insert_doc = Rc::clone(&document);
    let delete_doc = Rc::clone(&document);
    let selection_doc = Rc::clone(&document);
    let mut client = HostClient {
        read_text: move |range: Utf16Range| {
            let document = read_doc.borrow();
            let units = document.text.encode_utf16().collect::<Vec<_>>();
            if range.location + range.length > units.len() {
                return None;
            }
            Some(String::from_utf16_lossy(
                &units[range.location..range.location + range.length],
            ))
        },
        selected_range: move || Some(Utf16Range::new(selected_doc.borrow().caret, 0)),
        set_marked_text: None::<fn(&str, Utf16Range, Utf16Range)>,
        marked_range: None::<fn() -> Option<Utf16Range>>,
        insert_text: move |text: &str, range: Utf16Range| {
            let mut document = insert_doc.borrow_mut();
            let mut units = document.text.encode_utf16().collect::<Vec<_>>();
            if range.location == usize::MAX {
                let caret = document.caret.min(units.len());
                let inserted = text.encode_utf16().collect::<Vec<_>>();
                units.splice(caret..caret, inserted);
                document.caret = caret + text.encode_utf16().count();
            } else if range.location + range.length <= units.len() {
                let inserted = text.encode_utf16().collect::<Vec<_>>();
                units.splice(range.location..range.location + range.length, inserted);
                document.caret = range.location + text.encode_utf16().count();
            }
            document.text = String::from_utf16_lossy(&units);
        },
        delete_backward: Some(move || {
            let mut document = delete_doc.borrow_mut();
            if document.caret == 0 {
                return;
            }
            let mut units = document.text.encode_utf16().collect::<Vec<_>>();
            units.remove(document.caret - 1);
            document.caret -= 1;
            document.text = String::from_utf16_lossy(&units);
        }),
        set_selection: Some(move |range: Utf16Range| {
            selection_doc.borrow_mut().caret = range.location;
        }),
    };
    let range = Utf16Range::new(0, original.encode_utf16().count());
    let outcome = CommittedTextReplacement::replace_range(
        original,
        "I want to buy coffee.",
        range,
        &mut client,
    );
    outcome == ReplacementOutcome::Replaced && document.borrow().text == "I want to buy coffee."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_sentence_mixed_input() {
        // 用户先用其他输入法输入了 ni，再切到 Lucid 输入 Good.
        let units = "niGood.".encode_utf16().collect::<Vec<_>>();
        let result = extract_sentence_from_units(&units);
        assert_eq!(result, Some(("niGood.".to_owned(), 0, 7)));
    }

    #[test]
    fn test_extract_sentence_with_chinese_prefix() {
        // 用户先用中文输入法输入了文字，再切到 Lucid 输入 niGood.
        let units = "你好世界niGood.".encode_utf16().collect::<Vec<_>>();
        let result = extract_sentence_from_units(&units);
        assert_eq!(result, Some(("你好世界niGood.".to_owned(), 0, 11)));
    }

    #[test]
    fn test_extract_sentence_after_previous_sentence() {
        // 存在上一句已结束的句子
        let units = "Hello world. niGood.".encode_utf16().collect::<Vec<_>>();
        let result = extract_sentence_from_units(&units);
        // 上一句在 index 11 结束，后面从 index 13 开始（去除空格）
        assert_eq!(result, Some(("niGood.".to_owned(), 13, 7)));
    }

    #[test]
    fn test_extract_sentence_after_newline() {
        // 换行后输入的内容，不应跨行提取
        let units = "第一行内容\nniGood.".encode_utf16().collect::<Vec<_>>();
        let result = extract_sentence_from_units(&units);
        assert_eq!(result, Some(("niGood.".to_owned(), 6, 7)));
    }

    #[test]
    fn test_extract_sentence_preserves_decimal_point() {
        // 句子中包含数字小数点 3.14，不应被小数点切断
        let units = "The pi is 3.14.".encode_utf16().collect::<Vec<_>>();
        let result = extract_sentence_from_units(&units);
        assert_eq!(result, Some(("The pi is 3.14.".to_owned(), 0, 15)));
    }

    #[test]
    fn test_extract_sentence_with_leading_whitespace() {
        // 句首有半角和全角空白
        let units = " \u{3000}niGood.".encode_utf16().collect::<Vec<_>>();
        let result = extract_sentence_from_units(&units);
        assert_eq!(result, Some(("niGood.".to_owned(), 2, 7)));
    }

    #[test]
    fn test_extract_sentence_single_period_rejected() {
        // 单独一个点不应作为句子提取
        let units = ".".encode_utf16().collect::<Vec<_>>();
        let result = extract_sentence_from_units(&units);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_sentence_with_chinese_punctuation() {
        // 上一句是中文句号结束
        let units = "这是第一句。niGood.".encode_utf16().collect::<Vec<_>>();
        let result = extract_sentence_from_units(&units);
        assert_eq!(result, Some(("niGood.".to_owned(), 6, 7)));
    }
}
