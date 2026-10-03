//! Integration tests for `client.chats()` (multi-turn chat sessions).
//! Runs against the public API only, via `wiremock`, mirroring
//! `src/chats.rs`'s own unit tests but exercised from outside the crate.

#![expect(
    clippy::unwrap_used,
    reason = "integration-test helpers outside #[test] functions may unwrap; a failure is a test-setup bug"
)]

#[path = "../common/mod.rs"]
mod common;

mod test_get_history;
mod test_send_message;
mod test_validate_response;

use common::test_client;
use futures_util::StreamExt;
use gemini_genai::types::{Content, Part};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

/// A `functionCall` reply, as the model sends when it wants a tool run.
pub(crate) fn function_call_reply(name: &str, args: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{"functionCall": {"name": name, "args": args}}]},
            "finishReason": "STOP"
        }]
    })
}

/// One SSE response whose events are `chunks`, in order.
pub(crate) fn sse_response(chunks: &[serde_json::Value]) -> ResponseTemplate {
    let body: String = chunks
        .iter()
        .map(|chunk| format!("data: {chunk}\n\n"))
        .collect::<Vec<_>>()
        .concat();
    ResponseTemplate::new(200)
        .set_body_string(body)
        .insert_header("content-type", "text/event-stream")
}

/// A streamed text chunk; `finish` adds `finishReason: STOP`.
pub(crate) fn text_chunk(text: &str, finish: bool) -> serde_json::Value {
    let mut candidate = serde_json::json!({
        "content": {"role": "model", "parts": [{"text": text}]}
    });
    if finish {
        candidate["finishReason"] = serde_json::json!("STOP");
    }
    serde_json::json!({"candidates": [candidate]})
}

/// Mounts `response` for exactly the next unanswered POST. Mocks mounted in
/// sequence answer successive requests in order.
pub(crate) async fn mount_next(server: &MockServer, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .respond_with(response)
        .up_to_n_times(1)
        .expect(1)
        .mount(server)
        .await;
}

/// Mounts `response` for every POST.
pub(crate) async fn mount_always(server: &MockServer, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .respond_with(response)
        .mount(server)
        .await;
}

/// The JSON bodies of the requests `server` has received, oldest first.
#[expect(
    clippy::unwrap_used,
    reason = "test helper: request recording is enabled and every body is JSON"
)]
pub(crate) async fn request_bodies(server: &MockServer) -> Vec<serde_json::Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| request.body_json().unwrap())
        .collect()
}

pub(crate) fn model_reply(text: &str) -> serde_json::Value {
    serde_json::json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{"text": text}]},
            "finishReason": "STOP"
        }]
    })
}

#[tokio::test]
async fn two_consecutive_send_message_calls_replay_and_accumulate_history() {
    let server = MockServer::start().await;
    // First call: contents = [user "hello"] (len 1).
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("hi there")))
        .expect(2)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let mut chat = client.chats().create("gemini-2.5-flash", None, None);

    let first = chat.send_message("hello", None).await.unwrap();
    assert_eq!(first.text().as_deref(), Some("hi there"));
    assert_eq!(chat.get_history(true).len(), 2);

    let second = chat.send_message("how are you", None).await.unwrap();
    assert_eq!(second.text().as_deref(), Some("hi there"));

    // After the second call, history holds 4 entries: user, model, user,
    // model.
    let history = chat.get_history(true);
    assert_eq!(history.len(), 4);
    assert_eq!(history[0].role.as_deref(), Some("user"));
    assert_eq!(history[1].role.as_deref(), Some("model"));
    assert_eq!(history[2].role.as_deref(), Some("user"));
    assert_eq!(history[3].role.as_deref(), Some("model"));
    assert_eq!(
        history[2].parts.as_ref().unwrap()[0].text.as_deref(),
        Some("how are you")
    );

    server.verify().await;
}

#[tokio::test]
async fn second_request_body_carries_the_full_curated_history() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("ack")))
        .expect(2)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let mut chat = client.chats().create("gemini-2.5-flash", None, None);
    chat.send_message("first", None).await.unwrap();
    chat.send_message("second", None).await.unwrap();

    // The 2nd request's `contents` array must have length 3: user, model,
    // user (the curated history from the first exchange, plus the new
    // message).
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    let second_body: serde_json::Value = requests[1].body_json().unwrap();
    let contents = second_body["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 3);
    assert_eq!(contents[0]["role"], "user");
    assert_eq!(contents[1]["role"], "model");
    assert_eq!(contents[2]["role"], "user");

    server.verify().await;
}

#[tokio::test]
async fn streaming_send_records_the_accumulated_model_reply_once_drained() {
    let server = MockServer::start().await;
    let sse_body = concat!(
        "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hel\"}]}}]}\n\n",
        "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"lo!\"}]},\"finishReason\":\"STOP\"}]}\n\n",
    );
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_body)
                .insert_header("content-type", "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let mut chat = client.chats().create("gemini-2.5-flash", None, None);

    // History is not updated until the stream is fully drained.
    {
        let mut stream = chat.send_message_stream("hi", None).await.unwrap();
        let mut collected = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.unwrap();
            if let Some(text) = chunk.text() {
                collected.push_str(&text);
            }
        }
        assert_eq!(collected, "Hello!");
    }

    // 1 user turn + 2 model turns: Python's `Chat.send_message_stream`
    // appends one `Content` per streamed chunk that carries model content
    // (`model_output.append(chunk.candidates[0].content)`), so a two-chunk
    // reply yields two separate model entries in history, not one merged
    // entry.
    let history = chat.get_history(true);
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].role.as_deref(), Some("user"));
    assert_eq!(history[1].role.as_deref(), Some("model"));
    assert_eq!(history[2].role.as_deref(), Some("model"));
    // Together, the model turns in history reflect the full streamed reply.
    let model_text: String = history[1..]
        .iter()
        .flat_map(|content| content.parts.as_ref().unwrap())
        .filter_map(|p| p.text.clone())
        .collect();
    assert_eq!(model_text, "Hello!");

    server.verify().await;
}

#[tokio::test]
async fn an_invalid_response_is_excluded_from_curated_but_kept_in_comprehensive_history() {
    let server = MockServer::start().await;
    // No candidates at all => `_validate_response` is false.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "candidates": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let mut chat = client.chats().create("gemini-2.5-flash", None, None);
    chat.send_message("hello", None).await.unwrap();

    // Curated history drops the whole (invalid) exchange.
    assert_eq!(chat.get_history(true).len(), 0);
    // Comprehensive history keeps the user turn plus a placeholder empty
    // model turn.
    let comprehensive = chat.get_history(false);
    assert_eq!(comprehensive.len(), 2);
    assert_eq!(comprehensive[0].role.as_deref(), Some("user"));
    assert_eq!(comprehensive[1].role.as_deref(), Some("model"));
    assert_eq!(comprehensive[1].parts, Some(vec![]));

    server.verify().await;
}

#[tokio::test]
async fn create_with_history_seeds_the_chat_before_any_send() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("ok")))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let seed = vec![
        Content {
            role: Some("user".to_owned()),
            parts: Some(vec![Part::from_text("previously")]),
        },
        Content {
            role: Some("model".to_owned()),
            parts: Some(vec![Part::from_text("acknowledged")]),
        },
    ];
    let mut chat = client
        .chats()
        .create("gemini-2.5-flash", None, Some(seed.clone()));

    assert_eq!(chat.get_history(true), seed.as_slice());
    assert_eq!(chat.get_history(false), seed.as_slice());

    chat.send_message("new message", None).await.unwrap();

    // The seeded turns remain, plus the new exchange.
    assert_eq!(chat.get_history(true).len(), 4);
    assert_eq!(
        chat.get_history(true)[0].parts.as_ref().unwrap()[0]
            .text
            .as_deref(),
        Some("previously")
    );

    // The request sent to the model must have replayed the seeded history.
    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = requests[0].body_json().unwrap();
    let contents = body["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 3);

    server.verify().await;
}
