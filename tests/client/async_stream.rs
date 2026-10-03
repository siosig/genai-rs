//! Ports of `client/test_async_stream.py`: how streamed responses are
//! decoded and how an error event in the middle of a stream surfaces.
//!
//! Python unit-tests a private line splitter (`HttpResponse._iter_response_stream`)
//! against mocked httpx/aiohttp responses. The crate decodes the stream as
//! Server-Sent Events, so these tests drive the same inputs through
//! `Models::generate_content_stream` against a mock server instead. Python
//! also tolerates bare JSON lines without the `data: ` prefix; the Rust
//! decoder accepts only SSE `data:` events, so the ported bodies use the
//! prefix. The sync Python tests map to the `blocking` client.

use futures_util::StreamExt;
use gemini_genai::{Error, types::GenerateContentResponse};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

type Items = Vec<Result<GenerateContentResponse, Error>>;

const STREAM_PATH: &str = "/v1beta/models/gemini-2.5-flash:streamGenerateContent";

/// A streamed chunk carrying `text`, as the service sends it.
fn chunk(text: &str) -> Value {
    json!({"candidates": [{"content": {"role": "model", "parts": [{"text": text}]}}]})
}

/// One SSE event for `value` (a single `data:` line followed by a blank line).
fn event(value: &Value) -> String {
    format!("data: {value}\n\n")
}

fn error_event() -> String {
    event(&json!({"error": {"code": 500, "message": "Error", "status": "INTERNAL"}}))
}

async fn serve(body: String) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(STREAM_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(body)
                .insert_header("content-type", "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;
    server
}

/// Collects every item of `generate_content_stream` against `body`.
async fn stream_items(body: String) -> Items {
    let server = serve(body).await;
    let stream = test_client(server.uri())
        .models()
        .generate_content_stream("gemini-2.5-flash", "hi", None)
        .await
        .unwrap();
    let items = stream.collect().await;
    server.verify().await;
    items
}

#[cfg(feature = "blocking")]
#[expect(
    clippy::unwrap_used,
    reason = "test helper: a panic on the worker thread is a test failure"
)]
async fn stream_items_blocking(body: String) -> Items {
    let server = serve(body).await;
    let base_url = server.uri();
    // The blocking client must run outside any Tokio runtime context.
    let items = std::thread::spawn(move || {
        crate::common::blocking_test_client(base_url)
            .models()
            .generate_content_stream("gemini-2.5-flash", "hi", None)
            .unwrap()
            .collect::<Items>()
    })
    .join()
    .unwrap();
    server.verify().await;
    items
}

fn texts(items: Items) -> Vec<String> {
    items
        .into_iter()
        .map(|item| item.unwrap().text().unwrap_or_default())
        .collect()
}

/// The error a stream's second item must carry: the service's in-stream
/// `{"error": ...}` event as an API error (Python's `ServerError`).
fn assert_error_event(items: Items) {
    let mut items = items.into_iter();
    let first = items.next().expect("a first chunk").unwrap();
    assert_eq!(first.text().as_deref(), Some("test"));
    let Some(Err(Error::Api(error))) = items.next() else {
        panic!("the error event must surface as Error::Api");
    };
    assert_eq!(error.code, 500);
    assert_eq!(error.status.as_deref(), Some("INTERNAL"));
    assert!(error.is_server_error());
}

fn error_event_body() -> String {
    format!("{}{}", event(&chunk("test")), error_event())
}

// upstream-test: client/test_async_stream.py::test_httpx_data_prefix
#[tokio::test]
async fn test_httpx_data_prefix() {
    // Python also yields a last event that has no terminating blank line.
    // The Rust decoder follows the SSE specification, which discards an
    // unterminated event at end of stream, so both events are terminated
    // here (the service always terminates them).
    let body = format!("{}{}", event(&chunk("hello")), event(&chunk("ok")));
    assert_eq!(texts(stream_items(body).await), ["hello", "ok"]);
}

// upstream-test: client/test_async_stream.py::test_httpx_multiline_data
#[tokio::test]
async fn test_httpx_multiline_data() {
    // One event whose JSON is spread over several `data:` lines is joined
    // with newlines into a single chunk.
    let body = concat!(
        "data: {\n",
        "data:   \"candidates\": [{\"content\": {\"role\": \"model\", \"parts\": [{\"text\": \"hello\"}]}}]\n",
        "data: }\n",
        "\n",
    );
    assert_eq!(texts(stream_items(body.to_owned()).await), ["hello"]);
}

// upstream-test: client/test_async_stream.py::test_httpx_multiline_data_sync
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_httpx_multiline_data_sync() {
    let body = concat!(
        "data: {\n",
        "data:   \"candidates\": [{\"content\": {\"role\": \"model\", \"parts\": [{\"text\": \"hello\"}]}}]\n",
        "data: }\n",
        "\n",
    );
    assert_eq!(
        texts(stream_items_blocking(body.to_owned()).await),
        ["hello"]
    );
}

// upstream-test: client/test_async_stream.py::test_httpx_multiple_json_chunk
#[tokio::test]
async fn test_httpx_multiple_json_chunk() {
    // Python's first chunk is a bare JSON line; the Rust decoder only reads
    // `data:` events, so all three carry the prefix here.
    let body: String = ["1", "2", "3"]
        .iter()
        .map(|text| event(&chunk(text)))
        .collect();
    assert_eq!(texts(stream_items(body).await), ["1", "2", "3"]);
}

// upstream-test: client/test_async_stream.py::test_httpx_empty_stream
#[tokio::test]
async fn test_httpx_empty_stream() {
    assert!(stream_items(String::new()).await.is_empty());
}

// upstream-test: client/test_async_stream.py::test_aiohttp_large_sse_line_with_thought_signature
#[tokio::test]
async fn test_aiohttp_large_sse_line_with_thought_signature() {
    // A single SSE line far beyond aiohttp's default 131072-byte limit must
    // still stream (thinking models can return very large signatures).
    let signature = "A".repeat(150_000);
    let large = json!({"candidates": [{"content": {"role": "model", "parts": [
        {"text": "", "thoughtSignature": signature}
    ]}}]});

    let items = stream_items(event(&large)).await;

    assert_eq!(items.len(), 1);
    let response = items.into_iter().next().unwrap().unwrap();
    let candidates = response.candidates.unwrap();
    let part = &candidates[0]
        .content
        .as_ref()
        .unwrap()
        .parts
        .as_ref()
        .unwrap()[0];
    assert!(
        part.thought_signature
            .as_ref()
            .is_some_and(|bytes| !bytes.is_empty()),
        "the thought signature must survive the stream"
    );
}

// upstream-test: client/test_async_stream.py::test_error_event_in_streamed_responses_bad_json
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_error_event_in_streamed_responses_bad_json() {
    let body = format!("{}data: {{\"error\": bad_json}}\n\n", event(&chunk("test")));
    let mut items = stream_items_blocking(body).await.into_iter();
    assert_eq!(
        items.next().unwrap().unwrap().text().as_deref(),
        Some("test")
    );
    assert!(
        matches!(items.next(), Some(Err(Error::Json(_)))),
        "an undecodable error event must be an error, not a chunk"
    );
}

// upstream-test: client/test_async_stream.py::test_error_event_in_streamed_responses
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_error_event_in_streamed_responses() {
    assert_error_event(stream_items_blocking(error_event_body()).await);
}

// upstream-test: client/test_async_stream.py::test_error_event_in_generate_content_stream
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_error_event_in_generate_content_stream() {
    assert_error_event(stream_items_blocking(error_event_body()).await);
}

// upstream-test: client/test_async_stream.py::test_error_event_in_streamed_responses_async
#[tokio::test]
async fn test_error_event_in_streamed_responses_async() {
    assert_error_event(stream_items(error_event_body()).await);
}

// upstream-test: client/test_async_stream.py::test_error_event_in_generate_content_stream_async
#[tokio::test]
async fn test_error_event_in_generate_content_stream_async() {
    assert_error_event(stream_items(error_event_body()).await);
}
