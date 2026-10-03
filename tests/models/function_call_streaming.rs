//! Ports of `models/test_function_call_streaming.py`. The Developer API does
//! not support `stream_function_call_arguments`; every non-Vertex upstream
//! test therefore asserts that the request is rejected client-side.

use futures_util::StreamExt;
use gemini_genai::{
    Error,
    types::{
        Content, FunctionCall, FunctionCallingConfig, FunctionDeclaration, GenerateContentConfig,
        Part, PartialArg, Tool, ToolConfig,
    },
};
use wiremock::MockServer;

use super::common::test_client;
use super::support::{TestResult, mount_json, received};

const MODEL: &str = "gemini-3-pro-preview";
const PROMPT: &str = "get the current weather in boston in celsius, the country should be US, \
                      the purpose is to know what to wear today?";

fn weather_config() -> GenerateContentConfig {
    let json_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "location": {"type": "string", "description": "The location to get the weather for"},
            "country": {"anyOf": [{"type": "string"}, {"type": "null"}]},
            "unit": {"type": "string", "enum": ["C", "F"]},
            "purpose": {"type": "string"}
        },
        "required": ["location", "unit", "country"]
    });
    GenerateContentConfig {
        tools: Some(vec![Tool {
            function_declarations: Some(vec![FunctionDeclaration {
                name: Some("get_current_weather".to_owned()),
                description: Some("Get the current weather in a city".to_owned()),
                parameters_json_schema: Some(json_schema),
                ..Default::default()
            }]),
            ..Default::default()
        }]),
        tool_config: Some(ToolConfig {
            function_calling_config: Some(FunctionCallingConfig {
                stream_function_call_arguments: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn assert_vertex_only<T: std::fmt::Debug>(result: &Result<T, Error>) {
    assert!(
        matches!(
            result,
            Err(Error::UnsupportedByBackend {
                field: "stream_function_call_arguments",
                ..
            })
        ),
        "expected UnsupportedByBackend(stream_function_call_arguments), got {result:?}"
    );
}

/// The seed history carries `will_continue`, another Vertex-only field that
/// the converter may reject before it reaches `stream_function_call_arguments`;
/// upstream only requires a `ValueError`.
fn assert_any_vertex_only<T: std::fmt::Debug>(result: &Result<T, Error>) {
    assert!(
        matches!(result, Err(Error::UnsupportedByBackend { .. })),
        "expected UnsupportedByBackend, got {result:?}"
    );
}

fn response_part() -> Part {
    Part::from_function_response(
        "get_current_weather",
        [
            ("temperature".to_owned(), serde_json::json!(21)),
            ("unit".to_owned(), serde_json::json!("C")),
        ]
        .into(),
    )
}

/// The upstream seed history: a streamed function call split in two turns.
fn previous_history() -> Vec<Content> {
    let call = |will_continue, partial_args| Content {
        role: Some("model".to_owned()),
        parts: Some(vec![Part {
            function_call: Some(FunctionCall {
                name: Some("get_current_weather".to_owned()),
                will_continue: Some(will_continue),
                partial_args,
                ..Default::default()
            }),
            ..Default::default()
        }]),
    };
    vec![
        Content::from(PROMPT),
        call(true, None),
        call(
            false,
            Some(vec![PartialArg {
                json_path: Some("$.country".to_owned()),
                null_value: Some("NULL_VALUE".to_owned()),
                ..Default::default()
            }]),
        ),
    ]
}

async fn rejected_without_request(server: &MockServer) -> TestResult {
    assert!(
        received(server).await?.is_empty(),
        "the request must be rejected before anything is sent"
    );
    Ok(())
}

// upstream-test: models/test_function_call_streaming.py::test_streaming_with_json_parameters_without_history
#[tokio::test]
async fn test_streaming_with_json_parameters_without_history() -> TestResult {
    let server = MockServer::start().await;
    let client = test_client(server.uri());
    let result = client
        .models()
        .generate_content_stream(MODEL, PROMPT, Some(weather_config()))
        .await;
    assert_vertex_only(&result.map(|_| ()));
    rejected_without_request(&server).await
}

// upstream-test: models/test_function_call_streaming.py::test_streaming_with_json_parameters_async
#[tokio::test]
async fn test_streaming_with_json_parameters_async() -> TestResult {
    // The Rust client is async-only; same rejection, drained as a stream.
    let server = MockServer::start().await;
    let client = test_client(server.uri());
    let result = client
        .models()
        .generate_content_stream(MODEL, PROMPT, Some(weather_config()))
        .await;
    match result {
        Err(err) => assert_vertex_only::<()>(&Err(err)),
        Ok(mut stream) => {
            let first = stream.next().await;
            return Err(format!("stream must not start, got first item {first:?}").into());
        }
    }
    rejected_without_request(&server).await
}

// upstream-test: models/test_function_call_streaming.py::test_streaming_with_gemini_parameters_without_history
#[tokio::test]
async fn test_streaming_with_gemini_parameters_without_history() -> TestResult {
    let server = MockServer::start().await;
    let client = test_client(server.uri());
    // Schema-typed `parameters` instead of `parameters_json_schema`.
    let mut config = weather_config();
    if let Some(decl) = config
        .tools
        .as_mut()
        .and_then(|t| t.first_mut())
        .and_then(|t| t.function_declarations.as_mut())
        .and_then(|d| d.first_mut())
    {
        decl.parameters_json_schema = None;
        decl.parameters = Some(serde_json::from_value(serde_json::json!({
            "type": "OBJECT",
            "properties": {"location": {"type": "STRING"}},
            "required": ["location"]
        }))?);
    }
    let result = client
        .models()
        .generate_content_stream(MODEL, PROMPT, Some(config))
        .await;
    assert_vertex_only(&result.map(|_| ()));
    rejected_without_request(&server).await
}

// upstream-test: models/test_function_call_streaming.py::test_streaming_with_gemini_parameters_with_response
#[tokio::test]
async fn test_streaming_with_gemini_parameters_with_response() -> TestResult {
    let server = MockServer::start().await;
    let client = test_client(server.uri());
    // upstream raises on the very first streaming call, so the follow-up
    // request carrying the function response is never reached.
    let first = client
        .models()
        .generate_content_stream(MODEL, PROMPT, Some(weather_config()))
        .await;
    assert_vertex_only(&first.map(|_| ()));
    let follow_up = client
        .models()
        .generate_content_stream(
            MODEL,
            vec![Content::from(PROMPT), Content::from(response_part())],
            Some(weather_config()),
        )
        .await;
    assert_vertex_only(&follow_up.map(|_| ()));
    rejected_without_request(&server).await
}

// upstream-test: models/test_function_call_streaming.py::test_chat_streaming_with_json_parameters_with_history
#[tokio::test]
async fn test_chat_streaming_with_json_parameters_with_history() -> TestResult {
    let server = MockServer::start().await;
    mount_json(&server, "POST", 200, &serde_json::json!({})).await;
    let client = test_client(server.uri());
    let mut chat = client
        .chats()
        .create(MODEL, Some(weather_config()), Some(previous_history()));
    // upstream: the first send_message_stream raises ValueError
    let result = chat.send_message_stream(PROMPT, None).await;
    assert_any_vertex_only(&result.map(|_| ()));
    rejected_without_request(&server).await
}

// upstream-test: models/test_function_call_streaming.py::test_chat_streaming_with_json_parameters_with_history_async
#[tokio::test]
async fn test_chat_streaming_with_json_parameters_with_history_async() -> TestResult {
    let server = MockServer::start().await;
    let client = test_client(server.uri());
    let mut chat = client
        .chats()
        .create(MODEL, Some(weather_config()), Some(previous_history()));
    let result = chat
        .send_message_stream(Content::from(response_part()), None)
        .await;
    assert_any_vertex_only(&result.map(|_| ()));
    rejected_without_request(&server).await
}
