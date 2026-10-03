//! Port of `google/genai/tests/models/test_generate_content_model.py`
//! (plain tests; the table cases live in the oracle corpus).

use futures_util::StreamExt;
use gemini_genai::Error;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

const TUNED_MODEL_ENDPOINT: &str =
    "projects/801452371447/locations/us-central1/endpoints/4095574160837705728";
const STREAM_PATH: &str = "/v1beta/models/gemini-2.5-flash:streamGenerateContent";
const UNARY_PATH: &str = "/v1beta/models/gemini-2.5-flash:generateContent";

fn sse_chunks(texts: &[&str]) -> String {
    let last = texts.len().saturating_sub(1);
    let mut body = String::new();
    for (index, text) in texts.iter().enumerate() {
        let mut candidate = serde_json::json!({
            "content": {"role": "model", "parts": [{"text": text}]}
        });
        if index == last {
            candidate["finishReason"] = serde_json::json!("STOP");
        }
        body.push_str("data: ");
        body.push_str(&serde_json::json!({"candidates": [candidate]}).to_string());
        body.push_str("\n\n");
    }
    body
}

async fn mount_stream(server: &MockServer, texts: &[&str]) {
    Mock::given(method("POST"))
        .and(path(STREAM_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_chunks(texts))
                .insert_header("content-type", "text/event-stream"),
        )
        .expect(1)
        .mount(server)
        .await;
}

#[expect(
    clippy::unwrap_used,
    reason = "test helper: a stream that fails to open or yield here is a test failure"
)]
async fn collect_stream(
    server: &MockServer,
    model: &str,
) -> Vec<gemini_genai::types::GenerateContentResponse> {
    let mut stream = test_client(server.uri())
        .models()
        .generate_content_stream(model, "Tell me a story in 300 words.", None)
        .await
        .unwrap();
    let mut chunks = Vec::new();
    while let Some(chunk) = stream.next().await {
        chunks.push(chunk.unwrap());
    }
    chunks
}

// upstream-test: models/test_generate_content_model.py::test_tuned_model_stream
#[tokio::test]
async fn test_tuned_model_stream() {
    // A Vertex AI endpoint resource name is not served by the Gemini Developer API.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": {"code": 404, "message": "model not found", "status": "NOT_FOUND"}
        })))
        .mount(&server)
        .await;
    let client = test_client(server.uri());
    let result = client
        .models()
        .generate_content_stream(TUNED_MODEL_ENDPOINT, "Tell me a story in 300 words.", None)
        .await;
    let error = match result {
        Err(error) => error,
        Ok(mut stream) => match stream.next().await {
            Some(Err(error)) => error,
            other => panic!("expected a 404 error, got {other:?}"),
        },
    };
    assert!(
        matches!(error, Error::Api(ref api) if api.code == 404),
        "{error:?}"
    );
}

// upstream-test: models/test_generate_content_model.py::test_start_with_models_stream
#[tokio::test]
async fn test_start_with_models_stream() {
    let server = MockServer::start().await;
    mount_stream(&server, &["Once ", "upon a time."]).await;
    let chunks = collect_stream(&server, "models/gemini-2.5-flash").await;
    assert!(chunks.len() >= 2);
    for chunk in &chunks {
        let finished = chunk
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .is_some_and(|c| c.finish_reason.is_some());
        assert!(chunk.text().is_some() || finished);
    }
    server.verify().await;
}

// upstream-test: models/test_generate_content_model.py::test_models_stream_with_non_empty_last_chunk
#[tokio::test]
async fn test_models_stream_with_non_empty_last_chunk() {
    let server = MockServer::start().await;
    mount_stream(&server, &["Once ", "upon ", "the end."]).await;
    let chunks = collect_stream(&server, "gemini-2.5-flash").await;
    let last = chunks
        .last()
        .and_then(gemini_genai::types::GenerateContentResponse::text);
    assert_eq!(last.as_deref(), Some("the end."));
}

// upstream-test: models/test_generate_content_model.py::test_start_with_models_stream_async
#[tokio::test]
async fn test_start_with_models_stream_async() {
    let server = MockServer::start().await;
    mount_stream(&server, &["a ", "b ", "c ", "d."]).await;
    let chunks = collect_stream(&server, "models/gemini-2.5-flash").await;
    assert!(chunks.len() > 2);
    server.verify().await;
}

// upstream-test: models/test_generate_content_model.py::test_start_with_models_async
#[tokio::test]
async fn test_start_with_models_async() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(UNARY_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "candidates": [{"content": {"role": "model", "parts": [{"text": "ok"}]}, "finishReason": "STOP"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let response = test_client(server.uri())
        .models()
        .generate_content(
            "models/gemini-2.5-flash",
            "Tell me a story in 50 words.",
            None,
        )
        .await
        .unwrap();
    assert_eq!(response.text().as_deref(), Some("ok"));
    server.verify().await;
}
