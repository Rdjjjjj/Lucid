//! 一个输入会话的状态。每个文本框一份，焦点离开就丢掉。

use std::time::{Duration, Instant};

use lucid_core::{CompletedSentence, SentenceCompletion, SentenceTracker, Utf16Range};

pub const PAUSE: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct PendingSuggestion {
    pub original: String,
    pub replacement: String,
    pub range: Option<Utf16Range>,
    pub version: u64,
}

#[derive(Debug)]
pub struct InputSession {
    tracker: SentenceTracker,
    request_id: u64,
    /// Absolute UTF-16 offset where this input session started tracking text.
    /// IMK ranges are document-relative; the core tracker is session-relative.
    document_origin: Option<usize>,
    sentence_origin: usize,
    tracked_origin: usize,
    pending_range: Option<Utf16Range>,
    pending_selection: Option<Utf16Range>,
    pending_original: Option<String>,
    last_key: String,
    last_key_code: i32,
    last_key_at: Option<Instant>,
    pause_deadline: Option<Instant>,
}

impl InputSession {
    pub fn new() -> Self {
        Self {
            tracker: SentenceTracker::new(),
            request_id: 0,
            document_origin: None,
            sentence_origin: 0,
            tracked_origin: 0,
            pending_range: None,
            pending_selection: None,
            pending_original: None,
            last_key: String::new(),
            last_key_code: i32::MIN,
            last_key_at: None,
            pause_deadline: None,
        }
    }

    pub fn reset(&mut self) {
        self.tracker.reset();
        self.request_id = self.request_id.wrapping_add(1);
        self.document_origin = None;
        self.sentence_origin = 0;
        self.tracked_origin = 0;
        self.pending_range = None;
        self.pending_selection = None;
        self.pending_original = None;
        self.pause_deadline = None;
    }

    pub fn invalidate_request(&mut self) -> u64 {
        self.request_id = self.request_id.wrapping_add(1);
        self.request_id
    }

    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    pub fn next_request(&mut self) -> u64 {
        self.request_id = self.request_id.wrapping_add(1);
        self.request_id
    }

    pub fn pending_text(&self) -> &str {
        self.tracker.pending_text()
    }

    pub fn delete_backward(&mut self) {
        if let Some(last) = self.tracker.pending_text().chars().next_back() {
            self.tracker.delete_backward();
            self.tracked_origin = self.tracked_origin.saturating_sub(last.len_utf16());
        }
        self.pause_deadline = Some(Instant::now() + PAUSE);
    }

    pub fn note_inserted(
        &mut self,
        text: &str,
        selected_before_insert: Option<Utf16Range>,
    ) -> Vec<CompletedSentence> {
        if self.tracker.pending_text().is_empty() {
            self.sentence_origin = self.tracked_origin;
            if let Some(selected) = selected_before_insert {
                let insertion_start = if selected.length == 0 {
                    selected.location
                } else {
                    selected.location
                };
                self.document_origin = Some(insertion_start.saturating_sub(self.tracked_origin));
            }
        }
        self.tracked_origin += text.encode_utf16().count();
        self.pending_selection = Some(Utf16Range::new(self.absolute_caret(), 0));
        let completed = self.tracker.append(text);
        self.pause_deadline = Some(Instant::now() + PAUSE);
        completed
    }

    pub fn absolute_range(&self, relative: Utf16Range) -> Utf16Range {
        Utf16Range::new(
            self.document_origin
                .unwrap_or(0)
                .saturating_add(relative.location),
            relative.length,
        )
    }

    fn absolute_caret(&self) -> usize {
        self.document_origin
            .unwrap_or(0)
            .saturating_add(self.tracked_origin)
    }

    pub fn remember_original(&mut self, original: &str, range: Option<Utf16Range>) {
        self.pending_original = Some(original.to_owned());
        let length = original.encode_utf16().count();
        let location = range
            .filter(|range| range.location != usize::MAX)
            .map(|range| range.location)
            .unwrap_or(self.sentence_origin);
        self.pending_range = Some(Utf16Range::new(location, length));
        self.sentence_origin = location + length;
    }

    pub fn pending_original(&self) -> Option<&str> {
        self.pending_original.as_deref()
    }

    pub fn pending_range(&self) -> Option<Utf16Range> {
        self.pending_range
    }

    pub fn pending_selection(&self) -> Option<Utf16Range> {
        self.pending_selection
    }

    pub fn clear_pending(&mut self) {
        self.pending_original = None;
        self.pending_range = None;
        self.pending_selection = None;
        self.document_origin = None;
        self.sentence_origin = 0;
        self.tracked_origin = 0;
        self.tracker.reset();
    }

    pub fn should_ignore_duplicate(&mut self, text: &str, key_code: i32) -> bool {
        let now = Instant::now();
        let duplicate = self.last_key == text
            && self.last_key_code == key_code
            && self
                .last_key_at
                .is_some_and(|at| now.duration_since(at) < Duration::from_millis(30));
        self.last_key = text.to_owned();
        self.last_key_code = key_code;
        self.last_key_at = Some(now);
        duplicate
    }

    pub fn take_pause_sentence(&mut self) -> Option<CompletedSentence> {
        let deadline = self.pause_deadline.take()?;
        if Instant::now() < deadline {
            self.pause_deadline = Some(deadline);
            return None;
        }
        let sentence = self.tracker.flush_on_pause()?;
        if SentenceCompletion::looks_finished(&sentence.text) {
            Some(sentence)
        } else {
            None
        }
    }

    pub fn cancel_pause(&mut self) {
        self.pause_deadline = None;
    }
}

impl Default for InputSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nihao_period_triggers_exactly_one_sentence() {
        let mut session = InputSession::new();
        for (offset, letter) in ["n", "i", "h", "a", "o"].iter().enumerate() {
            assert!(
                session
                    .note_inserted(letter, Some(Utf16Range::new(offset, 0)))
                    .is_empty()
            );
        }
        let completed = session.note_inserted(".", Some(Utf16Range::new(5, 0)));
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].text, "nihao.");
        assert_eq!(completed[0].utf16_range.length, 6);
    }

    #[test]
    fn ranges_are_absolute_when_typing_into_existing_document() {
        let mut session = InputSession::new();
        let completed =
            session.note_inserted("I want to mai coffee.", Some(Utf16Range::new(40, 0)));
        assert_eq!(completed.len(), 1);
        let sentence = &completed[0];
        assert_eq!(
            session.absolute_range(Utf16Range::new(
                sentence.utf16_range.location,
                sentence.utf16_range.length
            )),
            Utf16Range::new(40, 21)
        );
    }

    #[test]
    fn delete_backward_accounts_for_utf16_units() {
        let mut session = InputSession::new();
        session.note_inserted("a😀", Some(Utf16Range::new(10, 0)));
        session.delete_backward();
        let completed = session.note_inserted(".", None);
        assert_eq!(completed[0].utf16_range.location, 0);
        assert_eq!(completed[0].utf16_range.length, 2);
        assert_eq!(
            session.absolute_range(Utf16Range::new(
                completed[0].utf16_range.location,
                completed[0].utf16_range.length
            )),
            Utf16Range::new(10, 2)
        );
    }
}
