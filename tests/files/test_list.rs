//! Ports of `google/genai/tests/files/test_list.py`.

use futures_util::TryStreamExt as _;
use gemini_genai::{Error, pagers::PagedItem, types::ListFilesConfig};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param, query_param_is_missing},
};

use super::test_client;

/// Mounts a two-page listing: `files/one` (+ token) then `files/two`.
async fn mount_two_pages(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/v1beta/files"))
        .and(query_param("pageSize", "2"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "files": [{"name": "files/one"}, {"name": "files/two"}],
            "nextPageToken": "tok1",
        })))
        .expect(1)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1beta/files"))
        .and(query_param("pageToken", "tok1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "files": [{"name": "files/three"}],
        })))
        .expect(1)
        .mount(server)
        .await;
}

fn page_size_two() -> ListFilesConfig {
    ListFilesConfig {
        page_size: Some(2),
        ..Default::default()
    }
}

// upstream-test: files/test_list.py::test_pager
#[tokio::test]
async fn test_pager() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_two_pages(&server).await;

    let mut pager = test_client(server.uri())
        .files()
        .list(Some(page_size_two()))
        .await?;
    assert_eq!(pager.name(), PagedItem::Files);
    assert_eq!(pager.page_size(), 2);
    assert!(pager.page().len() <= 2);
    // Python: `assert 'content-type' in pager.sdk_http_response.headers` (header
    // names are lower-case on the wire; Python's async client reports
    // `Content-Type`, which an HTTP/1 header map cannot distinguish).
    assert!(
        pager
            .sdk_http_response()
            .and_then(|response| response.headers.as_ref())
            .is_some_and(|headers| headers.contains_key("content-type"))
    );

    // Walk every page; afterwards `next_page` must fail with "no more pages".
    let mut seen = pager.page().len();
    while let Ok(page) = pager.next_page().await {
        seen += page.len();
    }
    assert_eq!(seen, 3);
    assert!(matches!(pager.next_page().await, Err(Error::NoMorePages)));
    server.verify().await;
    Ok(())
}

// upstream-test: files/test_list.py::test_async_pager
#[tokio::test]
async fn test_async_pager() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_two_pages(&server).await;

    let pager = test_client(server.uri())
        .files()
        .list(Some(page_size_two()))
        .await?;
    assert_eq!(pager.name(), PagedItem::Files);
    assert_eq!(pager.page_size(), 2);
    assert!(pager.page().len() <= 2);
    // Python: `assert 'content-type' in pager.sdk_http_response.headers` (header
    // names are lower-case on the wire; Python's async client reports
    // `Content-Type`, which an HTTP/1 header map cannot distinguish).
    assert!(
        pager
            .sdk_http_response()
            .and_then(|response| response.headers.as_ref())
            .is_some_and(|headers| headers.contains_key("content-type"))
    );

    // `into_stream` iterates through all the pages.
    let names: Vec<_> = pager
        .into_stream()
        .map_ok(|file| file.name.unwrap_or_default())
        .try_collect()
        .await?;
    assert_eq!(names, ["files/one", "files/two", "files/three"]);
    server.verify().await;
    Ok(())
}
