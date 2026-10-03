//! Ports of `batches/test_cancel.py`.

use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

const MLDEV_BATCH_OPERATION_NAME: &str = "batches/0yew7plxupyybd7appsrq5vw7w0lp3l79lab";

// upstream-test: batches/test_cancel.py::test_async_cancel
#[tokio::test]
async fn test_async_cancel() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1beta/{MLDEV_BATCH_OPERATION_NAME}:cancel")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    client
        .batches()
        .cancel(MLDEV_BATCH_OPERATION_NAME, None)
        .await
        .unwrap();
    server.verify().await;
}
