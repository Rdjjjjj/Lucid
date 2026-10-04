//! 停顿是否可以当成“这句话写完了”。不确定时继续等，避免半句话就去请求 AI。

const COMPLETE_SHORT_ENDINGS: &[&str] = &[
    "me", "it", "us", "ok", "no", "yes", "up", "out", "now", "here", "there", "too", "him", "her",
    "them", "one", "all", "off", "on", "in", "go",
];

const DANGLING_LAST_TOKENS: &[&str] = &[
    "a", "an", "the", "to", "for", "of", "and", "or", "but", "if", "when", "while", "that", "this",
    "these", "those", "my", "your", "our", "their", "his", "her", "its", "i", "im", "i'm", "ive",
    "i've", "id", "i'd", "ill", "i'll", "youre", "you're", "we", "they", "he", "she", "am", "is",
    "are", "was", "were", "be", "been", "being", "will", "would", "can", "could", "should", "may",
    "might", "must", "shall", "want", "wanna", "need", "like", "at", "in", "on", "with", "from",
    "about", "into", "onto", "as", "than", "then", "also", "just", "very", "so", "because", "have",
    "has", "had", "do", "does", "did", "dont", "don't", "gonna", "going", "let", "lets", "let's",
    "please", "not", "too", "more", "most", "some", "any", "each", "every", "which", "whom",
    "whose", "wo", "yao", "xiang", "gei", "zai", "ba", "bei", "de", "le",
];

pub struct SentenceCompletion;

impl SentenceCompletion {
    pub fn looks_finished(text: &str) -> bool {
        let draft = text.trim();
        if draft.is_empty() {
            return false;
        }
        if draft.ends_with([',', '，', ';', ':', '：', '-', '—', '…']) || draft.ends_with("...")
        {
            return false;
        }
        if draft
            .chars()
            .next_back()
            .is_some_and(|character| ".!?。？！".contains(character))
        {
            return true;
        }
        let tokens = tokens(draft);
        if tokens.len() < 3 {
            return false;
        }
        let letters = draft
            .chars()
            .filter(|character| character.is_alphabetic())
            .count();
        if letters < 8 {
            return false;
        }
        let Some(last) = tokens.last() else {
            return false;
        };
        let last = last.trim_matches(['\'', '"', '“', '”', '‘', '’']);
        if last.is_empty() {
            return false;
        }
        let normalized = last.to_lowercase();
        if DANGLING_LAST_TOKENS.contains(&normalized.as_str()) {
            return false;
        }
        if normalized.chars().count() < 4 && !COMPLETE_SHORT_ENDINGS.contains(&normalized.as_str())
        {
            return false;
        }
        true
    }
}

fn tokens(text: &str) -> Vec<&str> {
    text.split(|character: char| character.is_whitespace() || character == '/')
        .filter(|token| !token.is_empty())
        .collect()
}
