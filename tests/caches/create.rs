//! Ports of `caches/test_create.py`.

use gemini_genai::types::{Content, CreateCachedContentConfig, Part};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path},
};

use crate::common::test_client;

// upstream-test: caches/test_create.py::test_async_googleai_file_create
#[tokio::test]
async fn test_async_googleai_file_create() {
    let file_uri = "https://generativelanguage.googleapis.com/v1beta/files/v200dhvn15h7";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/cachedContents"))
        .and(body_json(serde_json::json!({
            "model": "models/gemini-2.5-flash",
            // A typed `FileData` is forwarded with its snake_case keys, as
            // the Python converter does for a model-built `Part`.
            "contents": [{
                "role": "user",
                "parts": [{"fileData": {"mime_type": "application/pdf", "file_uri": file_uri}}]
            }],
            "systemInstruction": {
                "role": "user",
                "parts": [{"text": "What is the sum of the two pdfs?"}]
            },
            "displayName": "test cache",
            "ttl": "86400s",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "cachedContents/abc123",
            "model": "models/gemini-2.5-flash",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let config = CreateCachedContentConfig {
        contents: Some(vec![Content {
            role: Some("user".to_owned()),
            parts: Some(vec![Part::from_uri(file_uri, "application/pdf")]),
        }]),
        system_instruction: Some(Content {
            role: Some("user".to_owned()),
            parts: Some(vec![Part {
                text: Some("What is the sum of the two pdfs?".to_owned()),
                ..Default::default()
            }]),
        }),
        display_name: Some("test cache".to_owned()),
        ttl: Some("86400s".to_owned()),
        ..Default::default()
    };
    let cached = test_client(server.uri())
        .caches()
        .create("gemini-2.5-flash", Some(config))
        .await
        .unwrap();
    assert_eq!(cached.name.as_deref(), Some("cachedContents/abc123"));
    server.verify().await;
}
