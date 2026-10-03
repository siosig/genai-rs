//! Ports of the plain tests in `documents/test_get.py`.

use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

const EXISTING_DOCUMENT_NAME: &str =
    "fileSearchStores/fr3l0ri2so25-a3r1ump9x821/documents/asurveyofmodernistpoetrytxt-uvmqjtmkm1h2";

// upstream-test: documents/test_get.py::test_async_get
#[tokio::test]
async fn test_async_get() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1beta/{EXISTING_DOCUMENT_NAME}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"name": EXISTING_DOCUMENT_NAME})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let document = test_client(server.uri())
        .file_search_stores()
        .documents()
        .get(EXISTING_DOCUMENT_NAME, None)
        .await
        .unwrap();
    assert_eq!(document.name.as_deref(), Some(EXISTING_DOCUMENT_NAME));
    server.verify().await;
}
