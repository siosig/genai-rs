//! Integration tests for `client.live().music()` (realtime music
//! generation sessions over WebSocket), driven against the in-process
//! mock WebSocket server (`tests/common/ws_server.rs`).

mod common;

use common::test_client;
use common::ws_server::start_mock_ws_server;
use futures_util::{SinkExt, StreamExt};
use google_genai::types::{LiveMusicGenerationConfig, WeightedPrompt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

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

#[tokio::test]
async fn connect_sends_setup_and_waits_for_setup_complete() {
    let (base_url, server) = start_mock_ws_server(|mut ws, req| async move {
        assert!(
            req.uri.contains(
                "google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateMusic"
            ),
            "unexpected uri: {}",
            req.uri
        );
        assert!(req.uri.contains("key=test-key"));

        let setup = recv_json(&mut ws).await;
        assert_eq!(
            setup,
            json!({ "setup": { "model": "models/lyria-realtime-exp" } })
        );

        send_json(&mut ws, json!({ "setupComplete": {} })).await;
        ws.close(None).await.ok();
    })
    .await;

    let client = test_client(base_url);
    let session = client
        .live()
        .music()
        .connect("lyria-realtime-exp")
        .await
        .unwrap();
    session.close().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn session_sends_weighted_prompts_config_and_playback_control() {
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await; // setup
        send_json(&mut ws, json!({ "setupComplete": {} })).await;

        let prompts = recv_json(&mut ws).await;
        assert_eq!(
            prompts,
            json!({ "clientContent": { "weightedPrompts": [{"text": "minimal", "weight": 1.0}] } })
        );

        let config = recv_json(&mut ws).await;
        assert_eq!(config, json!({ "musicGenerationConfig": { "bpm": 120 } }));

        let play = recv_json(&mut ws).await;
        assert_eq!(play, json!({ "playbackControl": "PLAY" }));

        let pause = recv_json(&mut ws).await;
        assert_eq!(pause, json!({ "playbackControl": "PAUSE" }));

        let stop = recv_json(&mut ws).await;
        assert_eq!(stop, json!({ "playbackControl": "STOP" }));

        let reset = recv_json(&mut ws).await;
        assert_eq!(reset, json!({ "playbackControl": "RESET_CONTEXT" }));

        ws.close(None).await.ok();
    })
    .await;

    let client = test_client(base_url);
    let mut session = client
        .live()
        .music()
        .connect("lyria-realtime-exp")
        .await
        .unwrap();

    session
        .set_weighted_prompts(vec![WeightedPrompt {
            text: Some("minimal".to_owned()),
            weight: Some(1.0),
        }])
        .await
        .unwrap();

    session
        .set_music_generation_config(LiveMusicGenerationConfig {
            bpm: Some(120),
            ..Default::default()
        })
        .await
        .unwrap();

    session.play().await.unwrap();
    session.pause().await.unwrap();
    session.stop().await.unwrap();
    session.reset_context().await.unwrap();

    server.await.unwrap();
}

#[tokio::test]
async fn receive_yields_server_messages_and_ends_on_server_close() {
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await; // setup
        send_json(&mut ws, json!({ "setupComplete": {} })).await;

        send_json(
            &mut ws,
            json!({ "serverContent": { "audioChunks": [{"data": "AAAA"}] } }),
        )
        .await;
        ws.close(None).await.ok();
    })
    .await;

    let client = test_client(base_url);
    let mut session = client
        .live()
        .music()
        .connect("lyria-realtime-exp")
        .await
        .unwrap();

    let messages: Vec<_> = session.receive().collect().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].as_ref().unwrap().server_content.is_some());

    server.await.unwrap();
}
