//! Ports of `google/genai/tests/live/test_live_response.py`: decoding of
//! server messages (Python's private `AsyncSession._receive`, here the first
//! item of `LiveSession::receive`). Developer-API half only.

use gemini_genai::types::{MediaModality, TurnCompleteReason};

use super::receive_turn_of;

// upstream-test: live/test_live_response.py::test_receive_server_content
#[tokio::test]
async fn test_receive_server_content() {
    let raw = serde_json::json!({
        "usageMetadata": {
            "promptTokenCount": 15,
            "responseTokenCount": 25,
            "candidatesTokenCount": 50,
            "totalTokenCount": 200,
            "responseTokensDetails": [{ "tokenCount": 20, "modality": "TEXT" }],
            "candidatesTokensDetails": [{ "tokenCount": 10, "modality": "TEXT" }]
        },
        "serverContent": {
            "modelTurn": { "parts": [{ "text": "This is a simple response." }] },
            "turnComplete": true,
            "groundingMetadata": {
                "web_search_queries": ["test query"],
                "groundingChunks": [{ "web": { "domain": "google.com", "title": "Search results" } }]
            }
        }
    })
    .to_string();
    let messages = receive_turn_of(&[raw.as_str()]).await;
    let result = messages[0].as_ref().unwrap();

    let content = result.server_content.as_ref().unwrap();
    assert_eq!(
        content.model_turn.as_ref().unwrap().parts.as_ref().unwrap()[0]
            .text
            .as_deref(),
        Some("This is a simple response.")
    );
    assert_eq!(content.turn_complete, Some(true));
    let grounding = content.grounding_metadata.as_ref().unwrap();
    assert_eq!(
        grounding.web_search_queries.as_deref(),
        Some(["test query".to_owned()].as_slice())
    );
    let web = grounding.grounding_chunks.as_ref().unwrap()[0]
        .web
        .as_ref()
        .unwrap();
    assert_eq!(web.domain.as_deref(), Some("google.com"));
    assert_eq!(web.title.as_deref(), Some("Search results"));

    // usageMetadata is parsed; on the Developer API the response-token fields
    // are taken as sent (Vertex remaps candidatesTokenCount instead).
    let usage = result.usage_metadata.as_ref().unwrap();
    assert_eq!(usage.prompt_token_count, Some(15));
    assert_eq!(usage.total_token_count, Some(200));
    assert_eq!(usage.response_token_count, Some(25));
    let details = &usage.response_tokens_details.as_ref().unwrap()[0];
    assert_eq!(details.token_count, Some(20));
    assert_eq!(details.modality, Some(MediaModality::Text));
}

// upstream-test: live/test_live_response.py::test_receive_server_content_with_turn_reason
#[tokio::test]
async fn test_receive_server_content_with_turn_reason() {
    let raw = serde_json::json!({
        "serverContent": {
            "modelTurn": { "parts": [{ "text": "Please provide more details." }] },
            "turnComplete": true,
            "turnCompleteReason": "NEED_MORE_INPUT",
            "waitingForInput": true
        }
    })
    .to_string();
    let messages = receive_turn_of(&[raw.as_str()]).await;
    let content = messages[0]
        .as_ref()
        .unwrap()
        .server_content
        .as_ref()
        .unwrap();
    assert_eq!(
        content.model_turn.as_ref().unwrap().parts.as_ref().unwrap()[0]
            .text
            .as_deref(),
        Some("Please provide more details.")
    );
    assert_eq!(content.turn_complete, Some(true));
    assert_eq!(
        content.turn_complete_reason,
        Some(TurnCompleteReason::NeedMoreInput)
    );
    assert_eq!(content.waiting_for_input, Some(true));
}
