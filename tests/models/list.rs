//! Ports of `models/test_list.py` plain functions (Developer API).

use gemini_genai::{Error, pagers::PagedItem, types::ListModelsConfig};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, query_param},
};

use super::common::test_client;
use super::support::TestResult;

/// Two pages: the first (no `pageToken`) points at the second.
async fn two_page_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("pageToken", "page-2"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"models": [{"name": "models/c"}]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "models": [{"name": "models/a"}, {"name": "models/b"}],
            "nextPageToken": "page-2"
        })))
        .mount(&server)
        .await;
    server
}

/// Python: `assert 'content-type' in pager.sdk_http_response.headers`
/// (lower-case on the wire; see the note in `tests/caches/list.rs`).
fn assert_pager_has_content_type<T>(pager: &gemini_genai::pagers::Pager<T>) {
    assert!(
        pager
            .sdk_http_response()
            .and_then(|response| response.headers.as_ref())
            .is_some_and(|headers| headers.contains_key("content-type")),
        "pager.sdk_http_response.headers must carry content-type"
    );
}

async fn assert_pager_walks_all_pages(config: ListModelsConfig) -> TestResult {
    let server = two_page_server().await;
    let client = test_client(server.uri());
    let mut pager = client.models().list(Some(config)).await?;
    assert_eq!(pager.name(), PagedItem::Models);
    assert_eq!(pager.page_size(), 10);
    assert!(pager.page().len() <= 10);
    assert_pager_has_content_type(&pager);
    let mut seen: Vec<String> = pager.page().iter().filter_map(|m| m.name.clone()).collect();
    while let Ok(page) = pager.next_page().await {
        seen.extend(page.iter().filter_map(|m| m.name.clone()));
        assert_pager_has_content_type(&pager);
    }
    assert_eq!(seen, ["models/a", "models/b", "models/c"]);
    // exhausted: next_page() must report "no more pages" (Python IndexError)
    let after = pager.next_page().await;
    assert!(
        matches!(after, Err(Error::NoMorePages)),
        "expected NoMorePages, got {after:?}"
    );
    Ok(())
}

// upstream-test: models/test_list.py::test_tuned_models_pager
#[tokio::test]
async fn test_tuned_models_pager() -> TestResult {
    // upstream's body only sets page_size; query_base defaults to base models
    assert_pager_walks_all_pages(ListModelsConfig {
        page_size: Some(10),
        ..Default::default()
    })
    .await
}

// upstream-test: models/test_list.py::test_base_models_pager
#[tokio::test]
async fn test_base_models_pager() -> TestResult {
    assert_pager_walks_all_pages(ListModelsConfig {
        page_size: Some(10),
        query_base: Some(true),
        ..Default::default()
    })
    .await
}

// upstream-test: models/test_list.py::test_tuned_models_async_pager
#[tokio::test]
async fn test_tuned_models_async_pager() -> TestResult {
    use futures_util::StreamExt;

    let server = two_page_server().await;
    let client = test_client(server.uri());
    let config = ListModelsConfig {
        page_size: Some(3),
        query_base: Some(false),
        ..Default::default()
    };
    let pager = client.models().list(Some(config)).await?;
    assert_eq!(pager.name(), PagedItem::Models);
    assert_eq!(pager.page_size(), 3);
    assert!(pager.page().len() <= 3);
    assert_pager_has_content_type(&pager);
    let names: Vec<String> = pager
        .into_stream()
        .map(|item| item.map(|m| m.name.unwrap_or_default()))
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<_, _>>()?;
    assert_eq!(names, ["models/a", "models/b", "models/c"]);
    // tunedModels endpoint was used
    let requests = server.received_requests().await.ok_or("no log")?;
    assert_eq!(requests[0].url.path(), "/v1beta/tunedModels");
    Ok(())
}

// upstream-test: models/test_list.py::test_base_models_async_pager
#[tokio::test]
async fn test_base_models_async_pager() -> TestResult {
    assert_pager_walks_all_pages(ListModelsConfig {
        page_size: Some(10),
        ..Default::default()
    })
    .await
}

async fn list_len_for_body(body: &'static str) -> Result<usize, Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;
    let client = test_client(server.uri());
    let pager = client.models().list(None).await?;
    Ok(pager.page().len())
}

// upstream-test: models/test_list.py::test_base_response_with_empty_json_payload_and_http_headers
#[tokio::test]
async fn test_base_response_with_empty_json_payload_and_http_headers() -> TestResult {
    assert_eq!(list_len_for_body("{}").await?, 0);
    Ok(())
}

// upstream-test: models/test_list.py::test_unknown_json_payload
#[tokio::test]
async fn test_unknown_json_payload() -> TestResult {
    assert_eq!(
        list_len_for_body(r#"{"unknown_key": "unknown_value"}"#).await?,
        0
    );
    Ok(())
}

// upstream-test: models/test_list.py::test_empty_json_payload
#[tokio::test]
async fn test_empty_json_payload() -> TestResult {
    assert_eq!(list_len_for_body("").await?, 0);
    Ok(())
}

// upstream-test: models/test_list.py::test_empty_api_response_none_headers
#[tokio::test]
async fn test_empty_api_response_none_headers() -> TestResult {
    // An HTTP response cannot lack a header map; the empty `{}` body is the
    // behavior under test.
    assert_eq!(list_len_for_body("{}").await?, 0);
    Ok(())
}

// upstream-test: models/test_list.py::test_empty_api_response_empty_dict_headers
#[tokio::test]
async fn test_empty_api_response_empty_dict_headers() -> TestResult {
    assert_eq!(list_len_for_body("{}").await?, 0);
    Ok(())
}
