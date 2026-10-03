//! Ports of the plain tests in `file_search_stores/test_list.py`.

use futures_util::StreamExt;
use gemini_genai::types::ListFileSearchStoresConfig;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

use crate::common::test_client;

// upstream-test: file_search_stores/test_list.py::test_async_pager
#[tokio::test]
async fn test_async_pager() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/fileSearchStores"))
        .and(query_param("pageSize", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fileSearchStores": [
                {"name": "fileSearchStores/a"},
                {"name": "fileSearchStores/b"}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let pager = test_client(server.uri())
        .file_search_stores()
        .list(Some(ListFileSearchStoresConfig {
            page_size: Some(2),
            ..Default::default()
        }))
        .await
        .unwrap();
    // Upstream breaks after the first item; the first page holds both.
    let first = Box::pin(pager.into_stream()).next().await.unwrap().unwrap();
    assert_eq!(first.name.as_deref(), Some("fileSearchStores/a"));
    server.verify().await;
}
