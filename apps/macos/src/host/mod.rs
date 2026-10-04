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
}
static PENDING_RANGE: Mutex<Option<lucid_core::Utf16Range>> = Mutex::new(None);
static PAUSE_FLUSH: Mutex<Option<fn()>> = Mutex::new(None);
static SHARED_SESSION: Mutex<Option<crate::imk::InputSession>> = Mutex::new(None);

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

    // Do not compare IMK proxy pointers here. WeChat, Sublime and WebKit can
    // wrap the same editor in a new proxy during a normal activate/deactivate
    // round-trip (including when the suggestion panel is clicked). Treating
    // that as an app switch used to hide every completed suggestion and made
    // the "使用英文" button look like it did nothing.
    let same_host =
        CURRENT_HOST_ID.with(|slot| slot.borrow().as_deref() == Some(bundle_id.as_str()));
    if !same_host {
        reset_active_context();
        SuggestionPanel::hide();
    }
    remember_client(client, None);
}

pub fn remember_client(client: &objc2::runtime::AnyObject, range: Option<lucid_core::Utf16Range>) {
    let pointer = client as *const objc2::runtime::AnyObject as *mut objc2::runtime::AnyObject;
    let retained = unsafe { Retained::retain(pointer) };
    let host_id = crate::imk::TextClient::new(client).bundle_identifier();
    CURRENT_CLIENT.with(|slot| {
        *slot.borrow_mut() = retained;
    });
    CURRENT_HOST_ID.with(|slot| {
        *slot.borrow_mut() = Some(host_id);
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
    // The click temporarily makes the non-activating panel key. Hide it first,
    // then wait one run-loop turn before talking to the editor. Notes/WebKit
    // otherwise sometimes answer selectedRange/insertText for the panel's
    // stale responder, which looks like a button that did nothing.
    SuggestionPanel::hide();
    let range = PENDING_RANGE.lock().ok().and_then(|slot| *slot);
    let _ = DispatchQueue::main().after(DispatchTime::NOW.time(50_000_000), move || {
        apply_saved_suggestion(original, replacement, range);
    });
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
    let host_id = text_client.bundle_identifier();
    let outcome = if host_id == "com.tencent.xinWeChat" {
        replace_in_wechat(&text_client, &original, &replacement, range)
    } else {
        replace_with_client(&text_client, &original, &replacement, range)
    };
    tracing::info!(?outcome, "替换结果");
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
/// inside a key callback.
pub fn suggestion_origin(width: f64, height: f64) -> Option<NSPoint> {
    let client = CURRENT_CLIENT.with(|slot| slot.borrow().clone())?;
    let text_client = crate::imk::TextClient::new(&client);
    let range = PENDING_RANGE.lock().ok().and_then(|slot| *slot)?;
    let caret = Utf16Range::new(range.end(), 0);
    let caret_rect = text_client
        .first_rect_for_character_range(caret)
        .or_else(|| text_client.line_height_rect_for_character(caret.location))
        .or_else(|| {
            (range.length > 0).then(|| {
                let index = range.end().saturating_sub(1);
                text_client
                    .first_rect_for_character_range(Utf16Range::new(index, 1))
                    .or_else(|| text_client.line_height_rect_for_character(index))
            })?
        })?;
    let origin = panel_origin_near_rect(caret_rect, width, height);
    tracing::debug!(?caret_rect, ?origin, "建议卡片定位到光标");
    Some(origin)
}

fn contains(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.y >= rect.origin.y
        && point.x <= rect.origin.x + rect.size.width
        && point.y <= rect.origin.y + rect.size.height
}

fn panel_origin_near_rect(rect: NSRect, width: f64, height: f64) -> NSPoint {
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return NSPoint::new(rect.origin.x, rect.origin.y - height - 8.0);
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
    let margin = 12.0;
    let max_x =
        (visible.origin.x + visible.size.width - width - margin).max(visible.origin.x + margin);
    let x = (rect.origin.x - 8.0).clamp(visible.origin.x + margin, max_x);
    let below = rect.origin.y - height - 10.0;
    let above = rect.origin.y + rect.size.height + 10.0;
    let y = if below >= visible.origin.y + margin {
        below
    } else {
        above.min(visible.origin.y + visible.size.height - height - margin)
    };
    NSPoint::new(x, y.max(visible.origin.y + margin))
}

/// WeChat's IMK proxy reports a selection but ignores the replacement range
/// passed to `insertText:replacementRange:`.  Sending the replacement directly
/// therefore appends `Hello.` after `nihao.`.  Select the matched sentence and
/// commit over that selection; if the proxy only accepts editing commands,
/// delete exactly that sentence and insert at the verified caret.
fn replace_in_wechat(
    client: &crate::imk::TextClient<'_>,
    original: &str,
    replacement: &str,
    range: Option<lucid_core::Utf16Range>,
) -> lucid_core::ReplacementOutcome {
    use lucid_core::CommittedTextReplacement;

    // Always try to capture the real focused WeChat editor.  Do not gate this
    // behind a second `trusted()` call: Editor::capture logs the exact reason
    // (missing trust, focus changed, unsupported element), and this avoids
    // silently skipping the only atomic replacement path that can work in
    // WeChat's Chromium text proxy.
    if let Some(editor) = accessibility::Editor::capture("com.tencent.xinWeChat") {
        let ax_outcome = editor.replace(original, replacement, range);
        match ax_outcome {
            lucid_core::ReplacementOutcome::Replaced
            | lucid_core::ReplacementOutcome::Unverified => return ax_outcome,
            lucid_core::ReplacementOutcome::Stale | lucid_core::ReplacementOutcome::Unsupported => {
                tracing::info!("微信 AX 替换未完成，回退到 IMK 路径");
            }
        }
    }

    let Some(target) = client_replacement_range(client, original, range) else {
        tracing::info!("微信替换失败：没有找到原文范围");
        return lucid_core::ReplacementOutcome::Stale;
    };
    tracing::info!(
        support = %client.selector_support(&[
            "setSelectedRange:",
            "setMarkedText:selectionRange:replacementRange:",
            "insertText:replacementRange:",
            "deleteBackward:",
            "delete:",
            "doCommandBySelector:",
        ]),
        "微信输入代理能力"
    );
    if client.commit_marked_replacement(original, replacement, target) {
        return lucid_core::ReplacementOutcome::Replaced;
    }
    if client.substring(target).as_deref() != Some(original)
        && !CommittedTextReplacement::matches_loosely(client.substring(target).as_deref(), original)
    {
        tracing::info!(?target, "微信替换失败：原文已变化");
        return lucid_core::ReplacementOutcome::Stale;
    }

    // WeChat ignores `replacementRange`, but several releases replace the active
    // selection.  This is one write and never needs synthetic keys or the clipboard.
    if try_replace_by_active_selection(client, target, replacement) {
        return lucid_core::ReplacementOutcome::Replaced;
    }

    // The proxy often reports `deleteBackward:` while ignoring that message.
    // Selecting the sentence first makes `delete:` the command that actually
    // removes it; only a verified caret movement is allowed to continue.
    if try_replace_by_verified_delete(client, target, replacement) {
        return lucid_core::ReplacementOutcome::Replaced;
    }

    tracing::info!(?target, "微信替换失败：选区和删除命令都没有改写原文");
    lucid_core::ReplacementOutcome::Unverified
}

/// Replace a committed range by selecting it in the IMK client and committing
/// the English at the active selection.  WeChat ignores the explicit
/// `replacementRange` argument, but its editor selection is still authoritative
/// when this call is made while the IMK connection is focused.
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
    // WeChat commits ordinary letters itself and only delivers the final
    // punctuation to the input method. Reading the committed line here is the
    // only way to translate the whole sentence instead of just "o.".
    if host == "com.tencent.xinWeChat"
        && text
            .chars()
            .any(|character| ".!?。？！".contains(character))
        && let Some(client) = CURRENT_CLIENT.with(|slot| slot.borrow().clone())
    {
        let text_client = crate::imk::TextClient::new(&client);
        let caret = selected_before_insert
            .filter(|range| range.length == 0)
            .map(|range| range.location.saturating_add(text.encode_utf16().count()));
        if let Some(caret) = caret
            && accessibility::trusted()
            && let Some(editor) = accessibility::Editor::capture(host)
            && let Some(recovered) = editor.value_before_caret(caret)
        {
            tracing::info!(
                host,
                chars = recovered.chars().count(),
                "微信 AX 读回完整原文"
            );
            let range = lucid_core::Utf16Range::new(
                caret.saturating_sub(recovered.encode_utf16().count()),
                recovered.encode_utf16().count(),
            );
            if let Ok(mut slot) = SHARED_SESSION.lock() {
                slot.get_or_insert_with(crate::imk::InputSession::new)
                    .remember_original(&recovered, Some(range));
            }
            remember_range(Some(range));
            request_correction(recovered, next_shared_request(), host.to_owned());
            return true;
        }
        match sentence_before_caret(&text_client, selected_before_insert, text) {
            Some(sentence) => {
                let chars = sentence.text.chars().count();
                tracing::info!(host, chars, "微信句末读回完整原文");
                if let Ok(mut slot) = SHARED_SESSION.lock() {
                    slot.get_or_insert_with(crate::imk::InputSession::new)
                        .remember_original(&sentence.text, Some(sentence.range));
                }
                remember_range(Some(sentence.range));
                request_correction(sentence.text, next_shared_request(), host.to_owned());
                return true;
            }
            None => tracing::info!(?selected_before_insert, "微信句末读回失败，改用已跟踪文本"),
        }
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

/// Read the current line ending at the caret. WeChat may have committed every
/// letter before the punctuation without sending those keys to Lucid.
fn sentence_before_caret(
    client: &crate::imk::TextClient<'_>,
    selected_before_insert: Option<lucid_core::Utf16Range>,
    inserted: &str,
) -> Option<RecoveredSentence> {
    let inserted_units = inserted.encode_utf16().count();
    let caret = selected_before_insert
        .filter(|range| range.length == 0)
        .map(|range| range.location.saturating_add(inserted_units))
        .or_else(|| {
            client
                .selected_range()
                .filter(|range| range.length == 0)
                .map(|range| range.location)
        })?;
    if caret == 0 {
        tracing::info!("微信句末读回失败：光标在文档开头");
        return None;
    }
    let start = caret.saturating_sub(512);
    let window = lucid_core::Utf16Range::new(start, caret - start);
    let Some(text) = client.substring(window) else {
        tracing::info!(?window, "微信句末读回失败：输入框没有返回光标前文本");
        return None;
    };
    let units = text.encode_utf16().collect::<Vec<_>>();
    // The period is the boundary even when an earlier input method committed
    // the preceding letters. Do not stop at the letters Lucid itself tracked.
    let end = units.len();
    let mut sentence_start = 0usize;
    for (index, unit) in units.iter().enumerate().take(end.saturating_sub(1)) {
        if matches!(
            *unit,
            0x000A | 0x000D | 0x2028 | 0x2029 | 0x002E | 0x003F | 0x0021 | 0x3002 | 0xFF01 | 0xFF1F
        ) {
            sentence_start = index + 1;
        }
    }
    let sentence = String::from_utf16_lossy(&units[sentence_start..end])
        .trim()
        .to_owned();
    tracing::info!(
        caret,
        read_units = units.len(),
        sentence_units = sentence.encode_utf16().count(),
        "微信句末读回范围"
    );
    if sentence.chars().count() < 2 {
        return None;
    }
    let leading = units[sentence_start..end]
        .iter()
        .take_while(|unit| matches!(*unit, 0x0020 | 0x0009))
        .count();
    Some(RecoveredSentence {
        range: lucid_core::Utf16Range::new(
            start + sentence_start + leading,
            sentence.encode_utf16().count(),
        ),
        text: sentence,
    })
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
