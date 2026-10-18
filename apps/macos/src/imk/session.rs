//! 一个输入会话的状态。每个文本框一份，焦点离开就丢掉。

use std::time::{Duration, Instant};

use lucid_core::{CompletedSentence, SentenceCompletion, SentenceTracker, Utf16Range};

pub const PAUSE: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub struct InputSession {
    tracker: SentenceTracker,
    /// Absolute UTF-16 offset where this input session started tracking text.
    /// IMK ranges are document-relative; the core tracker is session-relative.
    document_origin: Option<usize>,
    sentence_origin: usize,
    tracked_origin: usize,
    pending_range: Option<Utf16Range>,
    pending_original: Option<String>,
    pause_deadline: Option<Instant>,
}

impl InputSession {
    pub fn new() -> Self {
        Self {
            tracker: SentenceTracker::new(),
            document_origin: None,
            sentence_origin: 0,
            tracked_origin: 0,
            pending_range: None,
            pending_original: None,
            pause_deadline: None,
        }
    }

    pub fn reset(&mut self) {
        self.tracker.reset();
        self.document_origin = None;
        self.sentence_origin = 0;
        self.tracked_origin = 0;
        self.pending_range = None;
        self.pending_original = None;
        self.pause_deadline = None;
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

    /// Forget only the completed sentence that can still be replaced. The
    /// active tracker remains intact so pressing Escape while the user is
    /// still typing does not lose the sentence under construction.
    pub fn clear_pending_suggestion(&mut self) {
        self.pending_original = None;
        self.pending_range = None;
    }

    pub fn clear_pending(&mut self) {
        self.clear_pending_suggestion();
        self.document_origin = None;
        self.sentence_origin = 0;
        self.tracked_origin = 0;
        self.tracker.reset();
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

    /// 清除 tracker 中正在追踪的暂存文本，并取消停顿计时器。
    /// 当从宿主直接读回完整句子并完成收束时调用，避免上一句残留在 tracker 中。
    pub fn reset_tracker(&mut self) {
        self.tracker.reset();
        self.tracked_origin = 0;
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
    fn clearing_pending_suggestion_keeps_active_sentence() {
        let mut session = InputSession::new();
        session.note_inserted("hi", Some(Utf16Range::new(4, 0)));
        session.remember_original("hi", Some(Utf16Range::new(4, 2)));
        session.clear_pending_suggestion();

        assert!(session.pending_original.is_none());
        assert!(session.pending_range.is_none());
        let completed = session.note_inserted(".", Some(Utf16Range::new(6, 0)));
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].text, "hi.");
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
