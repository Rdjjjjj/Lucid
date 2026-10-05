//! 建议卡片。按钮只触发确认或保留，替换本身走 Core 的校验。

use std::cell::{Cell, RefCell};

use dispatch2::{DispatchQueue, DispatchTime};

use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBackingStoreType, NSButton, NSEvent, NSFont, NSPanel, NSScreen, NSTextField,
    NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::host;

thread_local! {
    static PANEL: RefCell<Option<PanelState>> = const { RefCell::new(None) };
    static PANEL_GENERATION: Cell<u64> = const { Cell::new(0) };
}

struct PanelState {
    window: Retained<NSPanel>,
    replacement: Option<String>,
    original: Option<String>,
    actions: Vec<Retained<SuggestionAction>>,
}

struct ActionIvars {
    kind: &'static str,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "LucidSuggestionAction"]
    #[ivars = ActionIvars]
    struct SuggestionAction;

    impl SuggestionAction {
        #[unsafe(method(pressed:))]
        fn pressed(&self, _sender: Option<&NSObject>) {
            tracing::info!(kind = self.ivars().kind, "建议按钮被点击");
            if self.ivars().kind == "replace" {
                host::apply_current_suggestion();
            } else {
                host::keep_original();
            }
        }
    }

    unsafe impl NSObjectProtocol for SuggestionAction {}
);

pub struct SuggestionPanel;

impl SuggestionPanel {
    pub fn show_status(text: &str) {
        let text = text.to_owned();
        on_main(move || Self::present(&text, None, None));
    }

    pub fn show_suggestion(original: &str, replacement: &str) {
        let original = original.to_owned();
        let replacement = replacement.to_owned();
        on_main(move || Self::present(&replacement.clone(), Some(original), Some(replacement)));
    }

    pub fn hide() {
        on_main(|| {
            PANEL_GENERATION.with(|generation| generation.set(generation.get().wrapping_add(1)));
            PANEL.with(|panel| {
                if let Some(state) = panel.borrow_mut().take() {
                    state.window.orderOut(None);
                }
            });
        });
    }

    pub fn has_replacement() -> bool {
        PANEL.with(|panel| {
            panel
                .borrow()
                .as_ref()
                .and_then(|state| state.replacement.clone())
                .is_some()
        })
    }

    pub fn take_current() -> Option<(String, String)> {
        PANEL.with(|panel| {
            let state = panel.borrow();
            let state = state.as_ref()?;
            Some((state.original.clone()?, state.replacement.clone()?))
        })
    }

    fn present(text: &str, original: Option<String>, replacement: Option<String>) {
        tracing::info!(
            has_replacement = replacement.is_some(),
            chars = text.chars().count(),
            "显示建议面板"
        );
        let Some(mtm) = MainThreadMarker::new() else {
            tracing::error!("建议面板未在主线程创建");
            return;
        };
        let is_suggestion = replacement.is_some();
        let (width, height) = if is_suggestion {
            let text_len = text.chars().count();
            if text_len <= 20 {
                // 短句（如 Thank you. / Hello. / Good morning.）
                (220.0, 78.0)
            } else if text_len <= 50 {
                // 中等长度句子
                (250.0, 84.0)
            } else {
                // 较长句子
                (280.0, 94.0)
            }
        } else {
            (200.0, 42.0)
        };

        // 先关闭旧面板，避免“正在改写”面板和结果面板叠在一起。
        let generation = PANEL_GENERATION.with(|generation| {
            let next = generation.get().wrapping_add(1);
            generation.set(next);
            next
        });
        PANEL.with(|panel| {
            if let Some(state) = panel.borrow_mut().take() {
                state.window.orderOut(None);
            }
        });

        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::NonactivatingPanel
            | NSWindowStyleMask::UtilityWindow;
        let window = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height)),
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        window.setFloatingPanel(true);
        // 不要使用 NSScreenSaverWindowLevel：它会压住其他应用，像“卡死”一样。
        window.setLevel(objc2_app_kit::NSFloatingWindowLevel);
        window.setHidesOnDeactivate(!is_suggestion);
        window.setIgnoresMouseEvents(!is_suggestion);
        window.setTitle(&NSString::from_str(if is_suggestion {
            "英文建议"
        } else {
            "Lucid"
        }));

        let (label_origin, label_size, font_size) = if is_suggestion {
            let label_h = (height - 38.0).max(24.0);
            (
                NSPoint::new(10.0, height - label_h - 4.0),
                NSSize::new(width - 20.0, label_h),
                13.5,
            )
        } else {
            (
                NSPoint::new(10.0, 8.0),
                NSSize::new(width - 20.0, height - 16.0),
                12.0,
            )
        };
        let label = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
        label.setFrame(NSRect::new(label_origin, label_size));
        label.setFont(Some(&NSFont::systemFontOfSize(font_size)));
        let mut actions = Vec::new();
        if let Some(content) = window.contentView() {
            content.addSubview(&label);
            if is_suggestion {
                let replace_action = action(mtm, "replace");
                let keep_action = action(mtm, "keep");
                let replace = unsafe {
                    NSButton::buttonWithTitle_target_action(
                        &NSString::from_str("使用英文"),
                        Some(&replace_action),
                        Some(sel!(pressed:)),
                        mtm,
                    )
                };
                let keep = unsafe {
                    NSButton::buttonWithTitle_target_action(
                        &NSString::from_str("保留原文"),
                        Some(&keep_action),
                        Some(sel!(pressed:)),
                        mtm,
                    )
                };
                let btn_spacing = 8.0;
                let btn_margin = 10.0;
                let btn_width = ((width - btn_margin * 2.0 - btn_spacing) / 2.0).floor();
                let btn_height = 24.0;
                let btn_y = 7.0;
                replace.setFrame(NSRect::new(
                    NSPoint::new(btn_margin, btn_y),
                    NSSize::new(btn_width, btn_height),
                ));
                keep.setFrame(NSRect::new(
                    NSPoint::new(btn_margin + btn_width + btn_spacing, btn_y),
                    NSSize::new(btn_width, btn_height),
                ));
                content.addSubview(&replace);
                content.addSubview(&keep);
                actions = vec![replace_action, keep_action];
            }
        }
        let frame = window.frame();
        let origin = host::suggestion_origin(frame.size.width, frame.size.height)
            .unwrap_or_else(|| panel_origin(frame.size.height, frame.size.width));
        window.setFrameOrigin(origin);
        // A nonactivating panel can still receive button clicks, but merely
        // ordering it to the front is not enough on every macOS host: AppKit
        // may leave the panel non-key, so the first button press is swallowed.
        // Make this transient panel key without activating a separate app.
        window.setBecomesKeyOnlyIfNeeded(false);
        window.makeKeyAndOrderFront(None);
        PANEL.with(|panel| {
            for action in &actions {
                std::mem::forget(action.clone());
            }
            *panel.borrow_mut() = Some(PanelState {
                window,
                replacement,
                original,
                actions,
            });
        });

        // 进度提示只是状态反馈，不应永久遮挡用户界面；结果面板不会被这个计时器关闭。
        if !is_suggestion {
            let delay = if text.starts_with("正在") {
                1_500_000_000
            } else {
                4_000_000_000
            };
            let _ = DispatchQueue::main().after(DispatchTime::NOW.time(delay), move || {
                PANEL_GENERATION.with(|current| {
                    if current.get() != generation {
                        return;
                    }
                    PANEL.with(|panel| {
                        if let Some(state) = panel.borrow_mut().take() {
                            state.window.orderOut(None);
                        }
                    });
                });
            });
        }
    }
}

fn on_main<F>(work: F)
where
    F: FnOnce() + Send + 'static,
{
    if MainThreadMarker::new().is_some() {
        work();
    } else {
        DispatchQueue::main().exec_async(work);
    }
}

fn action(mtm: MainThreadMarker, kind: &'static str) -> Retained<SuggestionAction> {
    let this = SuggestionAction::alloc(mtm).set_ivars(ActionIvars { kind });
    unsafe { msg_send![super(this), init] }
}

fn panel_origin(height: f64, width: f64) -> NSPoint {
    let Some(mtm) = MainThreadMarker::new() else {
        return NSPoint::new(80.0, 80.0);
    };
    let mouse = NSEvent::mouseLocation();
    let screen = NSScreen::screens(mtm)
        .iter()
        .find(|screen| contains(screen.frame(), mouse))
        .or_else(|| NSScreen::mainScreen(mtm));
    let visible = screen
        .as_ref()
        .map(|screen| screen.visibleFrame())
        .unwrap_or(NSRect::new(
            NSPoint::new(80.0, 80.0),
            NSSize::new(800.0, 600.0),
        ));
    NSPoint::new(
        visible.origin.x + visible.size.width - width - 28.0,
        visible.origin.y + visible.size.height - height - 28.0,
    )
}

fn contains(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.y >= rect.origin.y
        && point.x <= rect.origin.x + rect.size.width
        && point.y <= rect.origin.y + rect.size.height
}
