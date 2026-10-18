//! 一个 TSF 上下文的输入会话。焦点离开后必须丢掉，不能拿去改另一个文档。

use std::time::Duration;

use lucid_core::{CompletedSentence, SentenceCompletion, SentenceTracker, Utf16Range};

use crate::keys::{self, KeyAction};

pub const PAUSE: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingSuggestion {
    pub original: String,
    pub replacement: String,
    pub chinese: Option<String>,
    pub range: Utf16Range,
    pub request_id: u64,
}

#[derive(Clone, Debug)]
pub struct InputSession {
    tracker: SentenceTracker,
    request_id: u64,
    document_origin: Option<usize>,
    sentence_origin: usize,
    tracked_origin: usize,
    pending_original: Option<String>,
    pending_range: Option<Utf16Range>,
    suggestion: Option<PendingSuggestion>,
    last_key: String,
    last_virtual_key: u32,
    last_key_at_ms: Option<u64>,
    pause_deadline_ms: Option<u64>,
    secure: bool,
}

impl Default for InputSession {
    fn default() -> Self {
        Self::new()
    }
}

impl InputSession {
    pub const fn new() -> Self {
        Self {
            tracker: SentenceTracker::new(),
            request_id: 0,
            document_origin: None,
            sentence_origin: 0,
            tracked_origin: 0,
            pending_original: None,
            pending_range: None,
            suggestion: None,
            last_key: String::new(),
            last_virtual_key: 0,
            last_key_at_ms: None,
            pause_deadline_ms: None,
            secure: false,
        }
    }

    pub fn reset(&mut self) {
        *self = Self {
            request_id: self.request_id.wrapping_add(1),
            ..Self::new()
        };
    }

    pub fn invalidate(&mut self) -> u64 {
        self.request_id = self.request_id.wrapping_add(1);
        self.suggestion = None;
        self.pause_deadline_ms = None;
        self.request_id
    }

    /// 让正在返回的 AI 结果失效，但保留当前句子的停顿计时。
    pub fn invalidate_pending(&mut self) -> u64 {
        self.request_id = self.request_id.wrapping_add(1);
        self.suggestion = None;
        self.request_id
    }

    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    pub fn mark_secure(&mut self) {
        self.secure = true;
        self.invalidate();
        self.tracker.reset();
    }

    pub fn is_secure(&self) -> bool {
        self.secure
    }

    pub fn pending_text(&self) -> &str {
        self.tracker.pending_text()
    }

    pub fn suggestion(&self) -> Option<&PendingSuggestion> {
        self.suggestion.as_ref()
    }

    pub fn take_suggestion(&mut self) -> Option<PendingSuggestion> {
        self.suggestion.take()
    }

    pub fn dismiss_suggestion(&mut self) {
        self.suggestion = None;
    }

    /// 记录宿主已经收下的文字。返回被标点明确结束的句子。
    pub fn note_inserted(
        &mut self,
        text: &str,
        caret_before: Option<usize>,
        now_ms: u64,
    ) -> Vec<CompletedSentence> {
        self.invalidate_pending();
        if self.tracker.pending_text().is_empty() {
            self.sentence_origin = self.tracked_origin;
            if let Some(caret) = caret_before {
                self.document_origin = Some(caret.saturating_sub(self.tracked_origin));
            }
        }
        self.tracked_origin += text.encode_utf16().count();
        let completed = self.tracker.append(text);
        self.pause_deadline_ms = Some(now_ms.saturating_add(PAUSE.as_millis() as u64));
        self.suggestion = None;
        completed
    }

    pub fn delete_backward(&mut self, now_ms: u64) {
        self.invalidate_pending();
        if let Some(last) = self.tracker.pending_text().chars().next_back() {
            self.tracker.delete_backward();
            self.tracked_origin = self.tracked_origin.saturating_sub(last.len_utf16());
        }
        self.pause_deadline_ms = Some(now_ms.saturating_add(PAUSE.as_millis() as u64));
        self.suggestion = None;
    }

    pub fn absolute_range(&self, location: usize, length: usize) -> Utf16Range {
        Utf16Range::new(
            self.document_origin.unwrap_or(0).saturating_add(location),
            length,
        )
    }

    pub fn remember_original(&mut self, original: &str, range: Utf16Range) {
        self.pending_original = Some(original.to_owned());
        self.pending_range = Some(range);
        self.sentence_origin = range.end();
    }

    pub fn pending_original(&self) -> Option<&str> {
        self.pending_original.as_deref()
    }

    pub fn pending_range(&self) -> Option<Utf16Range> {
        self.pending_range
    }

    pub fn should_ignore_duplicate(&mut self, text: &str, virtual_key: u32, now_ms: u64) -> bool {
        let duplicate = keys::is_duplicate(
            self.last_key_at_ms
                .map(|at| (self.last_key.clone(), self.last_virtual_key, at)),
            text,
            virtual_key,
            now_ms,
        );
        self.last_key = text.to_owned();
        self.last_virtual_key = virtual_key;
        self.last_key_at_ms = Some(now_ms);
        duplicate
    }

    pub fn cancel_pause(&mut self) {
        self.pause_deadline_ms = None;
    }

    pub fn pause_due(&self, now_ms: u64) -> bool {
        self.pause_deadline_ms
            .is_some_and(|deadline| now_ms >= deadline)
    }

    pub fn take_pause_sentence(&mut self, now_ms: u64) -> Option<CompletedSentence> {
        let deadline = self.pause_deadline_ms.take()?;
        if now_ms < deadline {
            self.pause_deadline_ms = Some(deadline);
            return None;
        }
        let sentence = self.tracker.flush_on_pause()?;
        if SentenceCompletion::looks_finished(&sentence.text) {
            Some(sentence)
        } else {
            None
        }
    }

    pub fn begin_request(&mut self) -> u64 {
        self.request_id = self.request_id.wrapping_add(1);
        self.request_id
    }

    pub fn accept_result(
        &mut self,
        request_id: u64,
        original: &str,
        replacement: &str,
        chinese: Option<String>,
        range: Utf16Range,
    ) -> bool {
        if request_id != self.request_id || self.secure || replacement.is_empty() {
            return false;
        }
        if self.pending_original.as_deref() != Some(original) {
            return false;
        }
        self.suggestion = Some(PendingSuggestion {
            original: original.to_owned(),
            replacement: replacement.to_owned(),
            chinese,
            range,
            request_id,
        });
        true
    }

    pub fn handle_key(&mut self, action: KeyAction, now_ms: u64) -> KeyDisposition {
        if self.secure {
            return KeyDisposition::Pass;
        }
        match action {
            KeyAction::Ignore => KeyDisposition::Pass,
            KeyAction::Escape => {
                self.suggestion = None;
                self.invalidate();
                KeyDisposition::Pass
            }
            KeyAction::Return => {
                self.reset();
                KeyDisposition::Pass
            }
            KeyAction::Backspace => {
                self.cancel_pause();
                KeyDisposition::Edit(Edit::Backspace)
            }
            KeyAction::Insert(character) => {
                let text = character.to_string();
                self.cancel_pause();
                if self.should_ignore_duplicate(&text, character as u32, now_ms) {
                    KeyDisposition::Consume
                } else {
                    KeyDisposition::Edit(Edit::Insert(text))
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyDisposition {
    Pass,
    Consume,
    Edit(Edit),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    Insert(String),
    Backspace,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_completes_one_sentence_at_the_document_caret() {
        let mut session = InputSession::new();
        for (offset, letter) in ["n", "i", "h", "a", "o"].iter().enumerate() {
            assert!(
                session
                    .note_inserted(letter, Some(offset), offset as u64)
                    .is_empty()
            );
        }
        let completed = session.note_inserted(".", Some(5), 6);
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].text, "nihao.");
        assert_eq!(
            session.absolute_range(
                completed[0].utf16_range.location,
                completed[0].utf16_range.length
            ),
            Utf16Range::new(0, 6)
        );
    }

    #[test]
    fn existing_document_keeps_an_absolute_utf16_range() {
        let mut session = InputSession::new();
        let completed = session.note_inserted("a😀.", Some(40), 0);
        assert_eq!(completed.len(), 1);
        assert_eq!(
            session.absolute_range(
                completed[0].utf16_range.location,
                completed[0].utf16_range.length
            ),
            Utf16Range::new(40, 4)
        );
    }

    #[test]
    fn pause_waits_until_the_deadline_and_rejects_a_dangling_phrase() {
        let mut session = InputSession::new();
        session.note_inserted("I want to ", None, 0);
        assert!(session.take_pause_sentence(1_999).is_none());
        assert!(session.pause_due(2_000));
        assert!(session.take_pause_sentence(2_000).is_none());
    }

    #[test]
    fn stale_result_does_not_replace_the_current_sentence() {
        let mut session = InputSession::new();
        let completed = session.note_inserted("nihao.", None, 0);
        let range = session.absolute_range(
            completed[0].utf16_range.location,
            completed[0].utf16_range.length,
        );
        session.remember_original("nihao.", range);
        let request = session.begin_request();
        session.note_inserted("x", None, 10);
        let newer = session.begin_request();
        assert!(!session.accept_result(request, "nihao.", "Hello.", None, range));
        session.remember_original("nihao.", range);
        assert!(session.accept_result(newer, "nihao.", "Hello.", Some("你好。".to_owned()), range));
        assert_eq!(session.suggestion().unwrap().replacement, "Hello.");
    }

    #[test]
    fn duplicate_key_is_consumed_without_a_second_insert() {
        let mut session = InputSession::new();
        assert_eq!(
            session.handle_key(KeyAction::Insert('a'), 1_000),
            KeyDisposition::Edit(Edit::Insert("a".to_owned()))
        );
        assert_eq!(
            session.handle_key(KeyAction::Insert('a'), 1_010),
            KeyDisposition::Consume
        );
    }

    #[test]
    fn secure_context_never_tracks_text() {
        let mut session = InputSession::new();
        session.mark_secure();
        assert_eq!(
            session.handle_key(KeyAction::Insert('a'), 0),
            KeyDisposition::Pass
        );
        assert!(session.pending_text().is_empty());
        assert!(!session.accept_result(1, "secret", "Secret", None, Utf16Range::new(0, 6)));
    }
}
