use lucid_core::CorrectionResult;
use lucid_provider_http::ResponseParser;

#[test]
fn parses_openai_content() {
    let data = br#"{"choices":[{"message":{"content":"I want to buy coffee."}}]}"#;
    assert_eq!(
        ResponseParser::sentence(data).as_deref(),
        Some("I want to buy coffee.")
    );
}

#[test]
fn prefers_content_over_reasoning() {
    let data =
        br#"{"choices":[{"message":{"reasoning":"thinking","content":"I want to buy coffee."}}]}"#;
    assert_eq!(
        ResponseParser::sentence(data).as_deref(),
        Some("I want to buy coffee.")
    );
}

#[test]
fn recovers_json_payload_inside_content() {
    let data = br#"{"choices":[{"message":{"content":"{\"corrected_text\":\"I want to buy coffee.\"}"}}]}"#;
    assert_eq!(
        ResponseParser::sentence(data).as_deref(),
        Some("I want to buy coffee.")
    );
}

#[test]
fn parses_model_ids() {
    let data = br#"{"data":[{"id":"gpt-a"},{"id":"gpt-b"}]}"#;
    assert_eq!(
        ResponseParser::model_ids(data).unwrap(),
        vec!["gpt-a", "gpt-b"]
    );
}

#[test]
fn parses_english_and_chinese_split() {
    let result = CorrectionResult::new("Sparkling water and Coke. ||| 气泡水和可乐。");
    assert_eq!(result.corrected_text, "Sparkling water and Coke.");
    assert_eq!(result.chinese_text.as_deref(), Some("气泡水和可乐。"));

    let plain = CorrectionResult::new("Only English sentence.");
    assert_eq!(plain.corrected_text, "Only English sentence.");
    assert_eq!(plain.chinese_text, None);
}
