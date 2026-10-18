use lucid_core::ai::{AiConfiguration, AiProtocol};

#[test]
fn rejects_insecure_remote_configuration() {
    let config = AiConfiguration::new(
        AiProtocol::OpenAiCompatible,
        "http://api.example.com",
        "model",
    );
    assert!(config.validate(true).is_err());
}

#[test]
fn allows_local_http_and_model_list_before_selection() {
    let config = AiConfiguration::new(AiProtocol::OpenAiCompatible, "http://127.0.0.1:8080/v1", "");
    assert!(config.validate(false).is_ok());
    assert!(config.validate(true).is_err());
}

#[test]
fn parses_english_and_chinese_split() {
    let result =
        lucid_core::ai::CorrectionResult::new("Sparkling water and Coke. ||| 气泡水和可乐。");
    assert_eq!(result.corrected_text, "Sparkling water and Coke.");
    assert_eq!(result.chinese_text.as_deref(), Some("气泡水和可乐。"));

    let plain = lucid_core::ai::CorrectionResult::new("Only English sentence.");
    assert_eq!(plain.corrected_text, "Only English sentence.");
    assert_eq!(plain.chinese_text, None);
}
