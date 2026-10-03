//! Ports of `caches/test_list.py`.

use gemini_genai::{Error, pagers::PagedItem, types::ListCachedContentsConfig};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param, query_param_is_missing},
};

use crate::common::test_client;

async fn two_page_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/cachedContents"))
        .and(query_param("pageSize", "2"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "cachedContents": [{"name": "cachedContents/a"}, {"name": "cachedContents/b"}],
            "nextPageToken": "tok1",
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1beta/cachedContents"))
        .and(query_param("pageToken", "tok1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "cachedContents": [{"name": "cachedContents/c"}],
        })))
        .expect(1)
        .mount(&server)
        .await;
    server
}

fn page_size_two() -> ListCachedContentsConfig {
    ListCachedContentsConfig {
        page_size: Some(2),
        ..Default::default()
    }
}

// upstream-test: caches/test_list.py::test_pager
#[tokio::test]
async fn test_pager() {
    let server = two_page_server().await;
    let mut cached_contents = test_client(server.uri())
        .caches()
        .list(Some(page_size_two()))
        .await
        .unwrap();
    assert_eq!(cached_contents.name(), PagedItem::CachedContents);
    assert_eq!(cached_contents.page_size(), 2);
    assert!(cached_contents.page().len() <= 2);
    // Python: `assert 'content-type' in cached_contents.sdk_http_response.headers` (header
    // names are lower-case on the wire; Python's async client reports
    // `Content-Type`, which an HTTP/1 header map cannot distinguish).
    assert!(
        cached_contents
            .sdk_http_response()
            .and_then(|response| response.headers.as_ref())
            .is_some_and(|headers| headers.contains_key("content-type"))
    );

    // Walk every page; the next `next_page()` must then fail.
    cached_contents.next_page().await.unwrap();
    let err = cached_contents.next_page().await.unwrap_err();
    assert!(matches!(err, Error::NoMorePages));
    server.verify().await;
}

// upstream-test: caches/test_list.py::test_async_pager
#[tokio::test]
async fn test_async_pager() {
    use tokio_stream::StreamExt as _;

    let server = two_page_server().await;
    let cached_contents = test_client(server.uri())
        .caches()
        .list(Some(page_size_two()))
        .await
        .unwrap();
    assert_eq!(cached_contents.name(), PagedItem::CachedContents);
    assert_eq!(cached_contents.page_size(), 2);
    assert!(cached_contents.page().len() <= 2);
    // Python: `assert 'content-type' in cached_contents.sdk_http_response.headers` (header
    // names are lower-case on the wire; Python's async client reports
    // `Content-Type`, which an HTTP/1 header map cannot distinguish).
    assert!(
        cached_contents
            .sdk_http_response()
            .and_then(|response| response.headers.as_ref())
            .is_some_and(|headers| headers.contains_key("content-type"))
    );

    let names: Vec<_> = cached_contents
        .into_stream()
        .map(|item| item.unwrap().name)
        .collect()
        .await;
    assert_eq!(names.len(), 3);
    server.verify().await;
}
