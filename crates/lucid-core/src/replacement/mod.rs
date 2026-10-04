//! 已经写入宿主的原文怎么定位、怎么替换。
//!
//! 宿主对范围的理解并不一致：有的光标落后一个字符，有的忽略 `replacementRange`，
//! 有的在建议面板抢走焦点后把光标报成 0。这里只认读回来的文本，读不回来就不写。

mod range;
mod replace;

pub use range::{
    preferred_range, range_behind_caret, range_of_original, range_searching_backwards,
    sentence_before_cursor, sentence_on_current_line,
};
pub use replace::{CommittedTextReplacement, HostClient, ReplacementOutcome};

/// 宿主文档里的 UTF-16 范围。`None` 表示宿主没有给出可用位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Utf16Range {
    pub location: usize,
    pub length: usize,
}

impl Utf16Range {
    pub const fn new(location: usize, length: usize) -> Self {
        Self { location, length }
    }

    pub const fn end(self) -> usize {
        self.location.saturating_add(self.length)
    }
}
