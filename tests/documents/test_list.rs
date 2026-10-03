//! Ports of the plain tests in `documents/test_list.py`.
//!
//! Python's sync and async pagers share the one async Rust `Pager`.
#![expect(
    clippy::unwrap_used,
    reason = "test helpers: a malformed mock or literal here is a test-setup bug"
)]

use futures_util::StreamExt;
use gemini_genai::types::ListDocumentsConfig;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

use crate::common::test_client;

const FILE_SEARCH_STORE_NAME: &str = "fileSearchStores/gzn7kdl2wpxl-4z2yqvuxbcxw";

/// Lists with `page_size=2` and checks every streamed item is a `Document`
/// named under the store.
async fn list_and_iterate() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1beta/{FILE_SEARCH_STORE_NAME}/documents")))
        .and(query_param("pageSize", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "documents": [
                {"name": format!("{FILE_SEARCH_STORE_NAME}/documents/d1")},
                {"name": format!("{FILE_SEARCH_STORE_NAME}/documents/d2")}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let pager = test_client(server.uri())
        .file_search_stores()
        .documents()
        .list(
            FILE_SEARCH_STORE_NAME,
            Some(ListDocumentsConfig {
                page_size: Some(2),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let documents: Vec<_> = Box::pin(pager.into_stream())
        .map(|item| item.unwrap())
        .collect()
        .await;
    assert_eq!(documents.len(), 2);
    assert!(documents.iter().all(|d| {
        d.name
            .as_deref()
            .is_some_and(|n| n.starts_with(FILE_SEARCH_STORE_NAME))
    }));
    server.verify().await;
}

// upstream-test: documents/test_list.py::test_pager
#[tokio::test]
async fn test_pager() {
    list_and_iterate().await;
}

// upstream-test: documents/test_list.py::test_async_pager
#[tokio::test]
async fn test_async_pager() {
    list_and_iterate().await;
}
