//! Ports of `google/genai/tests/chats/test_validate_response.py`.
//!
//! Upstream calls the private `chats._validate_response` directly. Rust keeps
//! `validate_response` private, so each case drives a chat with a mocked model
//! reply and observes the public effect of the validation: a reply is valid
//! exactly when its exchange enters the curated history.

use gemini_genai::types::Part;
use wiremock::{MockServer, ResponseTemplate};

use super::{mount_always, test_client};

/// Whether the chat treats `reply` (the raw model response JSON) as a valid
/// response, i.e. keeps the exchange in its curated history.
async fn is_valid_response(reply: serde_json::Value) -> bool {
    let server = MockServer::start().await;
    mount_always(&server, ResponseTemplate::new(200).set_body_json(reply)).await;
    let mut chat = test_client(server.uri())
        .chats()
        .create("gemini-2.5-flash", None, None);

    let Ok(_) = chat.send_message("Hello", None).await else {
        return false;
    };

    // The comprehensive history always records the exchange; only the
    // curated one reflects validity.
    assert_eq!(chat.get_history(false).len(), 2, "comprehensive history");
    !chat.get_history(true).is_empty()
}

// upstream-test: chats/test_validate_response.py::test_validate_response_default_response
#[tokio::test]
async fn test_validate_response_default_response() {
    assert!(!is_valid_response(serde_json::json!({})).await);
}

// upstream-test: chats/test_validate_response.py::test_validate_response_empty_content
#[tokio::test]
async fn test_validate_response_empty_content() {
    assert!(!is_valid_response(serde_json::json!({"candidates": []})).await);
}

// upstream-test: chats/test_validate_response.py::test_validate_response_empty_parts
#[tokio::test]
async fn test_validate_response_empty_parts() {
    let reply = serde_json::json!({"candidates": [{"content": {"parts": []}}]});
    assert!(!is_valid_response(reply).await);
}

// upstream-test: chats/test_validate_response.py::test_validate_response_empty_part
#[tokio::test]
async fn test_validate_response_empty_part() {
    let reply = serde_json::json!({"candidates": [{"content": {"parts": [{}]}}]});
    assert!(!is_valid_response(reply).await);
}

// upstream-test: chats/test_validate_response.py::test_validate_response_part_with_empty_text
#[tokio::test]
async fn test_validate_response_part_with_empty_text() {
    let reply = serde_json::json!({"candidates": [{"content": {"parts": [{"text": ""}]}}]});
    assert!(is_valid_response(reply).await);
}

// upstream-test: chats/test_validate_response.py::test_validate_response_part_with_text
#[tokio::test]
async fn test_validate_response_part_with_text() {
    let reply = serde_json::json!({
        "candidates": [{"content": {"parts": [{"text": "response from model"}]}}]
    });
    assert!(is_valid_response(reply).await);
}

// upstream-test: chats/test_validate_response.py::test_validate_response_part_with_function_call
#[tokio::test]
async fn test_validate_response_part_with_function_call() {
    let reply = serde_json::json!({
        "candidates": [{"content": {"parts": [{
            "functionCall": {"name": "foo", "args": {"bar": "baz"}}
        }]}}]
    });
    assert!(is_valid_response(reply).await);
    // Sanity: the upstream `Part()` default is what the "empty part" case
    // above relies on being distinct from every populated part.
    assert_ne!(Part::default(), Part::from_text(""));
}
