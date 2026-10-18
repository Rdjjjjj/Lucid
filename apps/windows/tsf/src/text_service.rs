//! 进程内 TSF 文本服务。按键由 TSF 交给这里，文字直接写入当前文档。

use std::sync::atomic::{AtomicU32, Ordering};

use windows::{
    Win32::{
        Foundation::{LPARAM, WPARAM},
        System::Com::{IClassFactory, IClassFactory_Impl},
        UI::TextServices::{
            ITfContext, ITfDocumentMgr, ITfEditRecord, ITfKeyEventSink, ITfKeyEventSink_Impl,
            ITfKeystrokeMgr, ITfSource, ITfTextEditSink, ITfTextEditSink_Impl,
            ITfTextInputProcessor_Impl, ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl,
            ITfThreadMgr, ITfThreadMgrEventSink, ITfThreadMgrEventSink_Impl, TF_TMAE_SECUREMODE,
            TF_TMF_SECUREMODE, TS_SD_READONLY, TS_SS_TRANSITORY,
        },
    },
    core::{BOOL, GUID, IUnknown, IUnknownImpl, Interface, Result, implement},
};

use crate::document;
use crate::keys::{self, KeyAction};
use crate::session::{Edit, InputSession, KeyDisposition};
use crate::suggestion;
use lucid_platform::{ClientMessage, ServerMessage, Utf16Range as ProtocolRange};

static OBJECTS: AtomicU32 = AtomicU32::new(0);

pub fn active_object_count() -> u32 {
    OBJECTS.load(Ordering::SeqCst)
}

struct ObjectGuard;

impl ObjectGuard {
    fn new() -> Self {
        OBJECTS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for ObjectGuard {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}

#[implement(IClassFactory)]
pub struct ClassFactory;

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(
        &self,
        outer: windows::core::Ref<IUnknown>,
        iid: *const GUID,
        object: *mut *mut core::ffi::c_void,
    ) -> Result<()> {
        if !outer.is_null() {
            return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                -2147221232,
            )));
        }
        let service: IUnknown = TextService::new().into();
        unsafe { service.query(iid, object).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}

#[implement(
    ITfTextInputProcessorEx,
    ITfThreadMgrEventSink,
    ITfTextEditSink,
    ITfKeyEventSink
)]
struct TextService {
    _guard: ObjectGuard,
    state: std::sync::Mutex<ServiceState>,
    pause_session: std::sync::Arc<std::sync::Mutex<InputSession>>,
    active_request: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

struct ServiceState {
    thread_manager: Option<ITfThreadMgr>,
    client_id: u32,
    thread_cookie: u32,
    context_cookie: u32,
    context: Option<ITfContext>,
    session: InputSession,
    secure: bool,
    session_id: u64,
}

impl TextService {
    fn new() -> Self {
        Self {
            _guard: ObjectGuard::new(),
            state: std::sync::Mutex::new(ServiceState {
                thread_manager: None,
                client_id: 0,
                thread_cookie: 0,
                context_cookie: 0,
                context: None,
                session: InputSession::new(),
                secure: false,
                session_id: next_session_id(),
            }),
            pause_session: std::sync::Arc::new(std::sync::Mutex::new(InputSession::new())),
            active_request: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    fn sync_pause(&self) {
        let session = lock(&self.state).session.clone();
        self.active_request
            .store(session.request_id(), std::sync::atomic::Ordering::Release);
        if let Ok(mut pause) = self.pause_session.lock() {
            *pause = session;
        }
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(
        &self,
        thread_manager: windows::core::Ref<ITfThreadMgr>,
        client_id: u32,
    ) -> Result<()> {
        self.activate(thread_manager, client_id, 0)
    }

    fn Deactivate(&self) -> Result<()> {
        let mut state = lock(&self.state);
        deactivate_sinks(&mut state);
        state.session.reset();
        self.active_request.store(
            state.session.request_id(),
            std::sync::atomic::Ordering::Release,
        );
        suggestion::hide();
        Ok(())
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(
        &self,
        thread_manager: windows::core::Ref<ITfThreadMgr>,
        client_id: u32,
        flags: u32,
    ) -> Result<()> {
        self.activate(thread_manager, client_id, flags)
    }
}

impl TextService_Impl {
    fn activate(
        &self,
        thread_manager: windows::core::Ref<ITfThreadMgr>,
        client_id: u32,
        flags: u32,
    ) -> Result<()> {
        let thread_manager = thread_manager.ok()?;
        let secure = flags & (TF_TMAE_SECUREMODE | TF_TMF_SECUREMODE) != 0;
        let source: ITfSource = thread_manager.cast()?;
        let sink: IUnknown = self.to_interface::<ITfThreadMgrEventSink>().into();
        let cookie = unsafe { source.AdviseSink(&ITfThreadMgrEventSink::IID, &sink)? };
        let keystroke: ITfKeystrokeMgr = thread_manager.cast()?;
        let key_sink = self.to_interface::<ITfKeyEventSink>();
        unsafe { keystroke.AdviseKeyEventSink(client_id, &key_sink, true)? };
        let mut state = lock(&self.state);
        state.thread_manager = Some(thread_manager.clone());
        state.client_id = client_id;
        state.thread_cookie = cookie;
        state.secure = secure;
        if secure {
            state.session.mark_secure();
        }
        Ok(())
    }
}

impl ITfThreadMgrEventSink_Impl for TextService_Impl {
    fn OnInitDocumentMgr(&self, _document: windows::core::Ref<ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    fn OnUninitDocumentMgr(&self, _document: windows::core::Ref<ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    fn OnSetFocus(
        &self,
        focus: windows::core::Ref<ITfDocumentMgr>,
        _previous: windows::core::Ref<ITfDocumentMgr>,
    ) -> Result<()> {
        let mut state = lock(&self.state);
        unadvise_context(&mut state);
        state.session.reset();
        self.active_request.store(
            state.session.request_id(),
            std::sync::atomic::Ordering::Release,
        );
        suggestion::hide();
        if focus.is_null() {
            return Ok(());
        }
        let context = unsafe { focus.ok()?.GetTop() }.ok();
        if let Some(context) = context {
            if context_is_excluded(&context) {
                state.session.mark_secure();
                self.active_request.store(
                    state.session.request_id(),
                    std::sync::atomic::Ordering::Release,
                );
                return Ok(());
            }
            let source: ITfSource = context.cast()?;
            let sink: IUnknown = self.to_interface::<ITfTextEditSink>().into();
            let cookie = unsafe { source.AdviseSink(&ITfTextEditSink::IID, &sink)? };
            state.context = Some(context);
            state.context_cookie = cookie;
        }
        Ok(())
    }

    fn OnPushContext(&self, _context: windows::core::Ref<ITfContext>) -> Result<()> {
        Ok(())
    }

    fn OnPopContext(&self, _context: windows::core::Ref<ITfContext>) -> Result<()> {
        Ok(())
    }
}

impl ITfTextEditSink_Impl for TextService_Impl {
    fn OnEndEdit(
        &self,
        _context: windows::core::Ref<ITfContext>,
        _cookie: u32,
        _record: windows::core::Ref<ITfEditRecord>,
    ) -> Result<()> {
        // 外部编辑使当前句子失效。不要把别的程序写下的文字当成 Lucid 的输入。
        let mut state = lock(&self.state);
        state.session.invalidate();
        self.active_request.store(
            state.session.request_id(),
            std::sync::atomic::Ordering::Release,
        );
        Ok(())
    }
}

impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, foreground: BOOL) -> Result<()> {
        if !foreground.as_bool() {
            let mut state = lock(&self.state);
            state.session.invalidate();
            self.active_request.store(
                state.session.request_id(),
                std::sync::atomic::Ordering::Release,
            );
            suggestion::hide();
        }
        Ok(())
    }

    fn OnTestKeyDown(
        &self,
        context: windows::core::Ref<ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Result<BOOL> {
        Ok(self.eats_key(context, wparam, lparam).into())
    }

    fn OnTestKeyUp(
        &self,
        _context: windows::core::Ref<ITfContext>,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Result<BOOL> {
        Ok(false.into())
    }

    fn OnKeyDown(
        &self,
        context: windows::core::Ref<ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Result<BOOL> {
        let Ok(context) = context.ok() else {
            return Ok(false.into());
        };
        if context_is_excluded(&context) {
            let mut state = lock(&self.state);
            state.session.mark_secure();
            self.active_request.store(
                state.session.request_id(),
                std::sync::atomic::Ordering::Release,
            );
            return Ok(false.into());
        }
        let now = crate::win32::monotonic_millis();
        let client_id = {
            let state = lock(&self.state);
            if state.secure {
                return Ok(false.into());
            }
            state.client_id
        };
        self.flush_pause(&context, client_id, now);
        if wparam.0 == 0x09 {
            return Ok(self.confirm_suggestion(&context, client_id).into());
        }
        let action = key_action(wparam, lparam);
        let disposition = {
            let mut state = lock(&self.state);
            let disposition = state.session.handle_key(action, now);
            self.active_request.store(
                state.session.request_id(),
                std::sync::atomic::Ordering::Release,
            );
            disposition
        };
        match disposition {
            KeyDisposition::Pass => Ok(false.into()),
            KeyDisposition::Consume => Ok(true.into()),
            KeyDisposition::Edit(Edit::Insert(text)) => {
                let caret =
                    document::with_context(&context, || document::caret(&context, client_id));
                document::with_context(&context, || document::insert(&context, client_id, &text))?;
                self.after_insert(&context, client_id, &text, caret, now);
                self.sync_pause();
                self.arm_pause();
                self.flush_pause(&context, client_id, now);
                Ok(true.into())
            }
            KeyDisposition::Edit(Edit::Backspace) => {
                document::with_context(&context, || {
                    document::delete_backward(&context, client_id)
                })?;
                lock(&self.state).session.delete_backward(now);
                self.sync_pause();
                self.arm_pause();
                Ok(true.into())
            }
        }
    }

    fn OnKeyUp(
        &self,
        _context: windows::core::Ref<ITfContext>,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Result<BOOL> {
        Ok(false.into())
    }

    fn OnPreservedKey(
        &self,
        _context: windows::core::Ref<ITfContext>,
        _guid: *const GUID,
    ) -> Result<BOOL> {
        Ok(false.into())
    }
}

impl TextService_Impl {
    fn eats_key(
        &self,
        context: windows::core::Ref<ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> bool {
        let Ok(context) = context.ok() else {
            return false;
        };
        if context_is_excluded(&context) {
            return false;
        }
        let state = lock(&self.state);
        if state.secure || state.session.is_secure() {
            return false;
        }
        !matches!(key_action(wparam, lparam), KeyAction::Ignore)
    }

    fn after_insert(
        &self,
        context: &ITfContext,
        client_id: u32,
        text: &str,
        caret: Option<usize>,
        now: u64,
    ) {
        let completed = lock(&self.state).session.note_inserted(text, caret, now);
        if let Some(sentence) = completed.last() {
            self.request_sentence(
                context,
                client_id,
                &sentence.text,
                sentence.utf16_range.location,
                sentence.utf16_range.length,
            );
        }
    }

    fn request_sentence(
        &self,
        _context: &ITfContext,
        _client_id: u32,
        text: &str,
        location: usize,
        length: usize,
    ) {
        let (request_id, range, session_id, secure, active_request) = {
            let mut state = lock(&self.state);
            let range = state.session.absolute_range(location, length);
            state.session.remember_original(text, range);
            let request_id = state.session.begin_request();
            self.active_request
                .store(request_id, std::sync::atomic::Ordering::Release);
            (
                request_id,
                range,
                state.session_id,
                state.secure,
                std::sync::Arc::clone(&self.active_request),
            )
        };
        tracing::info!(chars = text.chars().count(), request_id, "发送 AI 改写请求");
        let text = text.to_owned();
        std::thread::spawn(move || {
            let message = crate::ipc::request_correction(
                session_id,
                secure,
                request_id,
                &text,
                ProtocolRange::new(range.location, range.length),
            );
            let Ok(ServerMessage::CorrectionReady {
                original,
                replacement,
                range: returned_range,
                chinese_text,
                ..
            }) = message
            else {
                return;
            };
            if active_request.load(std::sync::atomic::Ordering::Acquire) != request_id
                || original != text
                || replacement.trim().is_empty()
            {
                return;
            }
            suggestion::show(suggestion::SuggestionView {
                original: original.clone(),
                english: replacement.clone(),
                chinese: chinese_text.clone(),
                message: None,
            });
            suggestion::stage(
                original,
                replacement,
                chinese_text,
                lucid_core::Utf16Range::new(returned_range.location, returned_range.length),
                request_id,
            );
        });
    }

    fn arm_pause(&self) {
        let pause = std::sync::Arc::clone(&self.pause_session);
        let started = lock(&self.state).session.request_id();
        let (session_id, secure, active_request) = {
            let state = lock(&self.state);
            (
                state.session_id,
                state.secure,
                std::sync::Arc::clone(&self.active_request),
            )
        };
        std::thread::spawn(move || {
            std::thread::sleep(crate::session::PAUSE);
            let Some(sentence) = (|| {
                let mut session = pause.lock().unwrap_or_else(|poison| poison.into_inner());
                if session.request_id() != started {
                    return None;
                }
                session.take_pause_sentence(crate::win32::monotonic_millis())
            })() else {
                return;
            };
            let (range, request_id) = {
                let mut session = pause.lock().unwrap_or_else(|poison| poison.into_inner());
                let range = session
                    .absolute_range(sentence.utf16_range.location, sentence.utf16_range.length);
                session.remember_original(&sentence.text, range);
                (range, session.request_id())
            };
            let Ok(ServerMessage::CorrectionReady {
                original,
                replacement,
                range: returned_range,
                chinese_text,
                ..
            }) = crate::ipc::request_correction(
                session_id,
                secure,
                request_id,
                &sentence.text,
                ProtocolRange::new(range.location, range.length),
            )
            else {
                return;
            };
            if active_request.load(std::sync::atomic::Ordering::Acquire) != request_id
                || original != sentence.text
                || replacement.trim().is_empty()
            {
                return;
            }
            suggestion::show(suggestion::SuggestionView {
                original: original.clone(),
                english: replacement.clone(),
                chinese: chinese_text.clone(),
                message: None,
            });
            suggestion::stage(
                original,
                replacement,
                chinese_text,
                lucid_core::Utf16Range::new(returned_range.location, returned_range.length),
                request_id,
            );
        });
    }

    fn flush_pause(&self, context: &ITfContext, client_id: u32, now: u64) {
        let sentence = {
            let mut state = lock(&self.state);
            if !state.session.pause_due(now) {
                return;
            }
            state.session.take_pause_sentence(now)
        };
        let Some(sentence) = sentence else {
            return;
        };
        self.request_sentence(
            context,
            client_id,
            &sentence.text,
            sentence.utf16_range.location,
            sentence.utf16_range.length,
        );
    }

    fn confirm_suggestion(&self, context: &ITfContext, client_id: u32) -> bool {
        let Some(staged) = suggestion::take_staged() else {
            return false;
        };
        if lock(&self.state).session.request_id() != staged.request_id {
            suggestion::show_failure("建议已过期，未替换。");
            return true;
        }
        {
            let mut state = lock(&self.state);
            let request_id = state.session.request_id();
            state
                .session
                .remember_original(&staged.original, staged.range);
            let _ = state.session.accept_result(
                request_id,
                &staged.original,
                &staged.replacement,
                staged.chinese.clone(),
                staged.range,
            );
        }
        let outcome = document::with_context(context, || {
            document::replace_validated(
                context,
                client_id,
                &staged.original,
                &staged.replacement,
                staged.range,
            )
        });
        match outcome {
            lucid_core::ReplacementOutcome::Replaced => {
                lock(&self.state).session.dismiss_suggestion();
                suggestion::hide();
                let _ = crate::ipc::send_control(&ClientMessage::AcceptSuggestion {
                    session_id: lock(&self.state).session_id,
                    request_id: staged.request_id,
                });
                tracing::info!("已按确认替换原文");
                true
            }
            _ => {
                suggestion::show_failure("原文已变化或无法校验，未替换。");
                true
            }
        }
    }
}

fn next_session_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn key_action(wparam: WPARAM, lparam: LPARAM) -> KeyAction {
    let virtual_key = wparam.0 as u32;
    let scan = ((lparam.0 as u32) >> 16) & 0xff;
    let character = crate::win32::character_from_key(virtual_key, scan);
    keys::action_for_virtual_key(virtual_key, character, crate::win32::control_is_down())
}

fn context_is_excluded(context: &ITfContext) -> bool {
    let Ok(status) = (unsafe { context.GetStatus() }) else {
        return true;
    };
    status.dwStaticFlags & TS_SS_TRANSITORY != 0 || status.dwDynamicFlags & TS_SD_READONLY != 0
}

fn deactivate_sinks(state: &mut ServiceState) {
    if let Some(thread_manager) = state.thread_manager.take() {
        if state.thread_cookie != 0
            && let Ok(source) = thread_manager.cast::<ITfSource>()
        {
            unsafe {
                let _ = source.UnadviseSink(state.thread_cookie);
            }
        }
        if let Ok(keystroke) = thread_manager.cast::<ITfKeystrokeMgr>() {
            unsafe {
                let _ = keystroke.UnadviseKeyEventSink(state.client_id);
            }
        }
    }
    unadvise_context(state);
    state.client_id = 0;
    state.thread_cookie = 0;
}

fn unadvise_context(state: &mut ServiceState) {
    if let Some(context) = state.context.take()
        && state.context_cookie != 0
        && let Ok(source) = context.cast::<ITfSource>()
    {
        unsafe {
            let _ = source.UnadviseSink(state.context_cookie);
        }
    }
    state.context_cookie = 0;
}

fn lock(state: &std::sync::Mutex<ServiceState>) -> std::sync::MutexGuard<'_, ServiceState> {
    state.lock().unwrap_or_else(|poison| poison.into_inner())
}

impl Drop for TextService {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            deactivate_sinks(&mut state);
        }
    }
}
