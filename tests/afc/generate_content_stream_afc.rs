//! Ports of `google/genai/tests/afc/test_generate_content_stream_afc.py`.
//!
//! Python's tests mock `Models._generate_content_stream` and
//! `_extra_utils.get_function_response_parts` and count calls to them. Here a
//! mock HTTP server answers the streaming requests (one SSE body per request,
//! in order) and the registered tool answers `sunny`; "the helper was called
//! twice" becomes "two requests were sent and the function ran". Python's
//! synchronous and asynchronous variants both map to the one async Rust API,
//! so each variant is its own test over the same scenario.

use std::sync::atomic::Ordering;

use futures_util::StreamExt;
use gemini_genai::types::{
    AutomaticFunctionCallingConfig, GenerateContentConfig, GenerateContentResponse, ThinkingConfig,
};
use serde_json::{Value, json};
use wiremock::MockServer;

use super::{
    common::test_client,
    support::{
        config_with_tool, counting_tool, function_call_candidate, mount_always, mount_next,
        request_bodies, sse_reply, text_candidate,
    },
};

const QUESTION: &str = "what is the weather in San Francisco?";
const NO_AFC_TEXT: &str = "Okay, here is the weather in San Francisco as of approximately  8:10 pm PST on May 10, 2023. Please note that the weather can change rapidly.";
const AFC_TEXT: &str = "San Francisco weather is sunny.";

fn weather_call(name: &str) -> Value {
    function_call_candidate(name, &json!({"location": "San Francisco"}))
}

async fn stream_chunks(
    server: &MockServer,
    config: Option<GenerateContentConfig>,
) -> Vec<GenerateContentResponse> {
    let stream = test_client(server.uri())
        .models()
        .generate_content_stream("test_model", QUESTION, config)
        .await
        .expect("the first request succeeds");
    stream
        .map(|chunk| chunk.expect("every chunk is a response"))
        .collect()
        .await
}

fn first_part_is_function_call(chunk: &GenerateContentResponse) -> bool {
    !chunk.function_calls().is_empty()
}

/// No function tools: one request, and the answer comes back as is.
async fn no_function_map() {
    let server = MockServer::start().await;
    mount_next(&server, sse_reply(&[text_candidate(NO_AFC_TEXT)])).await;

    let chunks = stream_chunks(&server, None).await;

    assert_eq!(chunks.len(), 1);
    for chunk in &chunks {
        assert_eq!(chunk.text().as_deref(), Some(NO_AFC_TEXT));
    }
    assert_eq!(request_bodies(&server).await.len(), 1);
}

/// Function tools but AFC disabled: works as manual function calling.
async fn afc_disabled(tool_name: &str) {
    let server = MockServer::start().await;
    mount_always(&server, sse_reply(&[weather_call(tool_name)])).await;
    let (tool, calls) = counting_tool(tool_name, "windy");
    let mut config = config_with_tool(tool, None);
    config.automatic_function_calling = Some(AutomaticFunctionCallingConfig {
        disable: Some(true),
        ..Default::default()
    });

    let chunks = stream_chunks(&server, Some(config)).await;

    for chunk in &chunks {
        // Work as manual function calling.
        assert!(first_part_is_function_call(chunk));
    }
    assert_eq!(request_bodies(&server).await.len(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

/// Function tools, but the model never calls one: one request, plain answer.
async fn no_function_response(tool_name: &str) {
    let server = MockServer::start().await;
    mount_always(&server, sse_reply(&[text_candidate(NO_AFC_TEXT)])).await;
    let (tool, calls) = counting_tool(tool_name, "unknown");

    let chunks = stream_chunks(&server, Some(config_with_tool(tool, None))).await;

    assert_eq!(chunks.len(), 1);
    for chunk in &chunks {
        assert_eq!(chunk.text().as_deref(), Some(NO_AFC_TEXT));
    }
    assert_eq!(request_bodies(&server).await.len(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

/// Function tools and the model calls one: the answer is built from the
/// function's response, and the last chunk reports the history.
async fn function_tools_used(tool_name: &str, thought_summaries: bool) {
    let server = MockServer::start().await;
    mount_next(&server, sse_reply(&[weather_call(tool_name)])).await;
    mount_next(&server, sse_reply(&[text_candidate(AFC_TEXT)])).await;
    let (tool, calls) = counting_tool(tool_name, "sunny");
    let mut config = config_with_tool(tool, None);
    if thought_summaries {
        config.thinking_config = Some(ThinkingConfig {
            include_thoughts: Some(true),
            ..Default::default()
        });
    }

    let chunks = stream_chunks(&server, Some(config)).await;

    assert!(
        chunks
            .iter()
            .any(|chunk| chunk.text().as_deref() == Some(AFC_TEXT))
    );
    let bodies = request_bodies(&server).await;
    assert_eq!(bodies.len(), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    if thought_summaries {
        for body in &bodies {
            // The nested key keeps its snake_case spelling on the wire, as in
            // Python (the converter only renames one level).
            let thinking = &body["generationConfig"]["thinkingConfig"];
            assert_eq!(
                thinking["include_thoughts"]
                    .as_bool()
                    .or(thinking["includeThoughts"].as_bool()),
                Some(true),
                "thinkingConfig: {thinking}"
            );
        }
    }

    let last = chunks.last().expect("at least one chunk");
    let history = last
        .automatic_function_calling_history
        .as_ref()
        .expect("the last chunk carries the history");
    let expected = json!([
        {"role": "user", "parts": [{"text": QUESTION}]},
        {"role": "model", "parts": [{"functionCall": {"name": tool_name, "args": {"location": "San Francisco"}}}]},
        {"role": "user", "parts": [{"functionResponse": {"name": tool_name, "response": {"result": "sunny"}}}]},
    ]);
    assert_eq!(history.len(), 3);
    for (index, content) in history.iter().enumerate() {
        let actual = serde_json::to_value(content).expect("content serializes");
        // Compare on the wire field names; `exclude_none` upstream.
        assert_eq!(wire_camel(&actual), expected[index], "history[{index}]");
    }
}

/// `serde_json` of a typed value uses `snake_case` field names; the expected
/// values above use the wire's `camelCase`, so the few keys involved are mapped.
fn wire_camel(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let key = match key.as_str() {
                        "function_call" => "functionCall",
                        "function_response" => "functionResponse",
                        other => other,
                    };
                    (key.to_owned(), wire_camel(value))
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(wire_camel).collect()),
        other => other.clone(),
    }
}

// upstream-test: afc/test_generate_content_stream_afc.py::test_generate_content_stream_no_function_map
#[tokio::test]
async fn test_generate_content_stream_no_function_map() {
    no_function_map().await;
}

// upstream-test: afc/test_generate_content_stream_afc.py::test_generate_content_stream_afc_disabled
#[tokio::test]
async fn test_generate_content_stream_afc_disabled() {
    afc_disabled("afc_stream_afc_disabled_weather").await;
}

// upstream-test: afc/test_generate_content_stream_afc.py::test_generate_content_stream_no_function_response
#[tokio::test]
async fn test_generate_content_stream_no_function_response() {
    no_function_response("afc_stream_no_function_response_aqi").await;
}

// upstream-test: afc/test_generate_content_stream_afc.py::test_generate_content_stream_with_function_tools_used
#[tokio::test]
async fn test_generate_content_stream_with_function_tools_used() {
    function_tools_used("afc_stream_with_function_tools_used_weather", false).await;
}

// upstream-test: afc/test_generate_content_stream_afc.py::test_generate_content_stream_with_thought_summaries
#[tokio::test]
async fn test_generate_content_stream_with_thought_summaries() {
    function_tools_used("afc_stream_with_thought_summaries_weather", true).await;
}

// upstream-test: afc/test_generate_content_stream_afc.py::test_generate_content_stream_no_function_map_async
#[tokio::test]
async fn test_generate_content_stream_no_function_map_async() {
    no_function_map().await;
}

// upstream-test: afc/test_generate_content_stream_afc.py::test_generate_content_stream_afc_disabled_async
#[tokio::test]
async fn test_generate_content_stream_afc_disabled_async() {
    afc_disabled("afc_stream_afc_disabled_async_weather").await;
}

// upstream-test: afc/test_generate_content_stream_afc.py::test_generate_content_stream_no_function_response_async
#[tokio::test]
async fn test_generate_content_stream_no_function_response_async() {
    no_function_response("afc_stream_no_function_response_async_aqi").await;
}

// upstream-test: afc/test_generate_content_stream_afc.py::test_generate_content_stream_with_function_tools_used_async
#[tokio::test]
async fn test_generate_content_stream_with_function_tools_used_async() {
    function_tools_used("afc_stream_with_function_tools_used_async_weather", false).await;
}

// upstream-test: afc/test_generate_content_stream_afc.py::test_generate_content_stream_with_function_async_function_used_async
#[tokio::test]
async fn test_generate_content_stream_with_function_async_function_used_async() {
    function_tools_used(
        "afc_stream_with_function_async_function_used_async_weather",
        false,
    )
    .await;
}

// upstream-test: afc/test_generate_content_stream_afc.py::test_generate_content_stream_with_thought_summaries_async
#[tokio::test]
async fn test_generate_content_stream_with_thought_summaries_async() {
    function_tools_used("afc_stream_with_thought_summaries_async_weather", true).await;
}
