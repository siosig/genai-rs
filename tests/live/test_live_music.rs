//! Ports of `google/genai/tests/live/test_live_music.py` (mapped to
//! `src/live_music.rs`). Music is Developer-API only upstream as well (the
//! `vertexai=True` halves just assert `NotImplementedError`, which is
//! Vertex-specific). Deviation: the Python test sends top-level keys
//! `clientContent` / `musicGenerationConfig` / `playbackControl`, which is
//! exactly what this crate sends.

use futures_util::{SinkExt, StreamExt};
use gemini_genai::{
    Error,
    live_music::LiveMusicSession,
    types::{LiveMusicGenerationConfig, MusicGenerationMode, WeightedPrompt},
};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use super::{
    common::{test_client_with_api_key, ws_server::start_mock_ws_server},
    recv_json, send_json,
};

const MUSIC_MODEL: &str = "test_model";

/// Connects a music session to a mock server, runs `op` and returns its
/// result with the first frame the server received after the handshake.
async fn run_music_send(
    op: impl AsyncFnOnce(&mut LiveMusicSession) -> Result<(), Error>,
) -> (Result<(), Error>, Option<Value>) {
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<Value>>();
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await; // setup
        send_json(&mut ws, json!({ "setupComplete": {} })).await;
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
        .music()
        .connect(MUSIC_MODEL)
        .await
        .expect("connect to the mock server");
    let result = op(&mut session).await;
    session.close().await.ok();
    let sent = rx.await.ok().flatten();
    server.await.expect("mock server handler");
    (result, sent)
}

/// The audio-chunk frame the upstream `mock_websocket` fixture returns.
fn audio_chunk_frame() -> String {
    json!({
        "serverContent": {
            "audioChunks": [{
                "data": "Z2VsYmFuYW5h",
                "mimeType": "audio/l16;rate=48000;channels=2",
                "sourceMetadata": {
                    "clientContent": { "weightedPrompts": [{ "text": "Jazz", "weight": 1 }] },
                    "musicGenerationConfig": {
                        "seed": -957_124_937,
                        "bpm": 140,
                        "scale": "A_FLAT_MAJOR_F_MINOR"
                    }
                }
            }]
        }
    })
    .to_string()
}

// upstream-test: live/test_live_music.py::test_connect_uses_header_auth_without_query_key
#[tokio::test]
async fn test_connect_uses_header_auth_without_query_key() {
    let (base_url, server) = start_mock_ws_server(|mut ws, req| async move {
        assert!(!req.uri.contains("TEST_API_KEY"), "{}", req.uri);
        assert!(!req.uri.contains("?key="), "{}", req.uri);
        assert_eq!(req.header("x-goog-api-key"), Some("TEST_API_KEY"));
        recv_json(&mut ws).await; // setup
        send_json(&mut ws, json!({ "setupComplete": {} })).await;
    })
    .await;
    let client = test_client_with_api_key(base_url, "TEST_API_KEY");
    let session = client.live().music().connect(MUSIC_MODEL).await.unwrap();
    session.close().await.ok();
    server.await.unwrap();
}

// upstream-test: live/test_live_music.py::test_websocket_base_url
#[tokio::test]
async fn test_websocket_base_url() {
    // Python: `https://test.com` -> `wss://test.com`. A TLS server is out of
    // reach for the mock, so assert the same scheme rewriting on the
    // plaintext pair (`http://` -> `ws://`: the handshake only succeeds if
    // the base URL was rewritten); the `https` -> `wss` half is covered by
    // the `websocket_endpoint` unit tests in `src/live.rs`.
    let (base_url, server) = start_mock_ws_server(|mut ws, req| async move {
        assert!(
            req.uri.starts_with("/ws/google.ai.generativelanguage."),
            "{}",
            req.uri
        );
        recv_json(&mut ws).await;
        send_json(&mut ws, json!({ "setupComplete": {} })).await;
    })
    .await;
    assert!(base_url.starts_with("http://"));
    let client = test_client_with_api_key(base_url, "google_api_key");
    let session = client.live().music().connect(MUSIC_MODEL).await.unwrap();
    session.close().await.ok();
    server.await.unwrap();
}

// upstream-test: live/test_live_music.py::test_async_session_send_weighted_prompts
#[tokio::test]
async fn test_async_session_send_weighted_prompts() {
    let (result, sent) = run_music_send(async |s| {
        s.set_weighted_prompts(vec![WeightedPrompt {
            text: Some("Jazz".to_owned()),
            weight: Some(1.0),
        }])
        .await
    })
    .await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("clientContent").is_some(), "{sent}");
    assert_eq!(sent["clientContent"]["weightedPrompts"][0]["text"], "Jazz");
    assert_eq!(sent["clientContent"]["weightedPrompts"][0]["weight"], 1.0);
}

// upstream-test: live/test_live_music.py::test_async_session_send_config
#[tokio::test]
async fn test_async_session_send_config() {
    let (result, sent) = run_music_send(async |s| {
        s.set_music_generation_config(LiveMusicGenerationConfig {
            bpm: Some(140),
            music_generation_mode: Some(MusicGenerationMode::Vocalization),
            ..Default::default()
        })
        .await
    })
    .await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("musicGenerationConfig").is_some(), "{sent}");
    assert_eq!(sent["musicGenerationConfig"]["bpm"], 140);
}

// upstream-test: live/test_live_music.py::test_async_session_control_signal_play
#[tokio::test]
async fn test_async_session_control_signal_play() {
    let (result, sent) = run_music_send(async |s| s.play().await).await;
    result.unwrap();
    assert_eq!(sent.unwrap()["playbackControl"], "PLAY");
}

// upstream-test: live/test_live_music.py::test_async_session_control_signal_pause
#[tokio::test]
async fn test_async_session_control_signal_pause() {
    let (result, sent) = run_music_send(async |s| s.pause().await).await;
    result.unwrap();
    assert_eq!(sent.unwrap()["playbackControl"], "PAUSE");
}

// upstream-test: live/test_live_music.py::test_async_session_control_signal_stop
#[tokio::test]
async fn test_async_session_control_signal_stop() {
    let (result, sent) = run_music_send(async |s| s.stop().await).await;
    result.unwrap();
    assert_eq!(sent.unwrap()["playbackControl"], "STOP");
}

// upstream-test: live/test_live_music.py::test_async_session_control_signal_reset_context
#[tokio::test]
async fn test_async_session_control_signal_reset_context() {
    let (result, sent) = run_music_send(async |s| s.reset_context().await).await;
    result.unwrap();
    assert_eq!(sent.unwrap()["playbackControl"], "RESET_CONTEXT");
}

// upstream-test: live/test_live_music.py::test_async_session_receive
#[tokio::test]
async fn test_async_session_receive() {
    let frame = audio_chunk_frame();
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await;
        send_json(&mut ws, json!({ "setupComplete": {} })).await;
        ws.send(Message::text(frame)).await.unwrap();
        ws.close(None).await.ok();
    })
    .await;
    let client = test_client_with_api_key(base_url, "test-key");
    let mut session = client.live().music().connect(MUSIC_MODEL).await.unwrap();
    let messages: Vec<_> = session.receive().collect().await;
    server.await.unwrap();

    let response = messages[0].as_ref().unwrap();
    let chunk = &response
        .server_content
        .as_ref()
        .unwrap()
        .audio_chunks
        .as_ref()
        .unwrap()[0];
    // Data is decoded from base64.
    assert_eq!(chunk.data.as_deref(), Some(b"gelbanana".as_slice()));
    assert_eq!(
        chunk.mime_type.as_deref(),
        Some("audio/l16;rate=48000;channels=2")
    );
    let metadata = chunk.source_metadata.as_ref().unwrap();
    let prompt = &metadata
        .client_content
        .as_ref()
        .unwrap()
        .weighted_prompts
        .as_ref()
        .unwrap()[0];
    assert_eq!(prompt.text.as_deref(), Some("Jazz"));
    assert_eq!(prompt.weight, Some(1.0));
    assert_eq!(
        metadata.music_generation_config.as_ref().unwrap().bpm,
        Some(140)
    );
}

// upstream-test: live/test_live_music.py::test_async_session_receive_error
#[tokio::test]
async fn test_async_session_receive_error() {
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await;
        send_json(&mut ws, json!({ "setupComplete": {} })).await;
        ws.send(Message::text("invalid json")).await.unwrap();
        ws.close(None).await.ok();
    })
    .await;
    let client = test_client_with_api_key(base_url, "test-key");
    let mut session = client.live().music().connect(MUSIC_MODEL).await.unwrap();
    let first = std::pin::pin!(session.receive()).next().await;
    server.await.unwrap();
    assert!(
        matches!(first, Some(Err(Error::Json(_)))),
        "an undecodable frame must surface as a JSON error, got {first:?}"
    );
}

// upstream-test: live/test_live_music.py::test_async_session_close
#[tokio::test]
async fn test_async_session_close() {
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await;
        send_json(&mut ws, json!({ "setupComplete": {} })).await;
        let frame = ws.next().await;
        assert!(
            matches!(frame, Some(Ok(Message::Close(_)))),
            "close() must send a WebSocket close frame, got {frame:?}"
        );
    })
    .await;
    let client = test_client_with_api_key(base_url, "test-key");
    let session = client.live().music().connect(MUSIC_MODEL).await.unwrap();
    session.close().await.unwrap();
    server.await.unwrap();
}

// upstream-test: live/test_live_music.py::test_setup_to_api
#[tokio::test]
async fn test_setup_to_api() {
    let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        let setup = recv_json(&mut ws).await;
        let _ = tx.send(setup);
        send_json(&mut ws, json!({ "setupComplete": {} })).await;
    })
    .await;
    let client = test_client_with_api_key(base_url, "test-key");
    let session = client.live().music().connect(MUSIC_MODEL).await.unwrap();
    session.close().await.ok();
    server.await.unwrap();
    assert_eq!(
        rx.await.unwrap(),
        json!({ "setup": { "model": "models/test_model" } })
    );
}
