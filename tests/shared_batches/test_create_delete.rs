//! Port of `google/genai/tests/shared/batches/test_create_delete.py`.

use gemini_genai::types::{BatchJobSource, Content, InlinedRequest, Part};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

const GEMINI_MODEL: &str = "gemini-2.5-flash";

// The upstream helper deletes only when the job is not pending (to avoid an
// error); the mocked `get` reports a finished job so the delete branch runs.
// upstream-test: shared/batches/test_create_delete.py::test_create_delete_mldev
#[tokio::test]
async fn test_create_delete_mldev() {
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
            "metadata": {"state": "BATCH_STATE_SUCCEEDED"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1beta/batches/abc123"))
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
    let name = batch_job.name.unwrap();
    let batch_job = client.batches().get(&name, None).await.unwrap();
    assert_ne!(
        batch_job.state,
        Some(gemini_genai::types::JobState::JobStatePending)
    );
    client.batches().delete(&name, None).await.unwrap();
    server.verify().await;
}
