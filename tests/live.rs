//! Integration tests for `client.live()` (bidirectional realtime Live API
//! sessions over `WebSocket`). There is no `wiremock`-style mocking crate
//! for `WebSocket`s, so these drive an in-process mock `WebSocket` server
//! (`tests/common/ws_server.rs`) built directly on `tokio::net::TcpListener`
//! + `tokio_tungstenite::accept_hdr_async`.
#![expect(
    clippy::large_futures,
    reason = "Live::connect's future is inherently large (WebSocket handshake + setup-message state held across await points); harmless in test code that isn't stack-constrained"
)]

mod common;

use common::ws_server::start_mock_ws_server;
use futures_util::{SinkExt, StreamExt};
use google_genai::live::RealtimeInput;
use google_genai::types::{
    Content, FunctionResponse, HttpOptions, LiveConnectConfig, Modality, Part,
};
use google_genai::{Client, Error};
use serde_json::{Value, json};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

#[expect(
    clippy::unwrap_used,
    reason = "test helper: a broken Client::builder() here is a test-setup bug, not a runtime condition"
)]
fn test_client(base_url: String, api_key: &str) -> Client {
    Client::builder()
        .api_key(api_key)
        .http_options(HttpOptions {
            base_url: Some(base_url),
            ..Default::default()
        })
        .build()
        .unwrap()
}

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
async fn connect_uses_query_key_and_sends_setup_first() {
    let (base_url, server) = start_mock_ws_server(|mut ws, req| async move {
        assert!(
            req.uri.contains(
                "google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent"
            ),
            "unexpected uri: {}",
            req.uri
        );
        assert!(
            req.uri.contains("key=test-key"),
            "missing `?key=` query param: {}",
            req.uri
        );
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

    let client = test_client(base_url, "test-key");
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

    let client = test_client(base_url, "test-key");
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

    let client = test_client(base_url, "auth_tokens/abc123");
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

    let client = test_client(base_url, "test-key");
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

    let client = test_client(base_url, "test-key");
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

    let client = test_client(base_url, "test-key");
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

    let client = test_client(base_url, "test-key");
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
