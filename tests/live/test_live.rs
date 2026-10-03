//! Ports of `google/genai/tests/live/test_live.py`.
//!
//! Developer-API behaviour only: the Vertex AI halves of the parameterised
//! upstream cases, the deprecated `send()` / `start_stream()` /
//! `_parse_client_message` surface and the Python-only MCP/callable tool
//! inputs are excluded in `tools/codegen/upstream_tests.toml`.
//!
//! Where upstream builds the config from a Python dict, these tests build the
//! same dict as JSON and deserialize it into `LiveConnectConfig` (the generated
//! types accept the `snake_case` field names), which mirrors the dict path.
//! Setup assertions that upstream makes through
//! `LiveClientMessage._from_response` (which accepts `snake_case` and `camelCase`
//! spellings of one field interchangeably) go through `camelize_keys`.

use futures_util::{SinkExt, StreamExt};
use gemini_genai::{
    Error,
    types::{
        InteractionStatus, LiveConnectConfig, LiveServerGoAway, LiveServerMessage,
        LiveServerSessionResumptionUpdate, VadSignalType,
    },
};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use super::{
    TEST_MODEL, camelize_keys,
    common::{test_client_with_api_key, ws_server::start_mock_ws_server},
    get_connect_message, receive_turn_of, recv_json, send_json,
};

const TURN_COMPLETE_FRAME: &str = r#"{"serverContent": {"turnComplete": true}}"#;

fn config_from(dict: Value) -> LiveConnectConfig {
    serde_json::from_value(dict).expect("config dict deserializes into LiveConnectConfig")
}

/// The `setup` frame for `model` `test_model` with `config` (`None` = no config).
async fn setup_of(config: Option<LiveConnectConfig>) -> Value {
    get_connect_message(TEST_MODEL, config)
        .await
        .expect("connect succeeds")
}

fn assert_setup_eq(actual: Value, expected: Value) {
    assert_eq!(camelize_keys(actual), camelize_keys(expected));
}

fn system_instruction(text: &str) -> Value {
    json!({ "parts": [{ "text": text }], "role": "user" })
}

fn text_of(message: &LiveServerMessage) -> Option<&str> {
    message
        .server_content
        .as_ref()?
        .model_turn
        .as_ref()?
        .parts
        .as_ref()?
        .first()?
        .text
        .as_deref()
}

// upstream-test: live/test_live.py::test_websocket_base_url
#[tokio::test]
async fn test_websocket_base_url() {
    // Python: `https://test.com` -> `wss://test.com`. The mock server is
    // plaintext, so this checks the `http://` -> `ws://` rewrite (the
    // handshake only succeeds if the base URL was rewritten); `https` ->
    // `wss` is covered by the `websocket_endpoint` unit tests in `src/live.rs`.
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
    let session = client.live().connect(TEST_MODEL, None).await.unwrap();
    session.close().await.ok();
    server.await.unwrap();
}

// upstream-test: live/test_live.py::test_async_session_receive
#[tokio::test]
async fn test_async_session_receive() {
    let messages = receive_turn_of(&[TURN_COMPLETE_FRAME]).await;
    assert_eq!(
        messages[0]
            .as_ref()
            .unwrap()
            .server_content
            .as_ref()
            .unwrap()
            .turn_complete,
        Some(true)
    );
}

// upstream-test: live/test_live.py::test_async_session_receive_error
#[tokio::test]
async fn test_async_session_receive_error() {
    let messages = receive_turn_of(&["invalid json"]).await;
    assert!(
        matches!(messages.first(), Some(Err(Error::Json(_)))),
        "an undecodable frame must surface as a JSON error, got {messages:?}"
    );
}

// upstream-test: live/test_live.py::test_async_session_receive_text
#[tokio::test]
async fn test_async_session_receive_text() {
    let messages = receive_turn_of(&[
        r#"{"serverContent": {"modelTurn": {"parts":[{"text": "test"}]}}}"#,
        TURN_COMPLETE_FRAME,
    ])
    .await;
    assert_eq!(text_of(messages[0].as_ref().unwrap()), Some("test"));
    assert_eq!(
        messages[1]
            .as_ref()
            .unwrap()
            .server_content
            .as_ref()
            .unwrap()
            .turn_complete,
        Some(true)
    );
}

// upstream-test: live/test_live.py::test_async_session_receive_audio
#[tokio::test]
async fn test_async_session_receive_audio() {
    let messages = receive_turn_of(&[
        r#"{"serverContent": {"modelTurn": {"parts":[{"inlineData": {"data": "MDAwMDAw", "mimeType": "audio/pcm" }}]}}}"#,
        TURN_COMPLETE_FRAME,
    ])
    .await;
    let part = &messages[0]
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
        .unwrap()[0];
    let inline = part.inline_data.as_ref().unwrap();
    assert_eq!(inline.mime_type.as_deref(), Some("audio/pcm"));
    assert_eq!(inline.data.as_deref(), Some(b"000000".as_slice()));
}

// upstream-test: live/test_live.py::test_async_session_receive_tool_call
#[tokio::test]
async fn test_async_session_receive_tool_call() {
    let messages = receive_turn_of(&[
        r#"{"toolCall": {"functionCalls": [{"name": "get_current_weather", "args": {"location": "San Francisco", "unit": "C"}}]}}"#,
        TURN_COMPLETE_FRAME,
    ])
    .await;
    let call = &messages[0]
        .as_ref()
        .unwrap()
        .tool_call
        .as_ref()
        .unwrap()
        .function_calls
        .as_ref()
        .unwrap()[0];
    assert_eq!(call.name.as_deref(), Some("get_current_weather"));
    let args = call.args.as_ref().unwrap();
    assert_eq!(args["location"], "San Francisco");
    assert_eq!(args["unit"], "C");
}

// upstream-test: live/test_live.py::test_async_session_receive_transcription
#[tokio::test]
async fn test_async_session_receive_transcription() {
    let messages = receive_turn_of(&[
        r#"{"serverContent": {"inputTranscription": {"text": "test_input", "finished": true}}}"#,
        r#"{"serverContent": {"outputTranscription": {"text": "test_output", "finished": false}}}"#,
        TURN_COMPLETE_FRAME,
    ])
    .await;
    let input = messages[0]
        .as_ref()
        .unwrap()
        .server_content
        .as_ref()
        .unwrap()
        .input_transcription
        .as_ref()
        .unwrap();
    assert_eq!(input.text.as_deref(), Some("test_input"));
    assert_eq!(input.finished, Some(true));
    let output = messages[1]
        .as_ref()
        .unwrap()
        .server_content
        .as_ref()
        .unwrap()
        .output_transcription
        .as_ref()
        .unwrap();
    assert_eq!(output.text.as_deref(), Some("test_output"));
    assert_eq!(output.finished, Some(false));
}

// upstream-test: live/test_live.py::test_async_go_away
#[tokio::test]
async fn test_async_go_away() {
    let messages =
        receive_turn_of(&[r#"{"goAway": {"timeLeft": "10s"}}"#, TURN_COMPLETE_FRAME]).await;
    let expected = LiveServerMessage {
        go_away: Some(LiveServerGoAway {
            time_left: Some("10s".to_owned()),
        }),
        ..Default::default()
    };
    assert_eq!(messages[0].as_ref().unwrap(), &expected);
}

// upstream-test: live/test_live.py::test_async_session_resumption_update
#[tokio::test]
async fn test_async_session_resumption_update() {
    // Upstream's fixture also sends `"resumable": "true"` (a string), which
    // pydantic's lax mode coerces to a bool. Rust's bool deserialization is
    // strict and the real server sends a JSON bool, so the fixture uses `true`;
    // the string-typed int64 index (proto3 JSON) is kept as upstream has it.
    let messages = receive_turn_of(&[
        r#"{"sessionResumptionUpdate": {"newHandle": "test_handle", "resumable": true, "lastConsumedClientMessageIndex": "123456789"}}"#,
        TURN_COMPLETE_FRAME,
    ])
    .await;
    let expected = LiveServerMessage {
        session_resumption_update: Some(LiveServerSessionResumptionUpdate {
            new_handle: Some("test_handle".to_owned()),
            resumable: Some(true),
            last_consumed_client_message_index: Some(123_456_789),
        }),
        ..Default::default()
    };
    assert_eq!(messages[0].as_ref().unwrap(), &expected);
}

/// Number of messages `receive_turn` yields for `first` followed by a
/// completing sentinel: 1 when `first` itself ends the turn, 2 otherwise.
async fn messages_until_turn_ends(first: &str) -> usize {
    receive_turn_of(&[first, TURN_COMPLETE_FRAME]).await.len()
}

// upstream-test: live/test_live.py::test_is_interaction_complete
#[tokio::test]
async fn test_is_interaction_complete() {
    // `_is_interaction_complete` is private upstream and here; it is observed
    // through `receive_turn`, which stops after the message it reports complete.
    let cases = [
        // (message, ends the turn?)
        (r#"{"usageMetadata": {}}"#, false), // no server_content
        (r#"{"serverContent": {"turnComplete": false}}"#, false),
        (r#"{"serverContent": {"turnComplete": true}}"#, true),
        (
            r#"{"serverContent": {"turnComplete": true, "interactionStatus": "IN_PROGRESS"}}"#,
            false,
        ),
        (
            r#"{"serverContent": {"turnComplete": true, "interactionStatus": "IDLE"}}"#,
            true,
        ),
        (
            r#"{"serverContent": {"turnComplete": true, "interactionStatus": "INTERACTION_STATUS_UNSPECIFIED"}}"#,
            true,
        ),
        (
            r#"{"serverContent": {"turnComplete": false, "interactionStatus": "INTERACTION_STATUS_UNSPECIFIED"}}"#,
            false,
        ),
    ];
    for (frame, ends_turn) in cases {
        let expected = if ends_turn { 1 } else { 2 };
        assert_eq!(
            messages_until_turn_ends(frame).await,
            expected,
            "wrong turn boundary for {frame}"
        );
    }
}

// upstream-test: live/test_live.py::test_async_session_receive_interaction_status_idle
#[tokio::test]
async fn test_async_session_receive_interaction_status_idle() {
    let messages = receive_turn_of(&[
        r#"{"serverContent": {"modelTurn": {"parts":[{"text": "hello"}]}}}"#,
        r#"{"serverContent": {"turnComplete": true, "interactionStatus": "IDLE"}}"#,
    ])
    .await;
    assert_eq!(messages.len(), 2);
    assert_eq!(text_of(messages[0].as_ref().unwrap()), Some("hello"));
    let content = messages[1]
        .as_ref()
        .unwrap()
        .server_content
        .as_ref()
        .unwrap();
    assert_eq!(content.turn_complete, Some(true));
    assert_eq!(content.interaction_status, Some(InteractionStatus::Idle));
}

// upstream-test: live/test_live.py::test_async_session_receive_interaction_status_in_progress_then_idle
#[tokio::test]
async fn test_async_session_receive_interaction_status_in_progress_then_idle() {
    let messages = receive_turn_of(&[
        r#"{"serverContent": {"modelTurn": {"parts":[{"text": "thinking..."}]}}}"#,
        r#"{"serverContent": {"turnComplete": true, "interactionStatus": "IN_PROGRESS"}}"#,
        r#"{"serverContent": {"modelTurn": {"parts":[{"text": "answer"}]}}}"#,
        r#"{"serverContent": {"turnComplete": true, "interactionStatus": "IDLE"}}"#,
    ])
    .await;
    assert_eq!(messages.len(), 4);
    assert_eq!(text_of(messages[0].as_ref().unwrap()), Some("thinking..."));
    let second = messages[1]
        .as_ref()
        .unwrap()
        .server_content
        .as_ref()
        .unwrap();
    assert_eq!(second.turn_complete, Some(true));
    assert_eq!(
        second.interaction_status,
        Some(InteractionStatus::InProgress)
    );
    assert_eq!(text_of(messages[2].as_ref().unwrap()), Some("answer"));
    let fourth = messages[3]
        .as_ref()
        .unwrap()
        .server_content
        .as_ref()
        .unwrap();
    assert_eq!(fourth.turn_complete, Some(true));
    assert_eq!(fourth.interaction_status, Some(InteractionStatus::Idle));
}

// upstream-test: live/test_live.py::test_async_session_receive_vad_signal
#[tokio::test]
async fn test_async_session_receive_vad_signal() {
    let messages = receive_turn_of(&[
        r#"{"voiceActivityDetectionSignal": {"vadSignalType": "VAD_SIGNAL_TYPE_SOS"}}"#,
        TURN_COMPLETE_FRAME,
    ])
    .await;
    assert!(!messages.is_empty());
    let signal = messages[0]
        .as_ref()
        .unwrap()
        .voice_activity_detection_signal
        .as_ref()
        .unwrap();
    assert_eq!(
        signal.vad_signal_type,
        Some(VadSignalType::VadSignalTypeSos)
    );
    // The session can finish cleanly: the last message completes the turn.
    assert_eq!(
        messages
            .last()
            .unwrap()
            .as_ref()
            .unwrap()
            .server_content
            .as_ref()
            .unwrap()
            .turn_complete,
        Some(true)
    );
}

// upstream-test: live/test_live.py::test_async_session_close
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
    let session = client.live().connect(TEST_MODEL, None).await.unwrap();
    session.close().await.unwrap();
    server.await.unwrap();
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_no_config
#[tokio::test]
async fn test_bidi_setup_to_api_no_config() {
    assert_eq!(
        setup_of(None).await,
        json!({ "setup": { "model": "models/test_model" } })
    );
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_speech_config
#[tokio::test]
async fn test_bidi_setup_to_api_speech_config() {
    // Python's `system_instruction='test instruction'` string becomes a
    // `role: "user"` Content; Rust takes the typed `Content` directly.
    let expected = json!({
        "setup": {
            "model": "models/test_model",
            "generationConfig": {
                "speechConfig": {
                    "voice_config": { "prebuilt_voice_config": { "voice_name": "en-default" } },
                    "language_code": "en-US"
                },
                "enableAffectiveDialog": true,
                "temperature": 0.7,
                "topP": 0.8,
                "topK": 9.0,
                "maxOutputTokens": 10,
                "mediaResolution": "MEDIA_RESOLUTION_MEDIUM",
                "seed": 13
            },
            "proactivity": { "proactive_audio": true },
            "systemInstruction": system_instruction("test instruction")
        }
    });
    let config = config_from(json!({
        "speech_config": {
            "voice_config": { "prebuilt_voice_config": { "voice_name": "en-default" } },
            "language_code": "en-US"
        },
        "enable_affective_dialog": true,
        "proactivity": { "proactive_audio": true },
        "temperature": 0.7,
        "top_p": 0.8,
        "top_k": 9,
        "max_output_tokens": 10,
        "seed": 13,
        "system_instruction": system_instruction("test instruction"),
        "media_resolution": "MEDIA_RESOLUTION_MEDIUM"
    }));
    assert_setup_eq(setup_of(Some(config)).await, expected);
}

// upstream-test: live/test_live.py::test_bidi_setup_error_if_multispeaker_voice_config
#[tokio::test]
async fn test_bidi_setup_error_if_multispeaker_voice_config() {
    let config = config_from(json!({
        "speech_config": {
            "multi_speaker_voice_config": {
                "speaker_voice_configs": [
                    { "speaker": "Alice", "voice_config": { "prebuilt_voice_config": { "voice_name": "leda" } } },
                    { "speaker": "Bob", "voice_config": { "prebuilt_voice_config": { "voice_name": "kore" } } }
                ]
            }
        },
        "temperature": 0.7,
        "max_output_tokens": 10,
        "system_instruction": system_instruction("test instruction"),
        "media_resolution": "MEDIA_RESOLUTION_MEDIUM"
    }));
    let err = get_connect_message(TEST_MODEL, Some(config))
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("multi_speaker_voice_config"),
        "unexpected error: {err}"
    );
}

// upstream-test: live/test_live.py::test_explicit_vad
#[tokio::test]
async fn test_explicit_vad() {
    // Developer API: `explicit_vad_signal` is Vertex-only and rejected.
    let config = config_from(json!({ "explicit_vad_signal": true }));
    let err = get_connect_message(TEST_MODEL, Some(config))
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
}

// upstream-test: live/test_live.py::test_explicit_vad_config
#[tokio::test]
async fn test_explicit_vad_config() {
    let config = config_from(json!({ "explicit_vad_signal": true }));
    let err = get_connect_message(TEST_MODEL, Some(config))
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
}

// upstream-test: live/test_live.py::test_history_config
#[tokio::test]
async fn test_history_config() {
    let config =
        config_from(json!({ "history_config": { "initial_history_in_client_content": true } }));
    let setup = setup_of(Some(config)).await;
    assert_eq!(
        camelize_keys(setup["setup"]["historyConfig"].clone()),
        json!({ "initialHistoryInClientContent": true })
    );
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_system_instruction_as_content_type
#[tokio::test]
async fn test_bidi_setup_to_api_with_system_instruction_as_content_type() {
    let config =
        config_from(json!({ "system_instruction": system_instruction("test instruction") }));
    assert_eq!(
        setup_of(Some(config)).await,
        json!({ "setup": {
            "model": "models/test_model",
            "systemInstruction": system_instruction("test instruction"),
        }})
    );
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_config_tools_google_search
#[tokio::test]
async fn test_bidi_setup_to_api_with_config_tools_google_search() {
    let dict = json!({
        "response_modalities": ["TEXT"],
        "system_instruction": system_instruction("test instruction"),
        "generation_config": { "temperature": 0.7 },
        "tools": [{ "google_search": {} }]
    });
    let expected = json!({ "setup": {
        "model": "models/test_model",
        "generationConfig": { "temperature": 0.7, "responseModalities": ["TEXT"] },
        "systemInstruction": system_instruction("test instruction"),
        "tools": [{ "googleSearch": {} }],
    }});
    assert_eq!(setup_of(Some(config_from(dict.clone()))).await, expected);
    // Same config built a second time (upstream repeats it as a typed config).
    assert_eq!(setup_of(Some(config_from(dict))).await, expected);
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_context_window_compression
#[tokio::test]
async fn test_bidi_setup_to_api_with_context_window_compression() {
    let config = config_from(json!({
        "generation_config": { "temperature": 0.7 },
        "response_modalities": ["TEXT"],
        "system_instruction": system_instruction("test instruction"),
        "context_window_compression": {
            "trigger_tokens": 1000,
            "sliding_window": { "target_tokens": 10 }
        }
    }));
    let expected = json!({ "setup": {
        "model": "models/test_model",
        "generationConfig": { "temperature": 0.7, "responseModalities": ["TEXT"] },
        "systemInstruction": system_instruction("test instruction"),
        "contextWindowCompression": {
            "trigger_tokens": 1000,
            "sliding_window": { "target_tokens": 10 }
        }
    }});
    assert_setup_eq(setup_of(Some(config)).await, expected);
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_config_tools_function_declaration
#[tokio::test]
async fn test_bidi_setup_to_api_with_config_tools_function_declaration() {
    let declarations = json!([{
        "name": "get_current_weather",
        "description": "Get the current weather in a city",
        "parameters": {
            "type": "OBJECT",
            "properties": {
                "location": { "type": "STRING", "description": "The location to get the weather for" },
                "unit": { "type": "STRING", "enum": ["C", "F"] }
            }
        }
    }]);
    let config = config_from(json!({
        "generation_config": { "temperature": 0.7 },
        "tools": [{ "function_declarations": declarations }]
    }));
    let setup = setup_of(Some(config)).await;
    assert_eq!(
        setup["setup"]["tools"][0]["functionDeclarations"],
        declarations
    );
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_tools_function_behavior
#[tokio::test]
async fn test_bidi_setup_to_api_with_tools_function_behavior() {
    // Upstream derives the declaration with `FunctionDeclaration.from_callable`
    // (a Python-only introspection); here the declaration is spelled out.
    let config = config_from(json!({
        "generation_config": { "temperature": 0.7 },
        "tools": [{ "function_declarations": [{
            "name": "get_current_weather",
            "description": "Get the current weather in a city.",
            "behavior": "NON_BLOCKING"
        }]}]
    }));
    let setup = setup_of(Some(config)).await;
    assert_eq!(
        setup["setup"]["tools"][0]["functionDeclarations"][0]["behavior"],
        "NON_BLOCKING"
    );
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_config_tools_code_execution
#[tokio::test]
async fn test_bidi_setup_to_api_with_config_tools_code_execution() {
    let config = config_from(json!({ "tools": [{ "code_execution": {} }] }));
    let setup = setup_of(Some(config)).await;
    assert_eq!(setup["setup"]["tools"][0], json!({ "codeExecution": {} }));
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_realtime_input_config
#[tokio::test]
async fn test_bidi_setup_to_api_with_realtime_input_config() {
    let realtime_input_config = json!({
        "automatic_activity_detection": {
            "disabled": true,
            "start_of_speech_sensitivity": "START_SENSITIVITY_HIGH",
            "end_of_speech_sensitivity": "END_SENSITIVITY_HIGH",
            "prefix_padding_ms": 20,
            "silence_duration_ms": 100
        },
        "activity_handling": "NO_INTERRUPTION",
        "turn_coverage": "TURN_INCLUDES_ALL_INPUT"
    });
    let config = config_from(json!({ "realtime_input_config": realtime_input_config }));
    let setup = setup_of(Some(config)).await;
    assert_eq!(
        camelize_keys(setup["setup"]["realtimeInputConfig"].clone()),
        camelize_keys(realtime_input_config)
    );
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_input_transcription
#[tokio::test]
async fn test_bidi_setup_to_api_with_input_transcription() {
    let config = config_from(json!({ "input_audio_transcription": {} }));
    let setup = setup_of(Some(config)).await;
    assert_eq!(setup["setup"]["inputAudioTranscription"], json!({}));
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_output_transcription
#[tokio::test]
async fn test_bidi_setup_to_api_with_output_transcription() {
    let config = config_from(json!({ "output_audio_transcription": {} }));
    let setup = setup_of(Some(config)).await;
    assert_eq!(setup["setup"]["outputAudioTranscription"], json!({}));
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_media_resolution
#[tokio::test]
async fn test_bidi_setup_to_api_with_media_resolution() {
    let config = config_from(json!({ "media_resolution": "MEDIA_RESOLUTION_LOW" }));
    let setup = setup_of(Some(config)).await;
    assert_eq!(
        setup["setup"]["generationConfig"]["mediaResolution"],
        "MEDIA_RESOLUTION_LOW"
    );
}

// upstream-test: live/test_live.py::test_bidi_setup_generation_config_warning
#[tokio::test]
async fn test_bidi_setup_generation_config_warning() {
    // Python also emits a DeprecationWarning for `generation_config`; Rust has
    // no warnings channel, so only the wire behaviour is checked.
    let config = config_from(json!({ "generation_config": { "temperature": 0.7 } }));
    let setup = get_connect_message("models/test_model", Some(config))
        .await
        .unwrap();
    assert_eq!(setup["setup"]["generationConfig"]["temperature"], 0.7);
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_session_resumption
#[tokio::test]
async fn test_bidi_setup_to_api_with_session_resumption() {
    let config = config_from(json!({ "session_resumption": { "handle": "test_handle" } }));
    assert_eq!(
        setup_of(Some(config)).await,
        json!({ "setup": {
            "model": "models/test_model",
            "sessionResumption": { "handle": "test_handle" },
        }})
    );
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_transparent_session_resumption
#[tokio::test]
async fn test_bidi_setup_to_api_with_transparent_session_resumption() {
    // `transparent` is Vertex-only: the Developer API rejects it.
    let config = config_from(json!({
        "session_resumption": { "handle": "test_handle", "transparent": true }
    }));
    let err = get_connect_message(TEST_MODEL, Some(config))
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::UnsupportedByBackend { .. }),
        "unexpected error: {err:?}"
    );
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_translation_config
#[tokio::test]
async fn test_bidi_setup_to_api_with_translation_config() {
    let expected = json!({ "setup": {
        "model": "models/test_model",
        "generationConfig": {
            "translationConfig": { "echoTargetLanguage": true, "targetLanguageCode": "es" }
        }
    }});
    let dict = json!({
        "translation_config": { "echo_target_language": true, "target_language_code": "es" }
    });
    assert_eq!(setup_of(Some(config_from(dict))).await, expected);

    let typed = LiveConnectConfig {
        translation_config: Some(gemini_genai::types::TranslationConfig {
            echo_target_language: Some(true),
            target_language_code: Some("es".to_owned()),
        }),
        ..Default::default()
    };
    assert_eq!(setup_of(Some(typed)).await, expected);
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_thinking_config
#[tokio::test]
async fn test_bidi_setup_to_api_with_thinking_config() {
    let config = config_from(json!({
        "thinking_config": { "include_thoughts": true, "thinking_budget": 1024 }
    }));
    let expected = json!({ "setup": {
        "model": "models/test_model",
        "generationConfig": { "thinkingConfig": { "include_thoughts": true, "thinking_budget": 1024 } }
    }});
    assert_setup_eq(setup_of(Some(config)).await, expected);
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_auth_tokens
#[tokio::test]
async fn test_bidi_setup_to_api_with_auth_tokens() {
    let (base_url, server) = start_mock_ws_server(|mut ws, req| async move {
        assert_eq!(
            req.header("authorization"),
            Some("Token auth_tokens/TEST_AUTH_TOKEN")
        );
        assert!(
            req.uri.contains("BidiGenerateContentConstrained"),
            "{}",
            req.uri
        );
        recv_json(&mut ws).await;
        send_json(
            &mut ws,
            json!({ "setupComplete": {"sessionId": "test_session_id"} }),
        )
        .await;
    })
    .await;
    let client = test_client_with_api_key(base_url, "auth_tokens/TEST_AUTH_TOKEN");
    let session = client.live().connect(TEST_MODEL, None).await.unwrap();
    session.close().await.ok();
    server.await.unwrap();
}

// upstream-test: live/test_live.py::test_bidi_setup_to_api_with_api_key
#[tokio::test]
async fn test_bidi_setup_to_api_with_api_key() {
    let (base_url, server) = start_mock_ws_server(|mut ws, req| async move {
        assert_eq!(req.header("x-goog-api-key"), Some("TEST_API_KEY"));
        assert!(req.uri.contains("BidiGenerateContent"), "{}", req.uri);
        assert!(!req.uri.contains("Constrained"), "{}", req.uri);
        recv_json(&mut ws).await;
        send_json(
            &mut ws,
            json!({ "setupComplete": {"sessionId": "test_session_id"} }),
        )
        .await;
    })
    .await;
    let client = test_client_with_api_key(base_url, "TEST_API_KEY");
    let session = client.live().connect(TEST_MODEL, None).await.unwrap();
    session.close().await.ok();
    server.await.unwrap();
}

// upstream-test: live/test_live.py::test_async_session_setup_complete_with_voice_consent_signature
#[tokio::test]
async fn test_async_session_setup_complete_with_voice_consent_signature() {
    let (base_url, server) = start_mock_ws_server(|mut ws, _req| async move {
        recv_json(&mut ws).await;
        ws.send(Message::text(
            r#"{"setupComplete": {"sessionId": "test_session_id", "voiceConsentSignature": {"signature": "test_sig_abc123"}}}"#,
        ))
        .await
        .unwrap();
    })
    .await;
    let client = test_client_with_api_key(base_url, "test-key");
    let session = client.live().connect(TEST_MODEL, None).await.unwrap();
    let setup_complete = session.setup_complete().unwrap();
    assert_eq!(
        setup_complete.session_id.as_deref(),
        Some("test_session_id")
    );
    assert_eq!(
        setup_complete
            .voice_consent_signature
            .as_ref()
            .unwrap()
            .signature
            .as_deref(),
        Some("test_sig_abc123")
    );
    session.close().await.ok();
    server.await.unwrap();
}

// upstream-test: live/test_live.py::test_bidi_setup_replicated_voice_config_with_consent
#[tokio::test]
async fn test_bidi_setup_replicated_voice_config_with_consent() {
    let config = config_from(json!({
        "response_modalities": ["AUDIO"],
        "speech_config": { "voice_config": { "replicated_voice_config": {
            "mime_type": "audio/wav",
            "voice_sample_audio": "ZmFrZV9hdWRpb19kYXRh",
            "consent_audio": "ZmFrZV9jb25zZW50X2RhdGE="
        }}}
    }));
    let setup = camelize_keys(setup_of(Some(config)).await);
    let replicated =
        &setup["setup"]["generationConfig"]["speechConfig"]["voiceConfig"]["replicatedVoiceConfig"];
    assert_eq!(replicated["mimeType"], "audio/wav");
    assert!(!replicated["voiceSampleAudio"].is_null());
    assert!(!replicated["consentAudio"].is_null());

    let config_with_sig = config_from(json!({
        "response_modalities": ["AUDIO"],
        "speech_config": { "voice_config": { "replicated_voice_config": {
            "mime_type": "audio/wav",
            "voice_sample_audio": "ZmFrZV9hdWRpb19kYXRh",
            "voice_consent_signature": { "signature": "test_sig_abc123" }
        }}}
    }));
    let setup = camelize_keys(setup_of(Some(config_with_sig)).await);
    let replicated =
        &setup["setup"]["generationConfig"]["speechConfig"]["voiceConfig"]["replicatedVoiceConfig"];
    assert_eq!(replicated["mimeType"], "audio/wav");
    assert!(!replicated["voiceSampleAudio"].is_null());
    assert_eq!(
        replicated["voiceConsentSignature"]["signature"],
        "test_sig_abc123"
    );
}
