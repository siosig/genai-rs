//! Ports of `batches/test_create_with_file.py`.

use gemini_genai::types::{BatchJobSource, CreateBatchJobConfig};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path},
};

use crate::common::test_client;

const GEMINI_MODEL: &str = "gemini-2.5-flash";
const DISPLAY_NAME: &str = "test_batch";
const FILE_NAME: &str = "files/s0pa54alni6w";

// upstream-test: batches/test_create_with_file.py::test_async_create
#[tokio::test]
async fn test_async_create() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/v1beta/models/{GEMINI_MODEL}:batchGenerateContent"
        )))
        .and(body_json(serde_json::json!({
            "batch": {
                "displayName": DISPLAY_NAME,
                "inputConfig": {"fileName": FILE_NAME}
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "batches/abc123",
            "metadata": {
                "state": "BATCH_STATE_PENDING",
                "displayName": DISPLAY_NAME,
                "model": format!("models/{GEMINI_MODEL}"),
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let src = BatchJobSource {
        file_name: Some(FILE_NAME.to_owned()),
        ..Default::default()
    };
    let config = CreateBatchJobConfig {
        display_name: Some(DISPLAY_NAME.to_owned()),
        ..Default::default()
    };
    let batch_job = client
        .batches()
        .create(GEMINI_MODEL, src, Some(config))
        .await
        .unwrap();
    assert!(batch_job.name.as_deref().unwrap().starts_with("batches/"));
    assert_eq!(
        batch_job.model.as_deref(),
        Some(format!("models/{GEMINI_MODEL}").as_str())
    );
    server.verify().await;
}
