//! 覆盖英文无改动、缺失译文以及单独补取中文的真实 HTTP 路径。
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use lucid_core::prompt::{CHINESE_TRANSLATION_INSTRUCTION, CLEANUP_INSTRUCTION};
use lucid_core::{AiConfiguration, AiProtocol, CorrectionRequest};
use lucid_provider_http::HttpCorrectionService;
use serde_json::{Value, json};

async fn exercise_chinese_flow(protocol: AiProtocol) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for content in ["Hello. ||| 你好。", "Hello.", "你好。"] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let header_end = loop {
                let mut buffer = [0; 1024];
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0, "request ended before headers");
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().unwrap())
                })
                .unwrap();
            let mut body = bytes[header_end..].to_vec();
            while body.len() < length {
                let mut buffer = [0; 1024];
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0, "request ended before body");
                body.extend_from_slice(&buffer[..count]);
            }
            requests.push(serde_json::from_slice::<Value>(&body).unwrap());
            let response = match protocol {
                AiProtocol::OpenAiCompatible => {
                    json!({"choices": [{"message": {"content": content}}]})
                }
                AiProtocol::AnthropicCompatible => {
                    json!({"content": [{"type": "text", "text": content}]})
                }
            }
            .to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
        }
        requests
    });
    let configuration = AiConfiguration::new(protocol, format!("http://{address}"), "test-model");
    let service = HttpCorrectionService::new(configuration, "test-only-key").unwrap();
    let request = CorrectionRequest::new("Hello.");
    let unchanged = service.correct(&request).await.unwrap();
    assert_eq!(unchanged.corrected_text, "Hello.");
    assert_eq!(unchanged.chinese_text.as_deref(), Some("你好。"));
    let missing = service.correct(&request).await.unwrap();
    assert_eq!(missing.corrected_text, "Hello.");
    assert!(missing.chinese_text.is_none());
    assert_eq!(
        service
            .translate_to_chinese(&missing.corrected_text)
            .await
            .unwrap(),
        "你好。"
    );
    let requests = server.join().unwrap();
    for (index, body) in requests.iter().enumerate() {
        let expected_instruction = if index == 2 {
            CHINESE_TRANSLATION_INSTRUCTION
        } else {
            CLEANUP_INSTRUCTION
        };
        match protocol {
            AiProtocol::OpenAiCompatible => {
                assert_eq!(body["messages"][0]["content"], expected_instruction);
                assert_eq!(body["messages"][1]["content"], "Hello.");
            }
            AiProtocol::AnthropicCompatible => {
                assert_eq!(body["system"], expected_instruction);
                assert_eq!(body["messages"][0]["content"], "Hello.");
            }
        }
    }
}

#[tokio::test]
async fn openai_keeps_chinese_for_correct_english_and_can_fetch_missing_translation() {
    exercise_chinese_flow(AiProtocol::OpenAiCompatible).await;
}

#[tokio::test]
async fn anthropic_keeps_chinese_for_correct_english_and_can_fetch_missing_translation() {
    exercise_chinese_flow(AiProtocol::AnthropicCompatible).await;
}
