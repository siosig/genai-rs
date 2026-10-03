//! Ports of `batches/test_embedding.py`.

#![expect(
    clippy::unwrap_used,
    reason = "integration-test helpers outside #[test] functions may unwrap; a failure is a test-setup bug"
)]

use gemini_genai::types::{
    Content, CreateEmbeddingsBatchJobConfig, EmbedContentBatch, EmbedContentConfig,
    EmbeddingsBatchJobSource, Part,
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

const INLINE_OPERATION_NAME: &str = "batches/wdx71o8cgbzoa6gg3be1mg7g8ulrhapcjgo3";
const FILE_OPERATION_NAME: &str = "batches/507oatd242het8ox60pwsmn7tcmtkrj8itff";
const DISPLAY_NAME: &str = "test_batch";
const EMBEDDING_MODEL: &str = "gemini-embedding-001";
const EMBED_CONTENT_FILE_NAME: &str = "files/mq9e3mg3u2y5";

fn create_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "name": "batches/embed1",
        "metadata": {
            "state": "BATCH_STATE_PENDING",
            "displayName": DISPLAY_NAME,
            "model": format!("models/{EMBEDDING_MODEL}"),
        }
    }))
}

async fn mount_create(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path(format!(
            "/v1beta/models/{EMBEDDING_MODEL}:asyncBatchEmbedContent"
        )))
        .respond_with(create_response())
        .expect(1)
        .mount(server)
        .await;
}

fn inlined_embed_requests() -> EmbedContentBatch {
    EmbedContentBatch {
        config: Some(EmbedContentConfig {
            output_dimensionality: Some(64),
            ..Default::default()
        }),
        contents: Some(
            ["1", "2", "3"]
                .map(|text| Content {
                    parts: Some(vec![Part {
                        text: Some(text.to_owned()),
                        ..Default::default()
                    }]),
                    ..Default::default()
                })
                .to_vec(),
        ),
    }
}

async fn create_from_file(server: &MockServer) -> gemini_genai::types::BatchJob {
    let src = EmbeddingsBatchJobSource {
        file_name: Some(EMBED_CONTENT_FILE_NAME.to_owned()),
        ..Default::default()
    };
    let config = CreateEmbeddingsBatchJobConfig {
        display_name: Some(DISPLAY_NAME.to_owned()),
        ..Default::default()
    };
    test_client(server.uri())
        .batches()
        .create_embeddings(EMBEDDING_MODEL, src, Some(config))
        .await
        .unwrap()
}

// upstream-test: batches/test_embedding.py::test_async_from_inline
#[tokio::test]
async fn test_async_from_inline() {
    let server = MockServer::start().await;
    mount_create(&server).await;

    let src = EmbeddingsBatchJobSource {
        inlined_requests: Some(inlined_embed_requests()),
        ..Default::default()
    };
    let batch_job = test_client(server.uri())
        .batches()
        .create_embeddings(EMBEDDING_MODEL, src, None)
        .await
        .unwrap();
    assert!(batch_job.name.as_deref().unwrap().starts_with("batches/"));

    let received = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
    let requests = &body["batch"]["inputConfig"]["requests"]["requests"];
    assert_eq!(requests.as_array().map(Vec::len), Some(3));
    assert_eq!(requests[0]["request"]["outputDimensionality"], 64);
    server.verify().await;
}

// Python has sync and async variants; Rust has only the async client.
// upstream-test: batches/test_embedding.py::test_from_file
#[tokio::test]
async fn test_from_file() {
    let server = MockServer::start().await;
    mount_create(&server).await;

    let batch_job = create_from_file(&server).await;
    assert!(batch_job.name.as_deref().unwrap().starts_with("batches/"));
    assert_eq!(
        batch_job.model.as_deref(),
        Some(format!("models/{EMBEDDING_MODEL}").as_str())
    );
    server.verify().await;
}

// upstream-test: batches/test_embedding.py::test_async_from_file
#[tokio::test]
async fn test_async_from_file() {
    let server = MockServer::start().await;
    mount_create(&server).await;

    let batch_job = create_from_file(&server).await;
    assert!(batch_job.name.as_deref().unwrap().starts_with("batches/"));
    assert_eq!(
        batch_job.model.as_deref(),
        Some(format!("models/{EMBEDDING_MODEL}").as_str())
    );
    let received = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
    assert_eq!(body["batch"]["displayName"], DISPLAY_NAME);
    // Upstream's embeddings converter keeps the snake_case `file_name` key
    // (unlike generate-content batches, which send `fileName`).
    assert_eq!(
        body["batch"]["inputConfig"]["file_name"],
        EMBED_CONTENT_FILE_NAME
    );
    server.verify().await;
}

async fn get_with_output(name: &str, output: serde_json::Value) -> gemini_genai::types::BatchJob {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1beta/{name}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": name,
            "metadata": {"state": "BATCH_STATE_SUCCEEDED", "output": output}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let batch_job = test_client(server.uri())
        .batches()
        .get(name, None)
        .await
        .unwrap();
    server.verify().await;
    batch_job
}

fn inline_output() -> serde_json::Value {
    serde_json::json!({
        "inlinedEmbedContentResponses": {
            "inlinedResponses": [{"response": {"embedding": {"values": [0.1, 0.2]}}}]
        }
    })
}

fn file_output() -> serde_json::Value {
    serde_json::json!({"responsesFile": "files/embed-output"})
}

// upstream-test: batches/test_embedding.py::test_get_inline
#[tokio::test]
async fn test_get_inline() {
    let batch_job = get_with_output(INLINE_OPERATION_NAME, inline_output()).await;
    let dest = batch_job.dest.unwrap();
    assert!(dest.inlined_embed_content_responses.is_some());
}

// upstream-test: batches/test_embedding.py::test_async_get_inline
#[tokio::test]
async fn test_async_get_inline() {
    let batch_job = get_with_output(INLINE_OPERATION_NAME, inline_output()).await;
    let dest = batch_job.dest.unwrap();
    assert_eq!(
        dest.inlined_embed_content_responses.map(|r| r.len()),
        Some(1)
    );
}

// upstream-test: batches/test_embedding.py::test_get_file
#[tokio::test]
async fn test_get_file() {
    let batch_job = get_with_output(FILE_OPERATION_NAME, file_output()).await;
    assert!(batch_job.dest.unwrap().file_name.is_some());
}

// upstream-test: batches/test_embedding.py::test_async_get_file
#[tokio::test]
async fn test_async_get_file() {
    let batch_job = get_with_output(FILE_OPERATION_NAME, file_output()).await;
    assert_eq!(
        batch_job.dest.unwrap().file_name.as_deref(),
        Some("files/embed-output")
    );
}
