//! Verified keyboard fallback for WeChat's legacy IMK proxy.
//!
//! `setSelectedRange:` and AX may both be unavailable, while Cmd+A and IMK
//! readback still work. Never paste unless the whole selection has been read
//! and the original matched. Each wait yields the input-method run loop.

use std::cell::RefCell;
use std::sync::atomic::Ordering;

use dispatch2::{DispatchQueue, DispatchTime};
use lucid_core::{ReplacementOutcome, Utf16Range};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting, NSWorkspace};
use objc2_foundation::{NSArray, NSString};

use super::{ACTIVE_REQUEST, CURRENT_CLIENT, CURRENT_HOST_PID, utf16_replace, utf16_slice};

thread_local! {
    static ACTIVE: RefCell<Option<Replacement>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy)]
enum Phase {
    SelectOriginal,
    SelectResult,
}

struct Replacement {
    original: String,
    replacement: String,
    range: Option<Utf16Range>,
    expected: Option<String>,
    pid: i32,
    generation: u64,
    phase: Phase,
    attempts: u8,
    clipboard: Option<Clipboard>,
    completion: Box<dyn FnOnce(ReplacementOutcome)>,
}

struct Clipboard {
    items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>>,
    owned_count: isize,
}

impl Clipboard {
    fn prepare(text: &str) -> Option<Self> {
        let board = NSPasteboard::generalPasteboard();
        let initial_count = board.changeCount();
        let mut items = Vec::new();
        if let Some(previous) = board.pasteboardItems() {
            for item in previous {
                let copy = NSPasteboardItem::new();
                for kind in item.types() {
                    // Materialize every type before clearing, including image,
                    // file, and rich-text data. Abort if a provider cannot do so.
                    let data = item.dataForType(&kind)?;
                    if !copy.setData_forType(&data, &kind) {
                        return None;
                    }
                }
                items.push(ProtocolObject::from_retained(copy));
            }
        }
        if board.changeCount() != initial_count {
            return None;
        }
        board.clearContents();
        let mut snapshot = Self {
            items,
            owned_count: board.changeCount(),
        };
        if !board.setString_forType(
            &NSString::from_str(text),
            &NSString::from_str("public.utf8-plain-text"),
        ) {
            snapshot.owned_count = board.changeCount();
            return None; // Drop restores the snapshot.
        }
        snapshot.owned_count = board.changeCount();
        Some(snapshot)
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        let board = NSPasteboard::generalPasteboard();
        // A new user copy wins over our saved clipboard.
        if board.changeCount() == self.owned_count {
            board.clearContents();
            if !self.items.is_empty() {
                board.writeObjects(&NSArray::from_retained_slice(&self.items));
            }
        }
    }
}

pub(super) fn start(
    original: String,
    replacement: String,
    range: Option<Utf16Range>,
    completion: impl FnOnce(ReplacementOutcome) + 'static,
) {
    let pid = CURRENT_HOST_PID.with(|slot| *slot.borrow());
    let Some(pid) = pid else {
        completion(ReplacementOutcome::Unsupported);
        return;
    };
    if ACTIVE.with(|slot| slot.borrow().is_some()) {
        completion(ReplacementOutcome::Unsupported);
        return;
    }
    let fallback_range = CURRENT_CLIENT.with(|slot| {
        slot.borrow().as_ref().and_then(|object| {
            lucid_core::replacement::range_behind_caret(
                original.encode_utf16().count(),
                crate::imk::TextClient::new(object).selected_range(),
            )
        })
    });
    let state = Replacement {
        original,
        replacement,
        range: range.or(fallback_range),
        expected: None,
        pid,
        generation: ACTIVE_REQUEST.load(Ordering::SeqCst),
        phase: Phase::SelectOriginal,
        attempts: 0,
        clipboard: None,
        completion: Box::new(completion),
    };
    if !state.focused() {
        (state.completion)(ReplacementOutcome::Stale);
        return;
    }
    tracing::info!(pid, range = ?state.range, "微信启动已验证的快捷键覆盖流程");
    post_key(pid, 0 /* A */, COMMAND);
    enqueue(state);
}

impl Replacement {
    fn focused(&self) -> bool {
        ACTIVE_REQUEST.load(Ordering::SeqCst) == self.generation
            && !crate::imk::secure_input_enabled()
            && NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .is_some_and(|app| {
                    app.processIdentifier() == self.pid
                        && app
                            .bundleIdentifier()
                            .is_some_and(|id| id.to_string() == "com.tencent.xinWeChat")
                })
    }
}

fn enqueue(state: Replacement) {
    ACTIVE.with(|slot| *slot.borrow_mut() = Some(state));
    let _ = DispatchQueue::main().after(DispatchTime::NOW.time(80_000_000), step);
}

fn step() {
    let Some(mut state) = ACTIVE.with(|slot| slot.borrow_mut().take()) else {
        return;
    };
    if !state.focused() {
        finish(state, ReplacementOutcome::Stale);
        return;
    }
    let selected_text = CURRENT_CLIENT.with(|slot| {
        let object = slot.borrow().clone()?;
        let client = crate::imk::TextClient::new(&object);
        let range = client.selected_range()?;
        if range.location != 0 || range.length == 0 {
            return None;
        }
        let text = client.substring(range)?;
        (text.encode_utf16().count() == range.length).then_some(text)
    });
    let Some(text) = selected_text else {
        state.attempts += 1;
        if state.attempts < 8 {
            enqueue(state);
        } else {
            tracing::warn!("微信快捷键覆盖停止：没有读到完整选区");
            finish(state, ReplacementOutcome::Unverified);
        }
        return;
    };
    match state.phase {
        Phase::SelectOriginal => {
            let Some(expected) =
                replacement_draft(&text, &state.original, &state.replacement, state.range)
            else {
                tracing::info!("微信快捷键覆盖停止：原文已经变化");
                finish(state, ReplacementOutcome::Stale);
                return;
            };
            let Some(clipboard) = Clipboard::prepare(&expected) else {
                tracing::warn!("微信快捷键覆盖停止：无法备份剪贴板");
                finish(state, ReplacementOutcome::Unsupported);
                return;
            };
            state.clipboard = Some(clipboard);
            state.expected = Some(expected);
            if !state.focused() {
                finish(state, ReplacementOutcome::Stale);
                return;
            }
            post_key(state.pid, 9 /* V */, COMMAND);
            state.phase = Phase::SelectResult;
            state.attempts = 0;
            // Readback must occur after WeChat processes the paste. The next
            // run-loop turn selects the resulting draft, and then reads it.
            ACTIVE.with(|slot| *slot.borrow_mut() = Some(state));
            let _ = DispatchQueue::main().after(DispatchTime::NOW.time(120_000_000), || {
                let state = ACTIVE.with(|slot| slot.borrow_mut().take());
                if let Some(state) = state {
                    if state.focused() {
                        post_key(state.pid, 0, COMMAND);
                        enqueue(state);
                    } else {
                        finish(state, ReplacementOutcome::Unverified);
                    }
                }
            });
        }
        Phase::SelectResult => {
            let verified = state.expected.as_deref() == Some(text.as_str());
            tracing::info!(verified, "微信快捷键覆盖读回结果");
            finish(
                state,
                if verified {
                    ReplacementOutcome::Replaced
                } else {
                    ReplacementOutcome::Unverified
                },
            );
        }
    }
}

fn finish(state: Replacement, outcome: ReplacementOutcome) {
    // Collapse our whole-editor selection only if focus and input stayed put.
    if state.focused() {
        post_key(state.pid, 124 /* Right */, 0);
    }
    drop(state.clipboard);
    if outcome == ReplacementOutcome::Unsupported {
        crate::suggestion::SuggestionPanel::show_status("微信未能准备替换内容，原文已保留。");
    }
    (state.completion)(outcome);
}

/// Preserve every character outside the exact sentence, including other drafts.
fn replacement_draft(
    before: &str,
    original: &str,
    replacement: &str,
    range: Option<Utf16Range>,
) -> Option<String> {
    // Plain-text paste must not erase inline images or other rich objects.
    if before.contains('\u{fffc}') || replacement.is_empty() {
        return None;
    }
    let mut target = range.or_else(|| {
        (before == original).then_some(Utf16Range::new(0, original.encode_utf16().count()))
    })?;
    if utf16_slice(before, target).as_deref() != Some(original) {
        return None;
    }
    // WeChat can report the caret before the just-committed terminator in the
    // key callback. Include that terminator when the AI already supplies one.
    if !original.ends_with(['.', '!', '?', '。', '！', '？'])
        && replacement.ends_with(['.', '!', '?', '。', '！', '？'])
        && utf16_slice(before, Utf16Range::new(target.end(), 1))
            .is_some_and(|s| matches!(s.as_str(), "." | "!" | "?" | "。" | "！" | "？"))
    {
        target.length += 1;
    }
    utf16_replace(before, target, replacement)
}

const COMMAND: u64 = 1 << 20;
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceCreate(state: i32) -> *mut std::ffi::c_void;
    fn CGEventCreateKeyboardEvent(
        source: *const std::ffi::c_void,
        key: u16,
        down: bool,
    ) -> *mut std::ffi::c_void;
    fn CGEventSetFlags(event: *mut std::ffi::c_void, flags: u64);
    fn CGEventPostToPid(pid: i32, event: *mut std::ffi::c_void);
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(object: *const std::ffi::c_void);
}

fn post_key(pid: i32, key: u16, flags: u64) {
    unsafe {
        let source = CGEventSourceCreate(0);
        for down in [true, false] {
            let event = CGEventCreateKeyboardEvent(source, key, down);
            if !event.is_null() {
                CGEventSetFlags(event, flags);
                CGEventPostToPid(pid, event);
                CFRelease(event.cast());
            }
        }
        if !source.is_null() {
            CFRelease(source.cast());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_other_sentences_and_emoji() {
        let before = "草稿😀。nihao.\n后面的文字";
        let range = Utf16Range::new("草稿😀。".encode_utf16().count(), 5);
        assert_eq!(
            replacement_draft(before, "nihao", "Hello.", Some(range)).as_deref(),
            Some("草稿😀。Hello.\n后面的文字")
        );
    }
    #[test]
    fn refuses_stale_ranges_and_surrogate_splits() {
        assert!(
            replacement_draft(
                "other nihao.",
                "nihao",
                "Hello.",
                Some(Utf16Range::new(0, 5))
            )
            .is_none()
        );
        assert!(
            replacement_draft("😀nihao", "nihao", "Hello.", Some(Utf16Range::new(1, 5))).is_none()
        );
        assert!(replacement_draft("nihao more", "nihao", "Hello.", None).is_none());
    }
    #[test]
    fn only_consumes_a_pending_terminator_once() {
        assert_eq!(
            replacement_draft("nihao..", "nihao", "Hello.", Some(Utf16Range::new(0, 5))).as_deref(),
            Some("Hello..")
        );
        assert_eq!(
            replacement_draft("nihao.", "nihao.", "Hello.", None).as_deref(),
            Some("Hello.")
        );
    }
}
