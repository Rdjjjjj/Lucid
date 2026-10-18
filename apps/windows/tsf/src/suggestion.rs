//! 建议卡片只显示结果。确认前不改文档，失败时保留原文。

use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuggestionView {
    pub original: String,
    pub english: String,
    pub chinese: Option<String>,
    pub message: Option<String>,
}

fn slot() -> &'static Mutex<Option<SuggestionView>> {
    static SLOT: OnceLock<Mutex<Option<SuggestionView>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

pub fn show(view: SuggestionView) {
    if let Ok(mut current) = slot().lock() {
        *current = Some(view);
    }
}

pub fn hide() {
    if let Ok(mut current) = slot().lock() {
        *current = None;
    }
}

pub fn show_failure(message: &str) {
    show(SuggestionView {
        original: String::new(),
        english: String::new(),
        chinese: None,
        message: Some(message.to_owned()),
    });
}

#[derive(Clone, Debug)]
pub struct StagedReplacement {
    pub original: String,
    pub replacement: String,
    pub chinese: Option<String>,
    pub range: lucid_core::Utf16Range,
    pub request_id: u64,
}

fn staged() -> &'static Mutex<Option<StagedReplacement>> {
    static STAGED: OnceLock<Mutex<Option<StagedReplacement>>> = OnceLock::new();
    STAGED.get_or_init(|| Mutex::new(None))
}

pub fn stage(
    original: String,
    replacement: String,
    chinese: Option<String>,
    range: lucid_core::Utf16Range,
    request_id: u64,
) {
    if let Ok(mut slot) = staged().lock() {
        *slot = Some(StagedReplacement {
            original,
            replacement,
            chinese,
            range,
            request_id,
        });
    }
}

pub fn take_staged() -> Option<StagedReplacement> {
    staged().lock().ok().and_then(|mut slot| slot.take())
}
