//! Ports of the plain tests in `file_search_stores/test_get.py`.

use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

const EXISTING_FILE_SEARCH_STORE_NAME: &str = "fileSearchStores/my-store-37cbhu1nw16r";

// upstream-test: file_search_stores/test_get.py::test_async_get
#[tokio::test]
async fn test_async_get() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1beta/{EXISTING_FILE_SEARCH_STORE_NAME}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": EXISTING_FILE_SEARCH_STORE_NAME
        })))
        .expect(1)
        .mount(&server)
        .await;
    let store = test_client(server.uri())
        .file_search_stores()
        .get(EXISTING_FILE_SEARCH_STORE_NAME, None)
        .await
        .unwrap();
    assert_eq!(store.name.as_deref(), Some(EXISTING_FILE_SEARCH_STORE_NAME));
    server.verify().await;
}
