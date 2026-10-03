//! Ports of `batches/test_list.py`.

use gemini_genai::{pagers::PagedItem, types::ListBatchJobsConfig};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

use crate::common::test_client;

// upstream-test: batches/test_list.py::test_pager
#[tokio::test]
async fn test_pager() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/batches"))
        .and(query_param("pageSize", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "operations": [
                {"name": "batches/one", "metadata": {"state": "BATCH_STATE_RUNNING"}},
                {"name": "batches/two", "metadata": {"state": "BATCH_STATE_SUCCEEDED"}},
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let config = ListBatchJobsConfig {
        page_size: Some(10),
        ..Default::default()
    };
    let batch_jobs = client.batches().list(Some(config)).await.unwrap();
    assert_eq!(batch_jobs.name(), PagedItem::BatchJobs);
    assert_eq!(batch_jobs.page_size(), 10);
    assert!(batch_jobs.page().len() <= 10);
    // Python: `assert 'content-type' in batch_jobs.sdk_http_response.headers` (header
    // names are lower-case on the wire; Python's async client reports
    // `Content-Type`, which an HTTP/1 header map cannot distinguish).
    assert!(
        batch_jobs
            .sdk_http_response()
            .and_then(|response| response.headers.as_ref())
            .is_some_and(|headers| headers.contains_key("content-type"))
    );
    server.verify().await;
}

// The async Python pager is the only pager in Rust, so this also drains the
// pager as a stream.
// upstream-test: batches/test_list.py::test_async_pager
#[tokio::test]
async fn test_async_pager() {
    use tokio_stream::StreamExt as _;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/batches"))
        .and(query_param("pageSize", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "operations": [
                {"name": "batches/one", "metadata": {"state": "BATCH_STATE_RUNNING"}},
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let config = ListBatchJobsConfig {
        page_size: Some(10),
        ..Default::default()
    };
    let batch_jobs = client.batches().list(Some(config)).await.unwrap();
    assert_eq!(batch_jobs.name(), PagedItem::BatchJobs);
    assert_eq!(batch_jobs.page_size(), 10);
    assert!(batch_jobs.page().len() <= 10);
    // Python: `assert 'content-type' in batch_jobs.sdk_http_response.headers` (header
    // names are lower-case on the wire; Python's async client reports
    // `Content-Type`, which an HTTP/1 header map cannot distinguish).
    assert!(
        batch_jobs
            .sdk_http_response()
            .and_then(|response| response.headers.as_ref())
            .is_some_and(|headers| headers.contains_key("content-type"))
    );

    let names: Vec<_> = batch_jobs
        .into_stream()
        .map(|job| job.unwrap().name)
        .collect()
        .await;
    assert_eq!(names, vec![Some("batches/one".to_owned())]);
    server.verify().await;
}
