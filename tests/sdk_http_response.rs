//! `sdk_http_response` is attached to API responses exactly where Python's
//! generated resource methods do it (`types.HttpResponse(headers=...)`).

#![expect(
    clippy::expect_used,
    reason = "integration-test helpers outside #[test] functions may unwrap; a failure is a test-setup bug"
)]

#[path = "common/mod.rs"]
mod common;

use std::collections::HashMap;

use gemini_genai::types::HttpResponse;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use common::test_client;

const HEADER_NAME: &str = "x-sdk-test";
const HEADER_VALUE: &str = "marker-1";

/// Serves `body` for every request with the marker header attached.
async fn server_for(http_method: &str, body: serde_json::Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method(http_method))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header(HEADER_NAME, HEADER_VALUE)
                .set_body_json(body),
        )
        .mount(&server)
        .await;
    server
}

/// Asserts the response carries the marker header and, like Python, no body.
fn assert_marker(response: Option<HttpResponse>) {
    let response = response.expect("sdk_http_response must be set");
    let headers: HashMap<String, String> = response.headers.expect("headers must be set");
    assert_eq!(
        headers.get(HEADER_NAME).map(String::as_str),
        Some(HEADER_VALUE)
    );
    assert_eq!(response.body, None);
}

#[tokio::test]
async fn models_delete_sets_sdk_http_response_headers() {
    let server = server_for("DELETE", serde_json::json!({})).await;
    let response = test_client(server.uri())
        .models()
        .delete("tunedModels/x", None)
        .await
        .unwrap();
    assert_marker(response.sdk_http_response);
}

#[tokio::test]
async fn models_count_tokens_sets_sdk_http_response_headers() {
    let server = server_for("POST", serde_json::json!({"totalTokens": 3})).await;
    let response = test_client(server.uri())
        .models()
        .count_tokens("gemini-2.0-flash", "hi", None)
        .await
        .unwrap();
    assert_eq!(response.total_tokens, Some(3));
    assert_marker(response.sdk_http_response);
}

#[tokio::test]
async fn models_embed_content_sets_sdk_http_response_headers() {
    let server = server_for("POST", serde_json::json!({"embeddings": []})).await;
    let response = test_client(server.uri())
        .models()
        .embed_content("gemini-embedding-001", "hi", None)
        .await
        .unwrap();
    assert_marker(response.sdk_http_response);
}

#[tokio::test]
async fn models_list_pager_sets_sdk_http_response_headers() {
    let server = server_for("GET", serde_json::json!({"models": []})).await;
    let pager = test_client(server.uri()).models().list(None).await.unwrap();
    assert_marker(pager.sdk_http_response().cloned());
}

#[tokio::test]
async fn files_delete_sets_sdk_http_response_headers() {
    let server = server_for("DELETE", serde_json::json!({})).await;
    let response = test_client(server.uri())
        .files()
        .delete("files/abc", None)
        .await
        .unwrap();
    assert_marker(response.sdk_http_response);
}

#[tokio::test]
async fn files_list_pager_sets_sdk_http_response_headers() {
    let server = server_for("GET", serde_json::json!({"files": []})).await;
    let pager = test_client(server.uri()).files().list(None).await.unwrap();
    assert_marker(pager.sdk_http_response().cloned());
}

#[tokio::test]
async fn batches_delete_sets_sdk_http_response_headers() {
    let server = server_for("DELETE", serde_json::json!({"name": "batches/b1"})).await;
    let response = test_client(server.uri())
        .batches()
        .delete("batches/b1", None)
        .await
        .unwrap();
    assert_marker(response.sdk_http_response);
}

#[tokio::test]
async fn batches_list_pager_sets_sdk_http_response_headers() {
    let server = server_for("GET", serde_json::json!({"operations": []})).await;
    let pager = test_client(server.uri())
        .batches()
        .list(None)
        .await
        .unwrap();
    assert_marker(pager.sdk_http_response().cloned());
}

#[tokio::test]
async fn tunings_get_sets_sdk_http_response_headers() {
    let server = server_for("GET", serde_json::json!({"name": "tunedModels/t1"})).await;
    let job = test_client(server.uri())
        .tunings()
        .get("tunedModels/t1", None)
        .await
        .unwrap();
    assert_marker(job.sdk_http_response);
}

#[tokio::test]
async fn tunings_cancel_sets_sdk_http_response_headers() {
    let server = server_for("POST", serde_json::json!({})).await;
    let response = test_client(server.uri())
        .tunings()
        .cancel("tunedModels/t1", None)
        .await
        .unwrap();
    assert_marker(response.sdk_http_response);
}

#[tokio::test]
async fn documents_list_pager_has_no_sdk_http_response_like_python() {
    // Python's generated `Documents._list` never attaches response headers.
    let server = server_for("GET", serde_json::json!({"documents": []})).await;
    let pager = test_client(server.uri())
        .file_search_stores()
        .documents()
        .list("fileSearchStores/s1", None)
        .await
        .unwrap();
    assert_eq!(pager.sdk_http_response(), None);
}

#[tokio::test]
async fn file_search_stores_list_pager_has_no_sdk_http_response_like_python() {
    // Python's generated `FileSearchStores._list` never attaches headers.
    let server = server_for("GET", serde_json::json!({"fileSearchStores": []})).await;
    let pager = test_client(server.uri())
        .file_search_stores()
        .list(None)
        .await
        .unwrap();
    assert_eq!(pager.sdk_http_response(), None);
}
