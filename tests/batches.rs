//! Integration tests for `client.batches()` (batch job create/get/list/
//! cancel/delete). Runs against the public API only, via `wiremock`,
//! mirroring `src/batches.rs`'s own module doc.

use google_genai::types::{
    BatchJobDestination, BatchJobSource, Content, CreateBatchJobConfig, EmbeddingsBatchJobSource,
    HttpOptions, InlinedRequest, JobState, ListBatchJobsConfig, Part,
};
use google_genai::{Backend, Client, Error};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[expect(
    clippy::unwrap_used,
    reason = "test helper: a broken Client::builder() here is a test-setup bug, not a runtime condition"
)]
fn test_client(base_url: String) -> Client {
    Client::builder()
        .api_key("test-key")
        .http_options(HttpOptions {
            base_url: Some(base_url),
            ..Default::default()
        })
        .build()
        .unwrap()
}

fn batch_job_response(name: &str, state: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "metadata": {
            "state": state,
            "displayName": "my batch",
            "model": "models/gemini-2.5-flash",
        }
    })
}

#[tokio::test]
async fn create_with_inlined_requests_sends_the_nested_batch_input_config() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-flash:batchGenerateContent"))
        .and(body_json(serde_json::json!({
            "batch": {
                "inputConfig": {
                    "requests": {
                        "requests": [{
                            "request": {
                                "model": "models/gemini-2.5-flash",
                                "contents": [{"role": "user", "parts": [{"text": "Say hi"}]}]
                            }
                        }]
                    }
                }
            }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(batch_job_response("batches/abc123", "BATCH_STATE_PENDING")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let src = BatchJobSource {
        inlined_requests: Some(vec![InlinedRequest {
            model: Some("gemini-2.5-flash".to_owned()),
            contents: Some(vec![Content {
                role: Some("user".to_owned()),
                parts: Some(vec![Part {
                    text: Some("Say hi".to_owned()),
                    ..Default::default()
                }]),
            }]),
            ..Default::default()
        }]),
        ..Default::default()
    };

    let batch_job = client
        .batches()
        .create("gemini-2.5-flash", src, None)
        .await
        .unwrap();
    assert_eq!(batch_job.name.as_deref(), Some("batches/abc123"));
    assert_eq!(batch_job.state, Some(JobState::JobStatePending));
    server.verify().await;
}

#[tokio::test]
async fn create_with_file_name_sends_the_file_name_input_config() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-flash:batchGenerateContent"))
        .and(body_json(serde_json::json!({
            "batch": {
                "inputConfig": {
                    "fileName": "files/abc123"
                }
            }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(batch_job_response("batches/xyz789", "BATCH_STATE_PENDING")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let src = BatchJobSource {
        file_name: Some("files/abc123".to_owned()),
        ..Default::default()
    };

    let batch_job = client
        .batches()
        .create("gemini-2.5-flash", src, None)
        .await
        .unwrap();
    assert_eq!(batch_job.name.as_deref(), Some("batches/xyz789"));
    server.verify().await;
}

#[tokio::test]
async fn create_rejects_a_source_with_neither_inlined_requests_nor_file_name() {
    let client = test_client("http://127.0.0.1:1".to_owned());
    let err = client
        .batches()
        .create("gemini-2.5-flash", BatchJobSource::default(), None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Validation(_)));
}

#[tokio::test]
async fn create_with_a_vertex_only_dest_field_is_rejected() {
    let client = test_client("http://127.0.0.1:1".to_owned());
    let src = BatchJobSource {
        file_name: Some("files/abc123".to_owned()),
        ..Default::default()
    };
    let config = CreateBatchJobConfig {
        dest: Some(BatchJobDestination::default()),
        ..Default::default()
    };
    let err = client
        .batches()
        .create("gemini-2.5-flash", src, Some(config))
        .await
        .unwrap_err();
    match err {
        Error::UnsupportedByBackend { field, backend } => {
            assert_eq!(field, "dest");
            assert_eq!(backend, Backend::VertexAi);
        }
        other => panic!("expected Error::UnsupportedByBackend, got {other:?}"),
    }
}

#[tokio::test]
async fn create_embeddings_sends_the_async_batch_embed_content_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/text-embedding-004:asyncBatchEmbedContent",
        ))
        .and(body_json(serde_json::json!({
            "batch": {
                "inputConfig": {
                    "file_name": "files/embed-input"
                }
            }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(batch_job_response("batches/embed1", "BATCH_STATE_PENDING")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let src = EmbeddingsBatchJobSource {
        file_name: Some("files/embed-input".to_owned()),
        ..Default::default()
    };
    let batch_job = client
        .batches()
        .create_embeddings("text-embedding-004", src, None)
        .await
        .unwrap();
    assert_eq!(batch_job.name.as_deref(), Some("batches/embed1"));
    server.verify().await;
}

#[tokio::test]
async fn get_normalizes_the_batch_state_and_the_resource_name() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/batches/abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(batch_job_response(
            "batches/abc123",
            "BATCH_STATE_SUCCEEDED",
        )))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let batch_job = client.batches().get("batches/abc123", None).await.unwrap();
    assert_eq!(batch_job.state, Some(JobState::JobStateSucceeded));
    assert_eq!(batch_job.model.as_deref(), Some("models/gemini-2.5-flash"));
    server.verify().await;
}

#[tokio::test]
async fn get_rejects_a_name_that_is_not_a_batches_resource_name() {
    let client = test_client("http://127.0.0.1:1".to_owned());
    let err = client
        .batches()
        .get("not-a-batch-name", None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Validation(_)));
}

#[tokio::test]
async fn cancel_posts_to_the_cancel_suffixed_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/batches/abc123:cancel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    client
        .batches()
        .cancel("batches/abc123", None)
        .await
        .unwrap();
    server.verify().await;
}

#[tokio::test]
async fn delete_sends_a_delete_request_and_parses_the_resource_job() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v1beta/batches/abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "batches/abc123",
            "done": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let deleted = client
        .batches()
        .delete("batches/abc123", None)
        .await
        .unwrap();
    assert_eq!(deleted.name.as_deref(), Some("batches/abc123"));
    assert_eq!(deleted.done, Some(true));
    server.verify().await;
}

#[tokio::test]
async fn list_returns_a_pager_over_the_batch_jobs_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/batches"))
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
    let pager = client.batches().list(None).await.unwrap();
    assert_eq!(pager.page().len(), 2);
    assert_eq!(pager.page()[0].name.as_deref(), Some("batches/one"));
    assert_eq!(pager.page()[1].state, Some(JobState::JobStateSucceeded));
    server.verify().await;
}

#[tokio::test]
async fn list_sends_page_size_as_a_query_parameter_and_pages_forward() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/batches"))
        .and(query_param("pageSize", "1"))
        .and(wiremock::matchers::query_param_is_missing("pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "operations": [{"name": "batches/one", "metadata": {"state": "BATCH_STATE_RUNNING"}}],
            "nextPageToken": "tok-2"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1beta/batches"))
        .and(query_param("pageToken", "tok-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "operations": [{"name": "batches/two", "metadata": {"state": "BATCH_STATE_SUCCEEDED"}}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let config = ListBatchJobsConfig {
        page_size: Some(1),
        ..Default::default()
    };
    let mut pager = client.batches().list(Some(config)).await.unwrap();
    assert_eq!(pager.page()[0].name.as_deref(), Some("batches/one"));

    let second_page = pager.next_page().await.unwrap();
    assert_eq!(second_page[0].name.as_deref(), Some("batches/two"));

    let err = pager.next_page().await.unwrap_err();
    assert!(matches!(err, google_genai::Error::NoMorePages));
    server.verify().await;
}
