//! 当前输入会话里的句子缓冲。
//!
//! 只记录这次输入法会话收到的文本。焦点离开后调用方必须丢掉它，不能拿去改另一个窗口。

mod tracker;

pub use tracker::{CompletedSentence, SentenceTracker};
