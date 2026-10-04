//! 句末状态机：标点结束句子，小数点要再看一个字符。

use crate::text::{is_terminator, utf16_len};

/// 一个已经结束、可以交给 AI 的句子。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletedSentence {
    pub text: String,

    /// 相对本次会话起点的 UTF-16 范围，不含句前空白。
    pub utf16_range: Utf16Span,

    pub version: u64,
}

/// 只在句子模块内部使用的范围，避免和宿主绝对范围混用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Utf16Span {
    pub location: usize,
    pub length: usize,
}

/// 跟踪当前输入法会话收到的文本。调用方自己持有停顿计时器。
#[derive(Clone, Debug, Default)]
pub struct SentenceTracker {
    pending_text: String,
    version: u64,
    committed_utf16_length: usize,
    period_awaiting_lookahead: bool,
}

impl SentenceTracker {
    pub const fn new() -> Self {
        Self {
            pending_text: String::new(),
            version: 0,
            committed_utf16_length: 0,
            period_awaiting_lookahead: false,
        }
    }

    pub fn pending_text(&self) -> &str {
        &self.pending_text
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// 记录已经进入宿主的文本，返回被明确标点或换行结束的句子。
    pub fn append(&mut self, text: &str) -> Vec<CompletedSentence> {
        let mut completed = Vec::new();
        for character in text.chars() {
            if self.period_awaiting_lookahead {
                self.period_awaiting_lookahead = false;
                if character.is_ascii_digit() {
                    self.pending_text.push(character);
                    self.version = self.version.wrapping_add(1);
                    continue;
                }
                if let Some(sentence) = self.finish_pending() {
                    completed.push(sentence);
                }
            }

            let previous = self.pending_text.chars().next_back();
            self.pending_text.push(character);
            self.version = self.version.wrapping_add(1);

            if character == '.' && previous.is_some_and(|item| item.is_ascii_digit()) {
                self.period_awaiting_lookahead = true;
                continue;
            }
            if is_terminator(character)
                && let Some(sentence) = self.finish_pending()
            {
                completed.push(sentence);
            }
        }
        completed
    }

    /// 停顿后结束当前句子。小数点若还在等下一个字符，也在这里收束。
    pub fn flush_on_pause(&mut self) -> Option<CompletedSentence> {
        self.period_awaiting_lookahead = false;
        self.finish_pending()
    }

    pub fn invalidate_pending_text(&mut self) {
        self.pending_text.clear();
        self.period_awaiting_lookahead = false;
        self.version = self.version.wrapping_add(1);
    }

    pub fn reset(&mut self) {
        self.pending_text.clear();
        self.committed_utf16_length = 0;
        self.period_awaiting_lookahead = false;
        self.version = self.version.wrapping_add(1);
    }

    pub fn delete_backward(&mut self) {
        if self.pending_text.pop().is_none() {
            return;
        }
        self.period_awaiting_lookahead = false;
        self.version = self.version.wrapping_add(1);
    }

    fn finish_pending(&mut self) -> Option<CompletedSentence> {
        let sentence = self.pending_text.trim().to_owned();
        let leading_whitespace = self
            .pending_text
            .chars()
            .take_while(|character| character.is_whitespace())
            .collect::<String>();
        let location = self.committed_utf16_length + utf16_len(&leading_whitespace);
        let length = utf16_len(&sentence);
        self.committed_utf16_length += utf16_len(&self.pending_text);
        self.pending_text.clear();
        if sentence.is_empty() {
            return None;
        }
        Some(CompletedSentence {
            text: sentence,
            utf16_range: Utf16Span { location, length },
            version: self.version,
        })
    }
}
