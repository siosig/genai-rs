//! Ported from upstream `tests/types/test_bytes_internal.py`: base64
//! handling of `Blob.data` through `generate_content`, on the request and
//! on the response. (Developer API only; the Vertex parameterization is
//! out of scope. The Python `encode_unserializable_types` mock assertion is
//! a Python internal and is not ported.)

#![expect(
    clippy::expect_used,
    reason = "helper functions outside #[test] bodies assert test preconditions; a failure there is a test bug"
)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gemini_genai::types::{Blob, Content, Part};
use serde_json::{Value, json};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use crate::common::test_client;

/// 64 chars in url safe base64.
const BASE64_URL_SAFE: &str = "-_abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
/// 64 chars in normal base64 is an invalid format for `val_json_bytes`.
const BASE64_NOT_URL_SAFE: &str =
    "+/abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789===";
const MODEL: &str = "gemini-2.5-flash-001";

fn raw_bytes() -> Vec<u8> {
    STANDARD
        .decode("+/abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789")
        .expect("valid standard base64")
}

fn hello_reply() -> Value {
    json!({"candidates": [{"content": {"parts": [{"text": "Hello World"}], "role": "model"}}]})
}

fn content_with_blob(data: Vec<u8>) -> Content {
    Content {
        role: Some("user".to_owned()),
        parts: Some(vec![Part {
            inline_data: Some(Blob {
                mime_type: Some("image/png".to_owned()),
                data: Some(data),
                ..Default::default()
            }),
            ..Default::default()
        }]),
    }
}

async fn mock_server(reply: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(reply))
        .mount(&server)
        .await;
    server
}

/// The `data` of the first part's inline blob in the single captured
/// request, matching the key case-insensitively (`inlineData` vs
/// `inline_data`) like upstream's `get_value_ignore_key_case`.
async fn sent_inline_data(server: &MockServer) -> String {
    let requests = server.received_requests().await.expect("recording on");
    assert_eq!(requests.len(), 1);
    let body: Value = requests[0].body_json().expect("json body");
    let part = body["contents"][0]["parts"][0]
        .as_object()
        .expect("part is an object");
    let (_, blob) = part
        .iter()
        .find(|(key, _)| key.replace('_', "").eq_ignore_ascii_case("inlinedata"))
        .expect("inline data present");
    blob["data"].as_str().expect("data is a string").to_owned()
}

// Valid bytes (given as raw bytes, or as a url safe base64 string that was
// decoded on the way in) are sent as url safe base64.
// upstream-test: types/test_bytes_internal.py::test_base64_pydantic_input_success
#[tokio::test]
async fn test_base64_pydantic_input_success() {
    let from_base64: Blob =
        serde_json::from_value(json!({"data": BASE64_URL_SAFE})).expect("valid base64");
    let inputs = [
        ("raw bytes", raw_bytes()),
        ("url safe base64", from_base64.data.expect("data set")),
    ];
    for (label, bytes_input) in inputs {
        let server = mock_server(hello_reply()).await;
        let response = test_client(server.uri())
            .models()
            .generate_content(MODEL, content_with_blob(bytes_input), None)
            .await
            .expect("generate_content succeeds");

        assert_eq!(sent_inline_data(&server).await, BASE64_URL_SAFE, "{label}");
        let content = response.candidates.expect("candidates")[0]
            .content
            .clone()
            .expect("content");
        assert_eq!(
            content,
            Content {
                role: Some("model".to_owned()),
                parts: Some(vec![Part::from_text("Hello World")]),
            },
            "{label}"
        );
    }
}

// The same with the content given as a (camelCase) JSON dict.
// upstream-test: types/test_bytes_internal.py::test_base64_dict_input_success
#[tokio::test]
async fn test_base64_dict_input_success() {
    let raw_as_standard = STANDARD.encode(raw_bytes());
    for (label, data) in [
        ("url safe base64", BASE64_URL_SAFE.to_owned()),
        ("standard base64 of the raw bytes", raw_as_standard),
    ] {
        let contents: Content = serde_json::from_value(json!({
            "role": "user",
            "parts": [{"inlineData": {"mimeType": "image/png", "data": data}}],
        }))
        .expect("dict input is valid");
        let server = mock_server(hello_reply()).await;
        test_client(server.uri())
            .models()
            .generate_content(MODEL, contents, None)
            .await
            .expect("generate_content succeeds");
        assert_eq!(sent_inline_data(&server).await, BASE64_URL_SAFE, "{label}");
    }
}

// Invalid base64 (normal alphabet with bad padding) is rejected when the
// request content is built. Rust types hold raw bytes, so the only entry
// for a base64 string is deserialization; both Python variants (pydantic
// model input / dict input) therefore fail at the same place.
// upstream-test: types/test_bytes_internal.py::test_base64_pydantic_input_failure
#[test]
fn test_base64_pydantic_input_failure() {
    let result = serde_json::from_value::<Blob>(json!({
        "mime_type": "image/png",
        "data": BASE64_NOT_URL_SAFE,
    }));
    assert!(result.is_err());
}

// upstream-test: types/test_bytes_internal.py::test_base64_dict_input_failure
#[test]
fn test_base64_dict_input_failure() {
    let result = serde_json::from_value::<Content>(json!({
        "role": "user",
        "parts": [{"inlineData": {"mimeType": "image/png", "data": BASE64_NOT_URL_SAFE}}],
    }));
    assert!(result.is_err());
}

fn blob_reply(data: &str) -> Value {
    json!({"candidates": [{"content": {
        "parts": [{"inlineData": {"data": data, "mimeType": "image/png"}}],
        "role": "model",
    }}]})
}

// A url safe base64 string in the response is decoded to raw bytes.
// upstream-test: types/test_bytes_internal.py::test_base64_pydantic_output_success
#[tokio::test]
async fn test_base64_pydantic_output_success() {
    let server = mock_server(blob_reply(BASE64_URL_SAFE)).await;
    let response = test_client(server.uri())
        .models()
        .generate_content(MODEL, Content::from("Hello World"), None)
        .await
        .expect("generate_content succeeds");

    let content = response.candidates.expect("candidates")[0]
        .content
        .clone()
        .expect("content");
    assert_eq!(
        content,
        Content {
            role: Some("model".to_owned()),
            parts: Some(vec![Part::from_bytes(raw_bytes(), "image/png")]),
        }
    );
}

// Invalid base64 in the response is an error.
// upstream-test: types/test_bytes_internal.py::test_base64_pydantic_output_failure
#[tokio::test]
async fn test_base64_pydantic_output_failure() {
    let server = mock_server(blob_reply(BASE64_NOT_URL_SAFE)).await;
    let result = test_client(server.uri())
        .models()
        .generate_content(MODEL, Content::from("Hello World"), None)
        .await;
    assert!(result.is_err());
}
