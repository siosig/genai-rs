//! Ports of `google/genai/tests/chats/test_get_history.py`: how a chat
//! derives its curated history from a seeded or recorded comprehensive one.

use futures_util::StreamExt;
use gemini_genai::{
    Client,
    types::{Content, Part},
};
use wiremock::MockServer;

use super::{mount_always, sse_response, test_client};

/// A client that must never be asked to send anything: the tests that use
/// it only create a chat and read its history.
fn offline_client() -> Client {
    test_client("http://127.0.0.1:1".to_owned())
}

fn content(role: &str, parts: Vec<Part>) -> Content {
    Content {
        role: Some(role.to_owned()),
        parts: Some(parts),
    }
}

fn text_content(role: &str, text: &str) -> Content {
    content(role, vec![Part::from_text(text)])
}

fn assert_histories(
    chat: &gemini_genai::chats::Chat,
    comprehensive: &[Content],
    curated: &[Content],
) {
    assert_eq!(
        chat.get_history(false),
        comprehensive,
        "comprehensive history"
    );
    assert_eq!(chat.get_history(true), curated, "curated history");
}

// upstream-test: chats/test_get_history.py::test_history_start_with_valid_model_content
#[test]
fn test_history_start_with_valid_model_content() {
    let history = vec![
        text_content("model", "Hello there! how can I help you?"),
        text_content("user", "Hello"),
    ];

    let chat = offline_client()
        .chats()
        .create("gemini-2.5-flash", None, Some(history.clone()));

    assert_histories(&chat, &history, &history);
}

// upstream-test: chats/test_get_history.py::test_history_start_with_invalid_model_content
#[test]
fn test_history_start_with_invalid_model_content() {
    let history = vec![content("model", vec![]), text_content("user", "Hello")];

    let chat = offline_client()
        .chats()
        .create("gemini-2.5-flash", None, Some(history.clone()));

    assert_histories(&chat, &history, &[text_content("user", "Hello")]);
}

// upstream-test: chats/test_get_history.py::test_history_with_consecutive_valid_user_inputs
#[test]
fn test_history_with_consecutive_valid_user_inputs() {
    let history = vec![
        text_content("user", "user input 1"),
        text_content("user", "user input 2"),
    ];

    let chat = offline_client()
        .chats()
        .create("gemini-2.5-flash", None, Some(history.clone()));

    assert_histories(&chat, &history, &history);
}

// upstream-test: chats/test_get_history.py::test_history_with_valid_and_invalid_user_inputs
#[test]
fn test_history_with_valid_and_invalid_user_inputs() {
    // An empty user turn is not an "invalid model output": it stays curated.
    let history = vec![
        text_content("user", "user input 1"),
        content("user", vec![]),
        text_content("user", "user input 2"),
    ];

    let chat = offline_client()
        .chats()
        .create("gemini-2.5-flash", None, Some(history.clone()));

    assert_histories(&chat, &history, &history);
}

// upstream-test: chats/test_get_history.py::test_history_with_consecutive_valid_model_outputs
#[test]
fn test_history_with_consecutive_valid_model_outputs() {
    let history = vec![
        text_content("model", "model output 1"),
        text_content("model", "model output 2"),
    ];

    let chat = offline_client()
        .chats()
        .create("gemini-2.5-flash", None, Some(history.clone()));

    assert_histories(&chat, &history, &history);
}

// upstream-test: chats/test_get_history.py::test_history_with_valid_and_invalid_model_output
#[test]
fn test_history_with_valid_and_invalid_model_output() {
    // One invalid turn poisons the whole run of consecutive model turns.
    let history = vec![
        text_content("model", "model output 1"),
        content("model", vec![]),
        text_content("model", "model output 2"),
    ];

    let chat = offline_client()
        .chats()
        .create("gemini-2.5-flash", None, Some(history.clone()));

    assert_histories(&chat, &history, &[]);
}

// upstream-test: chats/test_get_history.py::test_history_end_with_user_input
#[test]
fn test_history_end_with_user_input() {
    let history = vec![
        text_content("user", "user input 1"),
        text_content("model", "model output"),
        text_content("user", "user input 2"),
    ];

    let chat = offline_client()
        .chats()
        .create("gemini-2.5-flash", None, Some(history.clone()));

    assert_histories(&chat, &history, &history);
}

// upstream-test: chats/test_get_history.py::test_unrecognized_role_in_history
#[test]
fn test_unrecognized_role_in_history() {
    // Deliberate deviation from Python, which raises `ValueError("Role must
    // be user or model ...")` from `Chats.create`: `create` here is
    // infallible (see `extract_curated_history` in `src/chats.rs`), so a turn
    // whose role is neither `user` nor `model` is kept as a user turn. The
    // test pins that the seed is neither rejected nor dropped.
    let history = vec![
        text_content("user", "Hello"),
        text_content("invalid_role", "Hello there! how can I help you?"),
    ];

    let chat = offline_client()
        .chats()
        .create("gemini-2.5-flash", None, Some(history.clone()));

    assert_histories(&chat, &history, &history);
}

// upstream-test: chats/test_get_history.py::test_sync_chat_create
#[test]
fn test_sync_chat_create() {
    let history = vec![
        text_content("user", "user input turn 1"),
        text_content("model", "model output turn 1"),
        text_content("model", "model output turn 1"),
        text_content("model", "user input turn 2"),
    ];

    let chat = offline_client()
        .chats()
        .create("gemini-2.5-flash", None, Some(history.clone()));

    assert_histories(&chat, &history, &history);
}

// upstream-test: chats/test_get_history.py::test_async_chat_create
#[test]
fn test_async_chat_create() {
    // Rust has a single (async) `Chat`, so this is `test_sync_chat_create`
    // with the longer upstream history.
    let history = vec![
        text_content("user", "user input turn 1"),
        text_content("model", "model output turn 1"),
        text_content("model", "model output turn 1"),
        text_content("model", "user input turn 2"),
        text_content("model", "model output turn 2"),
    ];

    let chat = offline_client()
        .chats()
        .create("gemini-2.5-flash", None, Some(history.clone()));

    assert_histories(&chat, &history, &history);
}

/// The upstream dict history, as the JSON a caller would deserialize it from.
#[expect(
    clippy::unwrap_used,
    reason = "test helper: the literal JSON below is a valid Content list"
)]
fn history_from_dicts() -> Vec<Content> {
    serde_json::from_value(serde_json::json!([
        {"role": "user", "parts": [{"text": "user input turn 1"}]},
        {"role": "model", "parts": [{"text": "model output turn 1"}]},
        {"role": "user", "parts": [{"text": "user input turn 2"}]},
        {"role": "model", "parts": [{"text": "model output turn 2"}]},
    ]))
    .unwrap()
}

fn expected_dict_history() -> Vec<Content> {
    vec![
        text_content("user", "user input turn 1"),
        text_content("model", "model output turn 1"),
        text_content("user", "user input turn 2"),
        text_content("model", "model output turn 2"),
    ]
}

// upstream-test: chats/test_get_history.py::test_sync_chat_create_with_history_dict
#[test]
fn test_sync_chat_create_with_history_dict() {
    // Python coerces dicts into `Content`; the Rust analogue is deserializing
    // the same JSON into `Content` before seeding.
    let chat =
        offline_client()
            .chats()
            .create("gemini-2.5-flash", None, Some(history_from_dicts()));

    let expected = expected_dict_history();
    assert_histories(&chat, &expected, &expected);
}

// upstream-test: chats/test_get_history.py::test_async_chat_create_with_history_dict
#[test]
fn test_async_chat_create_with_history_dict() {
    let chat =
        offline_client()
            .chats()
            .create("gemini-2.5-flash", None, Some(history_from_dicts()));

    let expected = expected_dict_history();
    assert_histories(&chat, &expected, &expected);
}

// upstream-test: chats/test_get_history.py::test_history_with_invalid_turns
#[test]
fn test_history_with_invalid_turns() {
    let valid_input = text_content("user", "Hello");
    let valid_output = vec![
        text_content("model", "Hello there! how can I help you?"),
        text_content("model", "Hello there! how can I help you?"),
    ];
    let invalid_input = text_content("user", "a input will be rejected by the model");
    let invalid_output = content("model", vec![]);

    let mut comprehensive = vec![valid_input.clone()];
    comprehensive.extend(valid_output.clone());
    comprehensive.push(invalid_input);
    comprehensive.push(invalid_output);
    let mut curated = vec![valid_input];
    curated.extend(valid_output);

    let chat =
        offline_client()
            .chats()
            .create("gemini-2.5-flash", None, Some(comprehensive.clone()));

    assert_histories(&chat, &comprehensive, &curated);
}

/// What `send_message("Hello")` leaves in the comprehensive history when the
/// model answers with `reply_parts`.
fn exchange(reply_parts: Vec<Part>) -> Vec<Content> {
    vec![text_content("user", "Hello"), content("model", reply_parts)]
}

// upstream-test: chats/test_get_history.py::test_chat_with_empty_text_part
#[tokio::test]
async fn test_chat_with_empty_text_part() {
    let server = MockServer::start().await;
    mount_always(
        &server,
        wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "candidates": [{"content": {"role": "model", "parts": [{"text": ""}]}}]
        })),
    )
    .await;
    let mut chat = test_client(server.uri())
        .chats()
        .create("gemini-2.5-flash", None, None);

    chat.send_message("Hello", None).await.unwrap();

    // An empty *text* is a real part, unlike an empty `Part`: the exchange
    // stays in the curated history.
    let expected = exchange(vec![Part::from_text("")]);
    assert_histories(&chat, &expected, &expected);
}

// upstream-test: chats/test_get_history.py::test_chat_with_empty_content
#[tokio::test]
async fn test_chat_with_empty_content() {
    let server = MockServer::start().await;
    mount_always(
        &server,
        wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"candidates": []})),
    )
    .await;
    let mut chat = test_client(server.uri())
        .chats()
        .create("gemini-2.5-flash", None, None);

    chat.send_message("Hello", None).await.unwrap();

    assert_histories(&chat, &exchange(vec![]), &[]);
}

// upstream-test: chats/test_get_history.py::test_chat_stream_with_empty_text_part
#[tokio::test]
async fn test_chat_stream_with_empty_text_part() {
    let server = MockServer::start().await;
    mount_always(
        &server,
        sse_response(&[serde_json::json!({
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": ""}]},
                "finishReason": "STOP"
            }]
        })]),
    )
    .await;
    let mut chat = test_client(server.uri())
        .chats()
        .create("gemini-2.5-flash", None, None);

    let mut stream = chat.send_message_stream("Hello", None).await.unwrap();
    while let Some(chunk) = stream.next().await {
        chunk.unwrap();
    }
    drop(stream);

    let expected = exchange(vec![Part::from_text("")]);
    assert_histories(&chat, &expected, &expected);
}

// upstream-test: chats/test_get_history.py::test_chat_stream_with_empty_content
#[tokio::test]
async fn test_chat_stream_with_empty_content() {
    let server = MockServer::start().await;
    mount_always(
        &server,
        sse_response(&[serde_json::json!({"candidates": []})]),
    )
    .await;
    let mut chat = test_client(server.uri())
        .chats()
        .create("gemini-2.5-flash", None, None);

    let mut stream = chat.send_message_stream("Hello", None).await.unwrap();
    while let Some(chunk) = stream.next().await {
        chunk.unwrap();
    }
    drop(stream);

    assert_histories(&chat, &exchange(vec![]), &[]);
}
