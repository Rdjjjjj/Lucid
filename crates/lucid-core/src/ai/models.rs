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
    pub chinese_text: Option<String>,
}

impl CorrectionResult {
    pub fn new(raw: impl Into<String>) -> Self {
        let text = raw.into();
        if let Some((en, zh)) = text.split_once("|||") {
            let en_clean = en.trim().to_owned();
            let zh_clean = zh.trim().to_owned();
            Self {
                corrected_text: en_clean,
                chinese_text: if zh_clean.is_empty() { None } else { Some(zh_clean) },
            }
        } else {
            Self {
                corrected_text: text.trim().to_owned(),
                chinese_text: None,
            }
        }
    }

    pub fn with_chinese(corrected_text: impl Into<String>, chinese_text: Option<String>) -> Self {
        Self {
            corrected_text: corrected_text.into(),
            chinese_text,
        }
    }
}
