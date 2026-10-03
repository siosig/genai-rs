//! Ports of `batches/test_get.py`.

use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

const MLDEV_BATCH_OPERATION_NAME: &str = "batches/z2p8ksus4lyxt25rntl3fpd67p2niw4hfij5";

// upstream-test: batches/test_get.py::test_async_get
#[tokio::test]
async fn test_async_get() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1beta/{MLDEV_BATCH_OPERATION_NAME}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": MLDEV_BATCH_OPERATION_NAME,
            "metadata": {"state": "BATCH_STATE_SUCCEEDED", "model": "models/gemini-2.5-flash"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let batch_job = client
        .batches()
        .get(MLDEV_BATCH_OPERATION_NAME, None)
        .await
        .unwrap();
    assert_eq!(batch_job.name.as_deref(), Some(MLDEV_BATCH_OPERATION_NAME));
    server.verify().await;
}
