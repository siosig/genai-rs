//! Ports of `batches/test_delete.py`.

use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

const MLDEV_BATCH_OPERATION_NAME: &str = "batches/70h2jo0ic2t1zejyl0p4jgi8mk1gj0wvjusv";

// upstream-test: batches/test_delete.py::test_async_delete
#[tokio::test]
async fn test_async_delete() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1beta/{MLDEV_BATCH_OPERATION_NAME}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": MLDEV_BATCH_OPERATION_NAME,
            "done": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let delete_job = client
        .batches()
        .delete(MLDEV_BATCH_OPERATION_NAME, None)
        .await
        .unwrap();
    assert_eq!(delete_job.name.as_deref(), Some(MLDEV_BATCH_OPERATION_NAME));
    assert_eq!(delete_job.done, Some(true));
    server.verify().await;
}
