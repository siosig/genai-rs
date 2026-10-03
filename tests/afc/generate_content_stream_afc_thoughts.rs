//! Ports of `google/genai/tests/afc/test_generate_content_stream_afc_thoughts.py`.
//!
//! Upstream replays a recorded session against `gemini-2.5-flash` and only
//! asserts that every chunk is non-empty. Here a mock server plays a
//! comparable session: the first round streams a thought summary and a
//! function call, the second streams the answer, and the stream is checked
//! to yield the chunks of both rounds.

use futures_util::StreamExt;
use gemini_genai::types::{GenerateContentConfig, ThinkingConfig};
use serde_json::json;
use wiremock::MockServer;

use super::{
    common::test_client,
    support::{
        config_with_tool, counting_tool, function_call_candidate, mount_next, request_count,
        sse_reply, text_candidate,
    },
};

async fn run(tool_name: &str) {
    let server = MockServer::start().await;
    let thought = json!({"content": {"role": "model", "parts": [{"text": "Looking up the weather.", "thought": true}]}});
    mount_next(
        &server,
        sse_reply(&[
            thought,
            function_call_candidate(tool_name, &json!({"location": "San Francisco, CA"})),
        ]),
    )
    .await;
    mount_next(&server, sse_reply(&[text_candidate("It is windy.")])).await;
    let (tool, calls) = counting_tool(tool_name, "windy");
    let config = GenerateContentConfig {
        thinking_config: Some(ThinkingConfig {
            include_thoughts: Some(true),
            ..Default::default()
        }),
        ..config_with_tool(tool, None)
    };

    let stream = test_client(server.uri())
        .models()
        .generate_content_stream(
            "gemini-2.5-flash",
            "what is the weather in San Francisco, CA?",
            Some(config),
        )
        .await
        .expect("the first request succeeds");
    let chunks: Vec<_> = stream.collect().await;

    assert_eq!(chunks.len(), 3, "two chunks of round one, one of round two");
    for chunk in &chunks {
        assert!(chunk.is_ok(), "{chunk:?}");
    }
    assert_eq!(request_count(&server).await, 2);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

// upstream-test: afc/test_generate_content_stream_afc_thoughts.py::test_generate_content_stream_with_function_and_thought_summaries
#[tokio::test]
async fn test_generate_content_stream_with_function_and_thought_summaries() {
    run("afc_thoughts_weather").await;
}

// upstream-test: afc/test_generate_content_stream_afc_thoughts.py::test_generate_content_stream_with_function_and_thought_summaries_async
#[tokio::test]
async fn test_generate_content_stream_with_function_and_thought_summaries_async() {
    run("afc_thoughts_weather_async").await;
}
