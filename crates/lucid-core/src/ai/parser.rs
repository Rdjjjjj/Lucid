//! 从两种兼容协议的响应里取出英文句子。模型不一定返回合法 JSON。

use serde_json::Value;

const IGNORED_KEYS: &[&str] = &[
    "reasoning_content",
    "reasoning",
    "thinking",
    "thought",
    "reasoning_tokens",
    "id",
    "object",
    "created",
    "model",
    "usage",
    "system_fingerprint",
    "finish_reason",
    "index",
    "logprobs",
    "role",
    "refusal",
];

const PREFERRED_KEYS: &[&str] = &[
    "content",
    "text",
    "output_text",
    "corrected_text",
    "correctedText",
    "output",
];

pub struct ResponseParser;

impl ResponseParser {
    pub fn sentence(data: &[u8]) -> Option<String> {
        if let Ok(value) = serde_json::from_slice::<Value>(data)
            && let Some(text) = extract_text(&value)
        {
            return Some(text);
        }
        let raw = String::from_utf8_lossy(data);
        cleaned_sentence(&raw)
    }

    pub fn model_ids(data: &[u8]) -> Option<Vec<String>> {
        let value = serde_json::from_slice::<Value>(data).ok()?;
        let object = value.as_object()?;
        for key in ["data", "models"] {
            if let Some(list) = object.get(key).and_then(Value::as_array) {
                let ids = list
                    .iter()
                    .filter_map(|item| item.get("id").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                if !ids.is_empty() {
                    return Some(ids);
                }
            }
        }
        None
    }
}

fn extract_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => cleaned_sentence(text),
        Value::Array(items) => {
            let joined = items
                .iter()
                .filter_map(|item| match item {
                    Value::String(text) => Some(text.clone()),
                    Value::Object(_) => item
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| {
                            item.get("content")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                        })
                        .or_else(|| extract_text(item)),
                    _ => None,
                })
                .collect::<String>();
            cleaned_sentence(&joined)
        }
        Value::Object(object) => extract_object(object),
        _ => None,
    }
}

fn extract_object(object: &serde_json::Map<String, Value>) -> Option<String> {
    if let Some(message) = object.get("message")
        && let Some(text) = extract_text(message)
    {
        return Some(text);
    }
    if let Some(choices) = object.get("choices").and_then(Value::as_array) {
        for choice in choices {
            if let Some(text) = extract_text(choice) {
                return Some(text);
            }
        }
    }
    for key in PREFERRED_KEYS {
        if IGNORED_KEYS.contains(key) {
            continue;
        }
        if let Some(value) = object.get(*key)
            && let Some(text) = extract_text(value)
        {
            return Some(text);
        }
    }
    if let Some(output) = object.get("output").and_then(Value::as_array) {
        for item in output {
            if let Some(text) = extract_text(item) {
                return Some(text);
            }
        }
    }
    for (key, value) in object {
        if IGNORED_KEYS.contains(&key.as_str()) || PREFERRED_KEYS.contains(&key.as_str()) {
            continue;
        }
        if let Some(text) = extract_text(value) {
            return Some(text);
        }
    }
    None
}

fn cleaned_sentence(raw: &str) -> Option<String> {
    let mut text = strip_code_fence(raw).trim().to_owned();
    if text.is_empty() {
        return None;
    }
    if text.starts_with('{') {
        if let Some(json) = extract_first_json_object(&text)
            && let Ok(value) = serde_json::from_str::<Value>(&json)
        {
            for key in [
                "corrected_text",
                "correctedText",
                "text",
                "sentence",
                "english",
                "output",
                "content",
            ] {
                if let Some(inner) = value.get(key).and_then(Value::as_str)
                    && let Some(cleaned) = cleaned_sentence(inner)
                {
                    return Some(cleaned);
                }
            }
        }
        return first_json_string_value(&text);
    }
    if text.starts_with('[')
        && let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&text)
    {
        let joined = items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        if let Some(cleaned) = cleaned_sentence(&joined) {
            return Some(cleaned);
        }
    }
    if (text.starts_with('"') && text.ends_with('"') && text.chars().count() >= 2)
        || (text.starts_with('\'') && text.ends_with('\'') && text.chars().count() >= 2)
    {
        text = text
            .chars()
            .skip(1)
            .take(text.chars().count().saturating_sub(2))
            .collect();
    }
    text = text.trim_matches(['"', '\'', '`']).trim().to_owned();
    if text.is_empty() || text.starts_with('{') || text.starts_with('[') {
        return None;
    }
    Some(text)
}

fn first_json_string_value(text: &str) -> Option<String> {
    for key in [
        "corrected_text",
        "correctedText",
        "text",
        "sentence",
        "english",
        "output",
        "content",
    ] {
        let pattern = format!("\"{key}\"");
        let Some(start) = text.find(&pattern) else {
            continue;
        };
        let after = &text[start + pattern.len()..];
        let Some(colon) = after.find(':') else {
            continue;
        };
        let rest = after[colon + 1..].trim_start();
        if let Some(value) = read_json_string(rest)
            && let Some(cleaned) = cleaned_sentence(&value)
        {
            return Some(cleaned);
        }
    }
    None
}

fn read_json_string(text: &str) -> Option<String> {
    let mut chars = text.chars();
    if chars.next() != Some('"') {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for character in chars {
        if escaped {
            out.push(match character {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                other => other,
            });
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '"' => return Some(out),
            other => out.push(other),
        }
    }
    None
}

fn extract_first_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth = 0;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, character) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..start + offset + 1].to_owned());
                }
            }
            _ => {}
        }
    }
    None
}

fn strip_code_fence(text: &str) -> String {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed.to_owned();
    };
    let rest = rest
        .trim_start_matches(['j', 's', 'o', 'n', 'J', 'S', 'O', 'N'])
        .trim_start();
    rest.trim_end_matches('`').trim().to_owned()
}
