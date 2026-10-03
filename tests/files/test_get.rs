//! Ports of `google/genai/tests/files/test_get.py`.

use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use super::test_client;

// upstream-test: files/test_get.py::test_async
#[tokio::test]
async fn test_async() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/files/vjvu9fwk2qj8"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "files/vjvu9fwk2qj8",
            "mimeType": "image/png",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let file = test_client(server.uri())
        .files()
        .get("files/vjvu9fwk2qj8", None)
        .await?;
    assert_eq!(file.name.as_deref(), Some("files/vjvu9fwk2qj8"));
    assert_eq!(file.mime_type.as_deref(), Some("image/png"));
    server.verify().await;
    Ok(())
}
