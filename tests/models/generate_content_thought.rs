//! Port of `google/genai/tests/models/test_generate_content_thought.py`
//! (plain test; the table cases live in the oracle corpus).

use gemini_genai::types::{GenerateContentConfig, ThinkingConfig};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use crate::common::test_client;

// upstream-test: models/test_generate_content_thought.py::test_thinking_budget
#[tokio::test]
async fn test_thinking_budget() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "candidates": [{"content": {"role": "model", "parts": [
                {"text": "Summing 1..100 pairwise.", "thought": true},
                {"text": "5050"}
            ]}, "finishReason": "STOP"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let config = GenerateContentConfig {
        thinking_config: Some(ThinkingConfig {
            include_thoughts: Some(true),
            thinking_budget: Some(10000),
            ..Default::default()
        }),
        ..Default::default()
    };
    let response = test_client(server.uri())
        .models()
        .generate_content(
            "gemini-2.5-pro",
            "What is the sum of natural numbers from 1 to 100?",
            Some(config),
        )
        .await
        .unwrap();

    let has_thought = response
        .candidates
        .iter()
        .flatten()
        .filter_map(|candidate| candidate.content.as_ref())
        .flat_map(|content| content.parts.iter().flatten())
        .any(|part| part.thought == Some(true));
    assert!(has_thought);

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = requests[0].body_json().unwrap();
    let thinking = &body["generationConfig"]["thinkingConfig"];
    assert_eq!(thinking["include_thoughts"], true);
    assert_eq!(thinking["thinking_budget"], 10000);
}
