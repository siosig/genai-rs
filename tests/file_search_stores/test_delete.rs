//! Ports of the plain tests in `file_search_stores/test_delete.py`.

use gemini_genai::types::DeleteFileSearchStoreConfig;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param, query_param_is_missing},
};

use crate::common::test_client;

// upstream-test: file_search_stores/test_delete.py::test_async_delete
#[tokio::test]
async fn test_async_delete() {
    let name = "fileSearchStores/my-file-search-store-l65kcyel9lkz";
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1beta/{name}")))
        .and(query_param_is_missing("force"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    test_client(server.uri())
        .file_search_stores()
        .delete(name, None)
        .await
        .unwrap();
    server.verify().await;
}

// upstream-test: file_search_stores/test_delete.py::test_async_force_delete
#[tokio::test]
async fn test_async_force_delete() {
    let name = "fileSearchStores/my-file-search-store-vjtrjw6re8oz";
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1beta/{name}")))
        .and(query_param("force", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    test_client(server.uri())
        .file_search_stores()
        .delete(
            name,
            Some(DeleteFileSearchStoreConfig {
                force: Some(true),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    server.verify().await;
}
