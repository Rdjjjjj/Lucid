//! 一次整理请求和模型返回的句子。句子正文不进入日志。

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorrectionRequest {
    pub sentence: String,
}

impl CorrectionRequest {
    pub fn new(sentence: impl Into<String>) -> Self {
        Self {
            sentence: sentence.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorrectionResult {
    pub corrected_text: String,
}

impl CorrectionResult {
    pub fn new(corrected_text: impl Into<String>) -> Self {
        Self {
            corrected_text: corrected_text.into(),
        }
    }
}
