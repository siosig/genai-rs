//! Integration tests for `client.live()` (bidirectional realtime Live API
//! sessions over `WebSocket`). There is no `wiremock`-style mocking crate
//! for `WebSocket`s, so these drive an in-process mock `WebSocket` server
//! (`tests/common/ws_server.rs`) built directly on `tokio::net::TcpListener`
//! + `tokio_tungstenite::accept_hdr_async`.
#![expect(
    clippy::expect_used,
    reason = "test helpers shared by the per-upstream-file modules: a failure there means the mock server or test setup is broken, not the code under test"
)]
#![expect(
    clippy::large_futures,
    reason = "Live::connect's future is inherently large (WebSocket handshake + setup-message state held across await points); harmless in test code that isn't stack-constrained"
)]

#[path = "../common/mod.rs"]
mod common;

use common::{test_client_with_api_key, ws_server::start_mock_ws_server};
use futures_util::{SinkExt, StreamExt};
use gemini_genai::{
    Error,
    live::{LiveSession, RealtimeInput},
    types::{Content, FunctionResponse, LiveConnectConfig, LiveServerMessage, Modality, Part},
};
use serde_json::{Value, json};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

// One module per upstream test file (`google/genai/tests/live/test_*.py`).
mod test_live;
mod test_live_music;
mod test_live_response;
mod test_send_client_content;
mod test_send_realtime_input;
mod test_send_tool_response;

/// Model name every ported test connects with.
const TEST_MODEL: &str = "test_model";

#[expect(
    clippy::unwrap_used,
    reason = "test helper: a malformed/failed frame here means the mock server or test setup is broken, not the code under test"
)]
async fn recv_json<S>(ws: &mut S) -> Value
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    match ws.next().await {
        Some(Ok(Message::Text(text))) => serde_json::from_str(&text).unwrap(),
        other => panic!("expected a text frame, got {other:?}"),
    }
}

#[expect(
    clippy::unwrap_used,
    reason = "test helper: a malformed/failed frame here means the mock server or test setup is broken, not the code under test"
)]
async fn send_json<S>(ws: &mut S, value: Value)
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    ws.send(Message::text(value.to_string())).await.unwrap();
}

async fn send_setup_complete(ws: &mut WebSocketStream<tokio::net::TcpStream>) {
    send_json(ws, json!({ "setupComplete": {} })).await;
}

#[tokio::test]
async fn connect_uses_api_key_header_and_sends_setup_first() {
    let (base_url, server) = start_mock_ws_server(|mut ws, req| async move {
        assert!(
            req.uri.contains(
                "google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent"
            ),
            "unexpected uri: {}",
            req.uri
        );
        assert!(
            !req.uri.contains("key="),
            "the API key must travel in a header, not the URL: {}",
            req.uri
        );
        assert_eq!(req.header("x-goog-api-key"), Some("test-key"));
        assert!(
            req.header("authorization").is_none(),
            "unexpected Authorization header for a plain API key"
        );

        let setup = recv_json(&mut ws).await;
        assert_eq!(setup["setup"]["model"], "models/gemini-2.0-flash-live-001");
        assert_eq!(
            setup["setup"]["generationConfig"]["responseModalities"],
            json!(["TEXT"])
        );
        assert_eq!(
            setup["setup"]["systemInstruction"]["parts"][0]["text"],
            "be terse"
        );

        send_setup_complete(&mut ws).await;
        ws.close(None).await.ok();
    })
    .await;

    let client = test_client_with_api_key(base_url, "test-key");
    let config = LiveConnectConfig {
        response_modalities: Some(vec![Modality::Text]),
        system_instruction: Some(Content {
            parts: Some(vec![Part {
                text: Some("be terse".to_owned()),
                ..Default::default()
            }]),
            role: None,
        }),
        ..Default::default()
    };
    let session = client
        .live()
        .connect("gemini-2.0-flash-live-001", Some(config))
        .await
        .unwrap();
    assert!(session.setup_complete().is_some());

    session.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn connect_rejects_vertex_only_config_field() {
    let (base_url, server) = start_mock_ws_server(|_ws, _req| async move {
        panic!("the client should reject the request locally, before ever connecting");
    })
    .await;

    let client = test_client_with_api_key(base_url, "test-key");
    let config = LiveConnectConfig {
        explicit_vad_signal: Some(true),
        ..Default::default()
    };
    let err = client
        .live()
        .connect("gemini-2.0-flash-live-001", Some(config))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::UnsupportedByBackend {
                field: "explicit_vad_signal",
                ..
            }
        ),
        "unexpected error: {err:?}"
    );

    // The mock server never accepted a connection, so `server` never
    // finished its handler; drop it instead of awaiting.
    server.abort();
}

#[tokio::test]
async fn ephemeral_token_uses_constrained_method_and_authorization_header() {
    let (base_url, server) = start_mock_ws_server(|mut ws, req| async move {
        assert!(
            req.uri.contains("BidiGenerateContentConstrained"),
            "expected the constrained method name: {}",
            req.uri
        );
        assert!(
            !req.uri.contains("key="),
            "an ephemeral token must not appear in the URL: {}",
            req.uri
        );
        assert_eq!(
            req.header("authorization"),
            Some("Token auth_tokens/abc123")
        );

        recv_json(&mut ws).await;
        send_setup_complete(&mut ws).await;
        ws.close(None).await.ok();
    })
    .await;

    let client = test_client_with_api_key(base_url, "auth_tokens/abc123");
    let session = client
        .live()
        .connect("gemini-2.0-flash-live-001", None)
        .await
        .unwrap();
    session.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn session_sends_client_content_realtime_input_and_tool_response() {
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await; // setup
        send_setup_complete(&mut ws).await;

        let client_content = recv_json(&mut ws).await;
        assert_eq!(
            client_content,
            json!({
                "clientContent": {
                    "turns": [{"role": "user", "parts": [{"text": "hello"}]}],
                    "turnComplete": true,
                }
            })
        );

        let realtime_input = recv_json(&mut ws).await;
        assert_eq!(
            realtime_input,
            json!({ "realtimeInput": { "text": "typed input" } })
        );

        let tool_response = recv_json(&mut ws).await;
        assert_eq!(
            tool_response,
            json!({
                "toolResponse": {
                    "functionResponses": [
                        {"id": "call-1", "name": "turn_on_the_lights", "response": {"result": "ok"}}
                    ]
                }
            })
        );

        ws.close(None).await.ok();
    })
    .await;

    let client = test_client_with_api_key(base_url, "test-key");
    let mut session = client
        .live()
        .connect("gemini-2.0-flash-live-001", None)
        .await
        .unwrap();

    session
        .send_client_content(
            Some(vec![Content {
                role: Some("user".to_owned()),
                parts: Some(vec![Part {
                    text: Some("hello".to_owned()),
                    ..Default::default()
                }]),
            }]),
            true,
        )
        .await
        .unwrap();

    session
        .send_realtime_input(RealtimeInput {
            text: Some("typed input".to_owned()),
            ..Default::default()
        })
        .await
        .unwrap();

    let mut response = std::collections::HashMap::new();
    response.insert("result".to_owned(), Value::String("ok".to_owned()));
    session
        .send_tool_response(vec![FunctionResponse {
            id: Some("call-1".to_owned()),
            name: Some("turn_on_the_lights".to_owned()),
            response: Some(response),
            ..Default::default()
        }])
        .await
        .unwrap();

    server.await.unwrap();
}

#[tokio::test]
async fn send_tool_response_without_id_is_a_validation_error() {
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await; // setup
        send_setup_complete(&mut ws).await;
        // No further messages expected: the client rejects the call before sending.
        ws.close(None).await.ok();
    })
    .await;

    let client = test_client_with_api_key(base_url, "test-key");
    let mut session = client
        .live()
        .connect("gemini-2.0-flash-live-001", None)
        .await
        .unwrap();

    let err = session
        .send_tool_response(vec![FunctionResponse {
            name: Some("turn_on_the_lights".to_owned()),
            ..Default::default()
        }])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Validation(_)));

    session.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn receive_yields_server_messages_in_order_and_ends_on_server_close() {
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await; // setup
        send_setup_complete(&mut ws).await;

        send_json(
            &mut ws,
            json!({ "serverContent": { "modelTurn": { "parts": [{"text": "hi"}] } } }),
        )
        .await;
        send_json(
            &mut ws,
            json!({ "serverContent": { "modelTurn": { "parts": [{"text": "there"}] }, "turnComplete": true } }),
        )
        .await;
        ws.close(None).await.ok();
    })
    .await;

    let client = test_client_with_api_key(base_url, "test-key");
    let mut session = client
        .live()
        .connect("gemini-2.0-flash-live-001", None)
        .await
        .unwrap();

    let messages: Vec<_> = session.receive().collect().await;
    assert_eq!(messages.len(), 2);
    let first = messages[0].as_ref().unwrap();
    assert_eq!(
        first
            .server_content
            .as_ref()
            .unwrap()
            .model_turn
            .as_ref()
            .unwrap()
            .parts
            .as_ref()
            .unwrap()[0]
            .text
            .as_deref(),
        Some("hi")
    );
    let second = messages[1].as_ref().unwrap();
    assert_eq!(
        second.server_content.as_ref().unwrap().turn_complete,
        Some(true)
    );

    server.await.unwrap();
}

#[tokio::test]
async fn sending_after_the_server_closes_the_connection_fails() {
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await; // setup
        send_setup_complete(&mut ws).await;
        ws.close(None).await.ok();
    })
    .await;

    let client = test_client_with_api_key(base_url, "test-key");
    let mut session = client
        .live()
        .connect("gemini-2.0-flash-live-001", None)
        .await
        .unwrap();
    server.await.unwrap();

    // Drain the close handshake so the sink observes the connection is gone.
    let _ = session.receive().collect::<Vec<_>>().await;

    let err = session
        .send_client_content(None, true)
        .await
        .expect_err("sending on a closed connection should fail");
    assert!(matches!(err, Error::WebSocket(_)));
}

// ============================================================================
// receive_turn (spec 003-upstream-2-23-sync, US3)
// ============================================================================

#[tokio::test]
async fn receive_turn_ends_on_idle_interaction_status_even_without_turn_complete() {
    // C-1/C-2 combined: an InProgress status with turn_complete=true does
    // NOT end the turn (interaction_status, when meaningful, overrides
    // turn_complete entirely); Idle does end it, with turn_complete unset.
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await; // setup
        send_setup_complete(&mut ws).await;

        send_json(
            &mut ws,
            json!({
                "serverContent": {
                    "modelTurn": { "parts": [{"text": "still working"}] },
                    "interactionStatus": "IN_PROGRESS",
                    "turnComplete": true
                }
            }),
        )
        .await;
        send_json(
            &mut ws,
            json!({
                "serverContent": {
                    "modelTurn": { "parts": [{"text": "done"}] },
                    "interactionStatus": "IDLE"
                }
            }),
        )
        .await;
        ws.close(None).await.ok();
    })
    .await;

    let client = test_client_with_api_key(base_url, "test-key");
    let mut session = client
        .live()
        .connect("gemini-2.0-flash-live-001", None)
        .await
        .unwrap();

    let messages: Vec<_> = session.receive_turn().collect().await;
    assert_eq!(
        messages.len(),
        2,
        "both messages belong to this turn: IN_PROGRESS doesn't end it, IDLE does"
    );
    assert_eq!(
        messages[0]
            .as_ref()
            .unwrap()
            .server_content
            .as_ref()
            .unwrap()
            .model_turn
            .as_ref()
            .unwrap()
            .parts
            .as_ref()
            .unwrap()[0]
            .text
            .as_deref(),
        Some("still working")
    );
    assert_eq!(
        messages[1]
            .as_ref()
            .unwrap()
            .server_content
            .as_ref()
            .unwrap()
            .model_turn
            .as_ref()
            .unwrap()
            .parts
            .as_ref()
            .unwrap()[0]
            .text
            .as_deref(),
        Some("done"),
        "the message that completes the turn must still be yielded, not dropped"
    );

    server.await.unwrap();
}

#[tokio::test]
async fn receive_turn_falls_back_to_turn_complete_when_interaction_status_is_absent() {
    // C-3: no interaction_status at all -- same behaviour as pre-2.23.0.
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await;
        send_setup_complete(&mut ws).await;
        send_json(
            &mut ws,
            json!({ "serverContent": { "modelTurn": { "parts": [{"text": "hi"}] }, "turnComplete": true } }),
        )
        .await;
        ws.close(None).await.ok();
    })
    .await;

    let client = test_client_with_api_key(base_url, "test-key");
    let mut session = client
        .live()
        .connect("gemini-2.0-flash-live-001", None)
        .await
        .unwrap();

    let messages: Vec<_> = session.receive_turn().collect().await;
    assert_eq!(messages.len(), 1);
    server.await.unwrap();
}

#[tokio::test]
async fn receive_turn_falls_back_to_turn_complete_when_interaction_status_is_unspecified() {
    // C-4: an explicit but Unspecified interaction_status is treated as
    // "no information" -- falls back to turn_complete, same as C-3.
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await;
        send_setup_complete(&mut ws).await;
        send_json(
            &mut ws,
            json!({
                "serverContent": {
                    "modelTurn": { "parts": [{"text": "hi"}] },
                    "interactionStatus": "INTERACTION_STATUS_UNSPECIFIED",
                    "turnComplete": true
                }
            }),
        )
        .await;
        ws.close(None).await.ok();
    })
    .await;

    let client = test_client_with_api_key(base_url, "test-key");
    let mut session = client
        .live()
        .connect("gemini-2.0-flash-live-001", None)
        .await
        .unwrap();

    let messages: Vec<_> = session.receive_turn().collect().await;
    assert_eq!(messages.len(), 1);
    server.await.unwrap();
}

#[tokio::test]
async fn receive_turn_can_be_called_repeatedly_for_consecutive_turns() {
    // C-5/C-6 combined: two turns, read via two separate `receive_turn()`
    // calls on the same session, each stopping at its own boundary.
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await;
        send_setup_complete(&mut ws).await;
        send_json(
            &mut ws,
            json!({ "serverContent": { "modelTurn": { "parts": [{"text": "turn one"}] }, "turnComplete": true } }),
        )
        .await;
        send_json(
            &mut ws,
            json!({ "serverContent": { "modelTurn": { "parts": [{"text": "turn two"}] }, "turnComplete": true } }),
        )
        .await;
        ws.close(None).await.ok();
    })
    .await;

    let client = test_client_with_api_key(base_url, "test-key");
    let mut session = client
        .live()
        .connect("gemini-2.0-flash-live-001", None)
        .await
        .unwrap();

    let first_turn: Vec<_> = session.receive_turn().collect().await;
    assert_eq!(first_turn.len(), 1);
    assert_eq!(
        first_turn[0]
            .as_ref()
            .unwrap()
            .server_content
            .as_ref()
            .unwrap()
            .model_turn
            .as_ref()
            .unwrap()
            .parts
            .as_ref()
            .unwrap()[0]
            .text
            .as_deref(),
        Some("turn one")
    );

    let second_turn: Vec<_> = session.receive_turn().collect().await;
    assert_eq!(second_turn.len(), 1);
    assert_eq!(
        second_turn[0]
            .as_ref()
            .unwrap()
            .server_content
            .as_ref()
            .unwrap()
            .model_turn
            .as_ref()
            .unwrap()
            .parts
            .as_ref()
            .unwrap()[0]
            .text
            .as_deref(),
        Some("turn two")
    );

    server.await.unwrap();
}

// ============================================================================
// Shared helpers for the per-upstream-file test modules below `tests/live/`.
// ============================================================================

/// Connects to a fresh mock server, runs `send` against the session, and
/// returns `send`'s result together with the first frame the server received
/// after the `setup` handshake (`None` when the client sent nothing, e.g.
/// because `send` failed validation).
async fn run_send(
    send: impl AsyncFnOnce(&mut LiveSession) -> Result<(), Error>,
) -> (Result<(), Error>, Option<Value>) {
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<Value>>();
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await; // setup
        send_setup_complete(&mut ws).await;
        let first = match ws.next().await {
            Some(Ok(Message::Text(text))) => serde_json::from_str(&text).ok(),
            _ => None,
        };
        let _ = tx.send(first);
    })
    .await;

    let client = test_client_with_api_key(base_url, "test-key");
    let mut session = client
        .live()
        .connect(TEST_MODEL, None)
        .await
        .expect("connect to the mock server");
    let result = send(&mut session).await;
    session.close().await.ok();
    let first_frame = rx.await.ok().flatten();
    server.await.expect("mock server handler");
    (result, first_frame)
}

/// Serves `frames` (raw text frames) right after the handshake, then closes.
/// Returns the base URL to connect to.
async fn serve_frames(frames: Vec<String>) -> (String, tokio::task::JoinHandle<()>) {
    start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await; // setup
        send_setup_complete(&mut ws).await;
        for frame in frames {
            ws.send(Message::text(frame)).await.expect("send frame");
        }
        ws.close(None).await.ok();
    })
    .await
}

/// Connects to a server that replays `frames` and collects the messages
/// `receive_turn()` yields (the Rust counterpart of Python's turn-aware
/// `AsyncSession.receive`).
async fn receive_turn_of(frames: &[&str]) -> Vec<Result<LiveServerMessage, Error>> {
    let (base_url, server) = serve_frames(frames.iter().map(|f| (*f).to_owned()).collect()).await;
    let client = test_client_with_api_key(base_url, "test-key");
    let mut session = client
        .live()
        .connect(TEST_MODEL, None)
        .await
        .expect("connect to the mock server");
    let messages: Vec<_> = session.receive_turn().collect().await;
    server.await.expect("mock server handler");
    messages
}

/// Connects with `config` and returns the `{"setup": ...}` frame the client
/// sent first (Python's `get_connect_message`), or the connect error.
async fn get_connect_message(
    model: &str,
    config: Option<LiveConnectConfig>,
) -> Result<Value, Error> {
    let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        let setup = recv_json(&mut ws).await;
        let _ = tx.send(setup);
        send_json(
            &mut ws,
            json!({ "setupComplete": {"sessionId": "test_session_id"} }),
        )
        .await;
        ws.close(None).await.ok();
    })
    .await;

    let client = test_client_with_api_key(base_url, "test-key");
    match client.live().connect(model, config).await {
        Ok(session) => {
            session.close().await.ok();
            let setup = rx.await.expect("setup frame");
            server.await.expect("mock server handler");
            Ok(setup)
        }
        Err(err) => {
            // The client rejected the config before (or while) connecting.
            server.abort();
            Err(err)
        }
    }
}

/// Rewrites every object key from `snake_case` to `camelCase`. Python's
/// `LiveClientMessage._from_response` treats both spellings as the same
/// field, and the upstream setup assertions rely on that (they mix
/// `voice_config` and `voiceConfig` for the same field), so comparisons that
/// upstream makes through `_from_response` go through this helper.
fn camelize_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, val)| (snake_to_camel(&key), camelize_keys(val)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(camelize_keys).collect()),
        other => other,
    }
}

fn snake_to_camel(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut upper_next = false;
    for ch in key.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Looks `key` up in `object` ignoring case and `_` (Python's
/// `pytest_helper.get_value_ignore_key_case`): the Live wire format accepts
/// both `mime_type` and `mimeType`, and nested blobs keep the field name.
fn get_value_ignore_key_case<'a>(object: &'a Value, key: &str) -> &'a Value {
    let normalize = |k: &str| k.replace('_', "").to_lowercase();
    object
        .as_object()
        .and_then(|map| map.iter().find(|(k, _)| normalize(k) == normalize(key)))
        .map_or(&Value::Null, |(_, value)| value)
}
