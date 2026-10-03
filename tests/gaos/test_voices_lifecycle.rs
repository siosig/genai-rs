//! Ported from `google/genai/tests/gaos/test_voices_lifecycle.py`.
//!
//! The Rust client is async-only, so the upstream sync and async variants both
//! drive the one async API; the upstream local recording HTTP server becomes a
//! wiremock server that answers with the same bodies.

use gemini_genai::voices::{Voice, VoiceCreateParams, VoiceListParams, VoiceListResponse};
use serde_json::{Value, json};

use crate::{
    common::test_client_with_api_key,
    recording::{captured, captured_bodies, recording_server},
};

fn voice_body() -> Value {
    json!({
        "id": "voice_abc123",
        "display_name": "Warm Narrator",
        "type": "prompted",
        "gender": "female",
        "language_code": "en-US",
        "prompted": {"input": "A warm, friendly narrator voice."},
    })
}

fn voice_list_body() -> Value {
    json!({
        "voices": [
            voice_body(),
            {
                "id": "Puck",
                "display_name": "Puck",
                "type": "prebuilt",
                "gender": "male",
                "language_code": "en-US",
            },
        ],
        "next_page_token": "token_next_123",
    })
}

/// The upstream handler: the list endpoint returns the list body, DELETE `{}`,
/// everything else the single voice.
fn voice_payload(method: &str, path_and_query: &str) -> Value {
    if method == "GET"
        && (path_and_query == "/v1beta/voices" || path_and_query.starts_with("/v1beta/voices?"))
    {
        voice_list_body()
    } else if method == "DELETE" {
        json!({})
    } else {
        voice_body()
    }
}

// upstream-test: gaos/test_voices_lifecycle.py::test_python_voices_lifecycle_routes_through_google_genai_client
#[tokio::test]
async fn test_python_voices_lifecycle_routes_through_google_genai_client() {
    let server = recording_server(voice_payload).await;
    let client = test_client_with_api_key(server.uri(), "test-api-key");
    let create_body = json!({
        "store": true,
        "voice": {
            "type": "prompted",
            "display_name": "Warm Narrator",
            "gender": "female",
            "language_code": "en-US",
            "prompted": {"input": "A warm, friendly narrator voice."},
        },
    });

    // 1. Create prompted voice
    let create_params: VoiceCreateParams = serde_json::from_value(create_body.clone()).unwrap();
    let created = client.voices().create(&create_params).await.unwrap();
    assert_eq!(created.id.as_deref(), Some("voice_abc123"));
    assert_eq!(created.display_name.as_deref(), Some("Warm Narrator"));

    // 2. List voices
    let list_res = client
        .voices()
        .list(&VoiceListParams {
            language_code: Some(vec!["en-US".to_owned()]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(list_res.voices.as_ref().map(Vec::len), Some(2));
    assert_eq!(list_res.next_page_token.as_deref(), Some("token_next_123"));

    // 3. Get voice
    let fetched = client.voices().get("voice_abc123").await.unwrap();
    assert_eq!(fetched.id.as_deref(), Some("voice_abc123"));

    // 4. Delete voice
    client.voices().delete("voice_abc123").await.unwrap();

    assert_eq!(
        captured(&server).await,
        [
            "POST /v1beta/voices",
            "GET /v1beta/voices?language_code=en-US",
            "GET /v1beta/voices/voice_abc123",
            "DELETE /v1beta/voices/voice_abc123",
        ]
    );
    assert_eq!(captured_bodies(&server).await[0], create_body);
}

// upstream-test: gaos/test_voices_lifecycle.py::test_python_voices_async_lifecycle
#[tokio::test]
async fn test_python_voices_async_lifecycle() {
    let server = recording_server(voice_payload).await;
    let client = test_client_with_api_key(server.uri(), "test-api-key");
    let create_params: VoiceCreateParams = serde_json::from_value(json!({
        "store": true,
        "voice": {
            "type": "prompted",
            "display_name": "Warm Narrator",
            "prompted": {"input": "A warm, friendly narrator voice."},
        },
    }))
    .unwrap();

    let created = client.voices().create(&create_params).await.unwrap();
    let list_res = client.voices().list(&Default::default()).await.unwrap();
    let fetched = client.voices().get("voice_abc123").await.unwrap();
    client.voices().delete("voice_abc123").await.unwrap();

    assert_eq!(created.id.as_deref(), Some("voice_abc123"));
    assert_eq!(fetched.id.as_deref(), Some("voice_abc123"));
    assert_eq!(list_res.voices.as_ref().map(Vec::len), Some(2));
    assert_eq!(
        captured(&server).await,
        [
            "POST /v1beta/voices",
            "GET /v1beta/voices",
            "GET /v1beta/voices/voice_abc123",
            "DELETE /v1beta/voices/voice_abc123",
        ]
    );
}

// upstream-test: gaos/test_voices_lifecycle.py::test_python_voices_with_raw_response
#[tokio::test]
async fn test_python_voices_with_raw_response() {
    // Python's `with_raw_response.list().parse()` is the parsed list response: the
    // Rust client has no raw-response wrapper and returns the parsed value directly.
    let server = recording_server(voice_payload).await;
    let client = test_client_with_api_key(server.uri(), "test-api-key");

    let parsed = client.voices().list(&Default::default()).await.unwrap();

    let voices = parsed.voices.unwrap();
    assert_eq!(voices.len(), 2);
    assert_eq!(voices[0].id.as_deref(), Some("voice_abc123"));
}

// upstream-test: gaos/test_voices_lifecycle.py::test_python_voices_types_and_models
#[test]
fn test_python_voices_types_and_models() {
    let voice: Voice = serde_json::from_value(json!({
        "id": "voice_abc123",
        "display_name": "Warm Narrator",
        "type": "prompted",
        "prompted": {"input": "Warm voice"},
    }))
    .unwrap();
    assert_eq!(voice.id.as_deref(), Some("voice_abc123"));
    assert_eq!(voice.r#type.as_ref().map(|t| t.as_str()), Some("prompted"));
    assert_eq!(
        voice.prompted.as_ref().and_then(|p| p.input.as_deref()),
        Some("Warm voice")
    );

    let list_resp = VoiceListResponse {
        voices: Some(vec![voice]),
        next_page_token: Some("next_tok".to_owned()),
    };
    assert_eq!(list_resp.voices.as_ref().map(Vec::len), Some(1));
    assert_eq!(list_resp.next_page_token.as_deref(), Some("next_tok"));
}
