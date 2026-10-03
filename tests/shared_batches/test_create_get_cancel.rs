//! Port of `google/genai/tests/shared/batches/test_create_get_cancel.py`.

use gemini_genai::types::{BatchJobSource, Content, InlinedRequest, Part};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

const GEMINI_MODEL: &str = "gemini-2.5-flash";

// upstream-test: shared/batches/test_create_get_cancel.py::test_create_get_cancel_mldev
#[tokio::test]
async fn test_create_get_cancel_mldev() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/v1beta/models/{GEMINI_MODEL}:batchGenerateContent"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "batches/abc123",
            "metadata": {"state": "BATCH_STATE_PENDING"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1beta/batches/abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "batches/abc123",
            "metadata": {"state": "BATCH_STATE_PENDING"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1beta/batches/abc123:cancel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let src = BatchJobSource {
        inlined_requests: Some(vec![InlinedRequest {
            contents: Some(vec![Content {
                role: Some("user".to_owned()),
                parts: Some(vec![Part {
                    text: Some("Why is the sky blue?".to_owned()),
                    ..Default::default()
                }]),
            }]),
            ..Default::default()
        }]),
        ..Default::default()
    };
    let batch_job = client
        .batches()
        .create(GEMINI_MODEL, src, None)
        .await
        .unwrap();
    let batch_job = client
        .batches()
        .get(&batch_job.name.unwrap(), None)
        .await
        .unwrap();
    client
        .batches()
        .cancel(&batch_job.name.unwrap(), None)
        .await
        .unwrap();
    server.verify().await;
}
