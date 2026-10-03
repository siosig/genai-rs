//! Ports of `google/genai/tests/live/test_send_client_content.py`.
//!
//! Developer-API behaviour only (the upstream `vertexai=True` half of each
//! parameterised case is Vertex-specific). Deviation: Python's session writes
//! the top-level key as `client_content`; this crate writes the equivalent
//! camelCase `clientContent` (both are accepted by the server), so the
//! assertions read `clientContent`.

use gemini_genai::types::{Blob, Content, Part};
use serde_json::json;

use super::run_send;

fn text_content(text: &str) -> Content {
    Content {
        parts: Some(vec![Part {
            text: Some(text.to_owned()),
            ..Default::default()
        }]),
        ..Default::default()
    }
}

// upstream-test: live/test_send_client_content.py::test_send_content_dict
#[tokio::test]
async fn test_send_content_dict() {
    let (result, sent) = run_send(async |s| {
        s.send_client_content(Some(vec![text_content("test")]), true)
            .await
    })
    .await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("clientContent").is_some(), "{sent}");
    assert_eq!(
        sent["clientContent"]["turns"][0]["parts"][0]["text"],
        "test"
    );
}

// upstream-test: live/test_send_client_content.py::test_send_content_dict_list
#[tokio::test]
async fn test_send_content_dict_list() {
    let (result, sent) = run_send(async |s| {
        s.send_client_content(Some(vec![text_content("test")]), true)
            .await
    })
    .await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("clientContent").is_some(), "{sent}");
    assert_eq!(
        sent["clientContent"]["turns"][0]["parts"][0]["text"],
        "test"
    );
}

// upstream-test: live/test_send_client_content.py::test_send_content_content
#[tokio::test]
async fn test_send_content_content() {
    // Python accepts a bare `Content` as well as a list; Rust takes `Vec<Content>`.
    let (result, sent) = run_send(async |s| {
        s.send_client_content(Some(vec![text_content("test")]), true)
            .await
    })
    .await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("clientContent").is_some(), "{sent}");
    assert_eq!(
        sent["clientContent"]["turns"][0]["parts"][0]["text"],
        "test"
    );
}

// upstream-test: live/test_send_client_content.py::test_send_content_with_blob
#[tokio::test]
async fn test_send_content_with_blob() {
    let content = Content {
        parts: Some(vec![Part {
            inline_data: Some(Blob {
                data: Some(b"test".to_vec()),
                ..Default::default()
            }),
            ..Default::default()
        }]),
        ..Default::default()
    };
    let (result, sent) =
        run_send(async |s| s.send_client_content(Some(vec![content]), true).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("clientContent").is_some(), "{sent}");
    assert_eq!(
        sent["clientContent"]["turns"][0]["parts"][0]["inlineData"],
        json!({ "data": "dGVzdA==" })
    );
}

// upstream-test: live/test_send_client_content.py::test_send_client_content_turn_complete_false
#[tokio::test]
async fn test_send_client_content_turn_complete_false() {
    let (result, sent) = run_send(async |s| s.send_client_content(None, false).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("clientContent").is_some(), "{sent}");
    assert_eq!(sent["clientContent"]["turnComplete"], false);
}

// upstream-test: live/test_send_client_content.py::test_send_client_content_empty
#[tokio::test]
async fn test_send_client_content_empty() {
    // Python's defaults are `turns=None, turn_complete=True`.
    let (result, sent) = run_send(async |s| s.send_client_content(None, true).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("clientContent").is_some(), "{sent}");
    assert_eq!(sent["clientContent"]["turnComplete"], true);
}
