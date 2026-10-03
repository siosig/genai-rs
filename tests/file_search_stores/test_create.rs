//! Ports of the plain tests in `file_search_stores/test_create.py`.
//!
//! Python replays recorded HTTP; these serve the same scenarios from
//! `wiremock` and assert on the request the crate sends.

use gemini_genai::types::CreateFileSearchStoreConfig;
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

const STORE_NAME: &str = "fileSearchStores/my-store-1a2b3c";

async fn store_server(request_body: Option<Value>) -> MockServer {
    let server = MockServer::start().await;
    // Python sends no body at all when there is nothing to send.
    let body_matches = move |request: &Request| {
        request_body.as_ref().map_or_else(
            || request.body.is_empty(),
            |expected| {
                serde_json::from_slice::<Value>(&request.body)
                    .is_ok_and(|actual| actual == *expected)
            },
        )
    };
    Mock::given(method("POST"))
        .and(path("/v1beta/fileSearchStores"))
        .and(body_matches)
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": STORE_NAME,
            "displayName": "My File Search Store"
        })))
        .expect(1)
        .mount(&server)
        .await;
    server
}

// upstream-test: file_search_stores/test_create.py::test_async_display_name
#[tokio::test]
async fn test_async_display_name() {
    let server = store_server(Some(json!({"displayName": "My File Search Store"}))).await;
    let store = test_client(server.uri())
        .file_search_stores()
        .create(Some(CreateFileSearchStoreConfig {
            display_name: Some("My File Search Store".to_owned()),
            ..Default::default()
        }))
        .await
        .unwrap();
    assert_eq!(store.name.as_deref(), Some(STORE_NAME));
    assert_eq!(store.display_name.as_deref(), Some("My File Search Store"));
    server.verify().await;
}

// upstream-test: file_search_stores/test_create.py::test_async_basic
#[tokio::test]
async fn test_async_basic() {
    let server = store_server(None).await;
    let store = test_client(server.uri())
        .file_search_stores()
        .create(None)
        .await
        .unwrap();
    assert_eq!(store.name.as_deref(), Some(STORE_NAME));
    server.verify().await;
}
