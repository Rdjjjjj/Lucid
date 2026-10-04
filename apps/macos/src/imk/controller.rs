//! IMK 输入控制器。每个文本框一个实例。
//!
//! 不同宿主走不同的 IMK 入口：普通 Cocoa 文本框走
//! `inputText:key:modifiers:client:`，部分编辑器/WebView 只走
//! `handleEvent:client:`。两条入口都在类注册时就声明为真实的 Objective-C 方法；可打印字符
//! 由输入法写入并返回 YES，命令键返回 NO 交给应用。

use std::cell::RefCell;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, Bool, NSObjectProtocol, Sel};
use objc2::{DefinedClass, define_class, msg_send, sel};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::NSString;
use objc2_input_method_kit::{IMKInputController, IMKServer};

use super::client::TextClient;
use super::session::InputSession;
use crate::host;
use crate::suggestion::SuggestionPanel;

define_class!(
    #[unsafe(super(IMKInputController))]
    #[name = "LucidInputController"]
    #[ivars = RefCell<ControllerState>]
    pub struct LucidInputController;

    impl LucidInputController {
        #[unsafe(method_id(initWithServer:delegate:client:))]
        fn init_with_server(
            this: Allocated<Self>,
            server: Option<&IMKServer>,
            delegate: Option<&AnyObject>,
            client: Option<&AnyObject>,
        ) -> Option<Retained<Self>> {
            let this = this.set_ivars(RefCell::new(ControllerState::new()));
            unsafe { msg_send![super(this), initWithServer: server, delegate: delegate, client: client] }
        }

        #[unsafe(method(inputText:key:modifiers:client:))]
        fn input_text_key(
            &self,
            text: Option<&NSString>,
            key_code: isize,
            flags: usize,
            client: Option<&AnyObject>,
        ) -> Bool {
            let Some(client) = client else {
                tracing::info!("inputText 没有 client");
                return Bool::NO;
            };
            let text = text.map(|value| value.to_string()).unwrap_or_default();
            Bool::new(self.insert_printable(&text, key_code as i32, flags, TextClient::new(client)))
        }

        #[unsafe(method(inputText:client:))]
        fn input_text(&self, text: Option<&NSString>, client: Option<&AnyObject>) -> Bool {
            self.input_text_key(sel!(inputText:key:modifiers:client:), text, -1, 0, client)
        }

        #[unsafe(method(handleEvent:client:))]
        fn handle_event(&self, event: Option<&NSEvent>, client: Option<&AnyObject>) -> Bool {
            let Some(event) = event else { return Bool::NO };
            if event.r#type() != NSEventType::KeyDown {
                return Bool::NO;
            }
            self.input_text_key(
                sel!(inputText:key:modifiers:client:),
                event.characters().as_deref(),
                event.keyCode() as isize,
                event.modifierFlags().0 as usize,
                client,
            )
        }

        #[unsafe(method(didCommandBySelector:client:))]
        fn did_command(&self, selector: Option<Sel>, client: Option<&AnyObject>) -> Bool {
            if client.is_some_and(|client| TextClient::new(client).is_sensitive_input()) {
                host::reset_active_context();
                SuggestionPanel::hide();
                return Bool::NO;
            }
            let name = selector.map(|value| value.name().to_string_lossy().into_owned()).unwrap_or_default();
            let host_id = client.map(|value| TextClient::new(value).bundle_identifier()).unwrap_or_default();
            tracing::info!(host = %host_id, selector = %name, "didCommand");
            if selector == Some(sel!(deleteBackward:)) {
                let mut state = self.ivars().borrow_mut();
                state.session.cancel_pause();
                state.session.delete_backward();
                host::delete_shared_backward();
            } else if selector == Some(sel!(insertNewline:)) {
                let mut state = self.ivars().borrow_mut();
                state.session.cancel_pause();
                state.session.reset();
                host::reset_shared();
            } else if selector == Some(sel!(cancelOperation:)) && SuggestionPanel::has_replacement() {
                let mut state = self.ivars().borrow_mut();
                state.session.invalidate_request();
                state.session.clear_pending();
                SuggestionPanel::hide();
            }
            Bool::NO
        }

        #[unsafe(method(recognizedEvents:))]
        fn recognized_events(&self, _sender: Option<&AnyObject>) -> usize {
            // 0 means "do not deliver keys". TextEdit asks this before sending inputText.
            objc2_app_kit::NSEventMask::KeyDown.0 as usize
        }

        #[unsafe(method(activateServer:))]
        fn activate_server(&self, sender: Option<&AnyObject>) {
            if sender.is_some_and(|client| TextClient::new(client).is_sensitive_input()) {
                host::reset_active_context();
                SuggestionPanel::hide();
                return;
            }
            let host_id = sender.map(|value| TextClient::new(value).bundle_identifier());
            tracing::info!(host = ?host_id, "activateServer");
            self.ivars().borrow_mut().session.reset();
            if let Some(sender) = sender {
                host::activate_client(sender);
            }
            host::set_pause_flush(flush_active_controller);
        }

        #[unsafe(method(deactivateServer:))]
        fn deactivate_server(&self, _sender: Option<&AnyObject>) {
            tracing::info!("deactivateServer，保留当前句子和异步建议");
            // IMK can deactivate around a normal insertText call. The next
            // activation decides whether the client really changed; clearing
            // the shared request here made the English panel disappear before
            // the model response arrived.
            self.ivars().borrow_mut().session.reset();
        }

        #[unsafe(method(commitComposition:))]
        fn commit_composition(&self, _sender: Option<&AnyObject>) {
            // 句末提交不是用户点了「使用英文」，不能在这里替换原文。
        }
    }

    unsafe impl NSObjectProtocol for LucidInputController {}
);

pub struct ControllerState {
    session: InputSession,
}

impl ControllerState {
    fn new() -> Self {
        Self {
            session: InputSession::new(),
        }
    }
}

impl LucidInputController {
    fn insert_printable(
        &self,
        text: &str,
        key_code: i32,
        flags: usize,
        client: TextClient<'_>,
    ) -> bool {
        if client.is_sensitive_input() {
            host::reset_active_context();
            SuggestionPanel::hide();
            return false;
        }
        let host_id = client.bundle_identifier();
        // Character counts are sufficient for diagnostics; key codes can
        // reconstruct private text and must not be persisted.
        tracing::info!(host = %host_id, chars = text.chars().count(), "inputText");
        if text.is_empty() {
            return false;
        }
        host::remember_client(client.object(), None);
        if has_command_or_control(flags) {
            return false;
        }
        // Raw-key hosts do not necessarily call didCommandBySelector:.
        // Keep the shared sentence in sync even when they only send key events.
        if key_code == 51 || text == "\u{8}" || text == "\u{7f}" {
            host::delete_shared_backward();
            return false;
        }
        if is_return(key_code, text) {
            host::reset_shared();
            return false;
        }
        if key_code == 53 || text == "\u{1b}" {
            host::cancel_suggestion();
            return false;
        }
        if !is_insertable(text) {
            return false;
        }
        SuggestionPanel::hide();
        let selected_before_insert = client.selected_range();
        let is_duplicate = {
            let mut state = self.ivars().borrow_mut();
            state.session.cancel_pause();
            state.session.should_ignore_duplicate(text, key_code)
        };
        if is_duplicate {
            // A few IMK hosts deliver the same physical key through both
            // handleEvent: and inputText:. We already inserted it on the first
            // callback; returning YES here prevents the host from inserting a
            // second copy itself.
            tracing::debug!(host = %host_id, "忽略重复按键");
            return true;
        }
        client.insert_text(text, None);
        host::note_inserted(text, &host_id, selected_before_insert);
        tracing::info!(host = %host_id, chars = text.chars().count(), "已上屏");
        true
    }
}

fn is_return(key_code: i32, text: &str) -> bool {
    key_code == 36 || key_code == 76 || text == "\n" || text == "\r"
}

fn has_command_or_control(flags: usize) -> bool {
    let command = NSEventModifierFlags::Command.0;
    let control = NSEventModifierFlags::Control.0;
    flags & (command | control) != 0
}

fn is_insertable(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|character| {
            let value = character as u32;
            value >= 0x20 && value != 0x7f && !(0xF700..=0xF8FF).contains(&value)
        })
}

fn flush_active_controller() {
    host::flush_shared_pause();
}

/// Check the actual class that IMK will instantiate, not just its Rust type.
/// BOOL encodes as B on Apple Silicon and c on some Intel targets; never
/// hard-code a method signature in a post-registration class_replaceMethod.
pub fn verify_event_entries() -> Result<(), String> {
    use objc2::{ClassType, Encode};
    let class = LucidInputController::class();
    for (selector, count) in [
        (sel!(inputText:key:modifiers:client:), 6),
        (sel!(inputText:client:), 4),
        (sel!(handleEvent:client:), 4),
    ] {
        if !class
            .instance_methods()
            .iter()
            .any(|method| method.name() == selector)
        {
            return Err(format!("控制器缺少输入入口 {selector}"));
        }
        let method = class.instance_method(selector).ok_or("无法读取输入入口")?;
        if method.arguments_count() != count
            || method.return_type().to_str().ok() != Some(Bool::ENCODING.to_string().as_str())
        {
            return Err(format!("输入入口 ABI 不匹配：{selector}"));
        }
    }
    let method = class
        .instance_method(sel!(recognizedEvents:))
        .ok_or("缺少事件掩码入口")?;
    if method.return_type().to_str().ok() != Some(usize::ENCODING.to_string().as_str()) {
        return Err("事件掩码 ABI 不匹配".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imk_entries_exist_on_class_registration_with_native_bool_abi() {
        verify_event_entries().unwrap();
    }
}
