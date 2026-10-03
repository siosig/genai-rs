//! Port of the plain test in `google/genai/tests/models/test_generate_images.py`.

use gemini_genai::{Error, types::GenerateImagesConfig};
use wiremock::MockServer;

use crate::common::test_client;

// upstream-test: models/test_generate_images.py::test_simple_prompt_async
#[tokio::test]
#[expect(deprecated, reason = "exercises the deprecated generate_images stub")]
async fn test_simple_prompt_async() {
    // Python raises ValueError on a Gemini Developer API client; Rust returns
    // `UnsupportedMethod` before sending anything.
    let server = MockServer::start().await;
    let config = GenerateImagesConfig {
        number_of_images: Some(1),
        output_mime_type: Some("image/jpeg".to_owned()),
        include_safety_attributes: Some(true),
        include_rai_reason: Some(true),
        ..Default::default()
    };
    let result = test_client(server.uri())
        .models()
        .generate_images("imagen-4.0-generate-001", "Red skateboard", Some(config))
        .await;
    assert!(
        matches!(result, Err(Error::UnsupportedMethod(_))),
        "{result:?}"
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}
