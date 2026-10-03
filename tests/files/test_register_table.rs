//! Ports of `google/genai/tests/files/test_register_table.py`.
//!
//! The upstream module's `get_headers()` builds a bearer token from Google
//! application-default credentials, which this crate has no equivalent for
//! (see `Files::register_files`); the request itself is ported.

use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path},
};

use super::test_client;

// upstream-test: files/test_register_table.py::test_async
#[tokio::test]
async fn test_async() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/files:register"))
        .and(body_json(
            serde_json::json!({"uris": ["gs://unified-genai-dev/image.jpg"]}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "files": [{"name": "files/abc", "mimeType": "image/jpeg"}],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let response = test_client(server.uri())
        .files()
        .register_files(vec!["gs://unified-genai-dev/image.jpg".to_owned()], None)
        .await?;
    let files = response.files.unwrap_or_default();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].mime_type.as_deref(), Some("image/jpeg"));
    server.verify().await;
    Ok(())
}
