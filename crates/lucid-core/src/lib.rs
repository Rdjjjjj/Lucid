//! Lucid 的平台无关核心。
//!
//! 句末判断、已提交文本的范围恢复、替换校验和 AI 协议适配都在这里。
//! 平台壳只负责把系统按键译成这里的调用，再把建议画出来。
//! 判断标准：把 macOS 的 IMK 换成别的输入法框架，不应该改这个 crate 的任何一行。

pub mod ai;
pub mod completion;
pub mod config;
pub mod prompt;
pub mod replacement;
pub mod sentence;
pub mod text;

pub use ai::{
    AiClientError, AiConfiguration, AiProtocol, CorrectionRequest, CorrectionResult,
    CorrectionService,
};
pub use completion::SentenceCompletion;
pub use config::SettingsStore;
pub use prompt::CLEANUP_INSTRUCTION;
pub use replacement::{CommittedTextReplacement, HostClient, ReplacementOutcome, Utf16Range};
pub use sentence::{CompletedSentence, SentenceTracker};
