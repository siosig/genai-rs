//! Ports of `google/genai/tests/chats/test_send_message.py`.
//!
//! Upstream runs these against recorded API replays; here a `wiremock`
//! server plays the model and the assertions check the requests the chat
//! sent as well as the history it kept. Rust has a single (async) `Chat`, so
//! the upstream `test_async_*` twins share their body with the sync test and
//! differ only in name.
//!
//! The AFC tool registry is keyed by function name and shared by every test
//! in this binary, so each test registers its tools under a name of its own.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use futures_util::StreamExt;
use gemini_genai::{
    Error,
    chats::Chat,
    function_tool,
    types::{
        AutomaticFunctionCallingConfig, Content, FunctionCallingConfig, FunctionCallingConfigMode,
        FunctionDeclaration, GenerateContentConfig, GenerateContentResponse, McpServer, Part,
        StreamableHttpTransport, ThinkingConfig, Tool, ToolConfig,
    },
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use wiremock::{MockServer, ResponseTemplate};

use super::{
    function_call_reply, model_reply, mount_always, mount_next, request_bodies, sse_response,
    test_client, text_chunk,
};

const MODEL: &str = "gemini-2.5-flash";

fn chat_on(server: &MockServer, config: Option<GenerateContentConfig>) -> Chat {
    test_client(server.uri())
        .chats()
        .create(MODEL, config, None)
}

/// Sends `message` as a stream and collects every chunk.
#[expect(
    clippy::unwrap_used,
    reason = "test helper: a stream error here is a test failure"
)]
async fn stream_chunks(
    chat: &mut Chat,
    message: impl Into<gemini_genai::types::Contents>,
    config: Option<GenerateContentConfig>,
) -> Vec<GenerateContentResponse> {
    let mut stream = chat.send_message_stream(message, config).await.unwrap();
    let mut chunks = Vec::new();
    while let Some(chunk) = stream.next().await {
        chunks.push(chunk.unwrap());
    }
    chunks
}

/// The value under `camel` or, failing that, `snake`. Nested config objects
/// are forwarded by the converters with their serde (`snake_case`) keys, which
/// the API accepts as well as camelCase, exactly as Python's converters do.
fn field<'a>(value: &'a Value, camel: &str, snake: &str) -> &'a Value {
    if value.get(camel).is_some() {
        &value[camel]
    } else {
        &value[snake]
    }
}

fn request_text(body: &Value) -> Option<&str> {
    body["contents"].as_array()?.last()?["parts"]
        .as_array()?
        .first()?["text"]
        .as_str()
}

fn text_of(content: &Content, index: usize) -> Option<&str> {
    content.parts.as_ref()?.get(index)?.text.as_deref()
}

fn thinking_config() -> GenerateContentConfig {
    GenerateContentConfig {
        thinking_config: Some(ThinkingConfig {
            include_thoughts: Some(true),
            thinking_budget: Some(10000),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn thought_reply() -> Value {
    json!({"candidates": [{
        "content": {"role": "model", "parts": [
            {"text": "summing 1..100", "thought": true},
            {"text": "5050"}
        ]},
        "finishReason": "STOP"
    }]})
}

fn has_thought(response: &GenerateContentResponse) -> bool {
    response
        .candidates
        .iter()
        .flatten()
        .filter_map(|candidate| candidate.content.as_ref())
        .flat_map(|content| content.parts.iter().flatten())
        .any(|part| part.thought == Some(true))
}

fn assert_thinking_requested(body: &Value) {
    let thinking = &body["generationConfig"]["thinkingConfig"];
    assert_eq!(field(thinking, "includeThoughts", "include_thoughts"), true);
    assert_eq!(field(thinking, "thinkingBudget", "thinking_budget"), 10000);
}

// ---- plain text messages ----------------------------------------------

async fn text_message_case(message: impl Into<gemini_genai::types::Contents>) -> Value {
    let server = MockServer::start().await;
    mount_always(
        &server,
        ResponseTemplate::new(200).set_body_json(model_reply("a story")),
    )
    .await;
    let mut chat = chat_on(&server, None);

    let response = chat.send_message(message, None).await.unwrap();

    assert_eq!(response.text().as_deref(), Some("a story"));
    assert_eq!(chat.get_history(false).len(), 2);
    request_bodies(&server).await.remove(0)
}

// upstream-test: chats/test_send_message.py::test_text
#[tokio::test]
async fn test_text() {
    let body = text_message_case("tell me a story in 100 words").await;

    assert_eq!(request_text(&body), Some("tell me a story in 100 words"));
}

// upstream-test: chats/test_send_message.py::test_part
#[tokio::test]
async fn test_part() {
    let body = text_message_case(Part::from_text("tell me a story in 100 words")).await;

    assert_eq!(request_text(&body), Some("tell me a story in 100 words"));
}

// upstream-test: chats/test_send_message.py::test_parts
#[tokio::test]
async fn test_parts() {
    let body = text_message_case(vec![
        Part::from_text("tell me a US city"),
        Part::from_text("the city is in west coast"),
    ])
    .await;

    let contents = body["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 1, "one user turn");
    assert_eq!(
        contents[0]["parts"],
        json!([{"text": "tell me a US city"}, {"text": "the city is in west coast"}])
    );
}

// upstream-test: chats/test_send_message.py::test_image
#[tokio::test]
async fn test_image() {
    // Upstream passes a PIL image; the Rust equivalent is inline bytes.
    let body = text_message_case(vec![
        Part::from_text("what is the image about?"),
        Part::from_bytes(vec![0xFF, 0xD8, 0xFF, 0xE0], "image/jpeg"),
    ])
    .await;

    let parts = &body["contents"][0]["parts"];
    assert_eq!(parts[0]["text"], "what is the image about?");
    let inline = field(&parts[1], "inlineData", "inline_data");
    assert_eq!(field(inline, "mimeType", "mime_type"), "image/jpeg");
    assert_eq!(inline["data"], "_9j_4A==");
}

// ---- thinking ---------------------------------------------------------

// upstream-test: chats/test_send_message.py::test_thinking_budget
#[tokio::test]
async fn test_thinking_budget() {
    let server = MockServer::start().await;
    mount_always(
        &server,
        ResponseTemplate::new(200).set_body_json(thought_reply()),
    )
    .await;
    let mut chat = chat_on(&server, Some(thinking_config()));

    let response1 = chat
        .send_message("what is the sum of natural numbers from 1 to 100?", None)
        .await
        .unwrap();
    assert!(has_thought(&response1));
    let response2 = chat
        .send_message("can you help me to understand the logic better?", None)
        .await
        .unwrap();
    assert!(has_thought(&response2));

    // The chat-level config rides along on every request.
    for body in request_bodies(&server).await {
        assert_thinking_requested(&body);
    }
}

// upstream-test: chats/test_send_message.py::test_thinking_budget_stream
#[tokio::test]
async fn test_thinking_budget_stream() {
    let server = MockServer::start().await;
    mount_always(&server, sse_response(&[thought_reply()])).await;
    let mut chat = chat_on(&server, Some(thinking_config()));

    let chunks1 = stream_chunks(
        &mut chat,
        "what is the sum of natural numbers from 1 to 100?",
        None,
    )
    .await;
    assert!(chunks1.iter().any(has_thought));
    let chunks2 = stream_chunks(
        &mut chat,
        "can you help me to understand the logic better?",
        None,
    )
    .await;
    assert!(chunks2.iter().any(has_thought));

    for body in request_bodies(&server).await {
        assert_thinking_requested(&body);
    }
}

// ---- file URIs --------------------------------------------------------

// upstream-test: chats/test_send_message.py::test_google_cloud_storage_uri
#[tokio::test]
async fn test_google_cloud_storage_uri() {
    // The Gemini Developer API rejects `gs://` URIs; upstream expects
    // `ClientError` there (`exception_if_mldev`).
    let server = MockServer::start().await;
    mount_always(
        &server,
        ResponseTemplate::new(400).set_body_json(json!({"error": {
            "code": 400,
            "message": "Cannot fetch content from the provided URL.",
            "status": "INVALID_ARGUMENT"
        }})),
    )
    .await;
    let mut chat = chat_on(&server, None);

    let error = chat
        .send_message(
            vec![
                Part::from_text("what is the image about?"),
                Part::from_uri(
                    "gs://unified-genai-dev/imagen-inputs/google_small.png",
                    "image/png",
                ),
            ],
            None,
        )
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Api(_)), "got {error:?}");
    let bodies = request_bodies(&server).await;
    let file = field(
        &bodies[0]["contents"][0]["parts"][1],
        "fileData",
        "file_data",
    );
    assert_eq!(
        field(file, "fileUri", "file_uri"),
        "gs://unified-genai-dev/imagen-inputs/google_small.png"
    );
}

// upstream-test: chats/test_send_message.py::test_uploaded_file_uri
#[tokio::test]
async fn test_uploaded_file_uri() {
    // Developer-API file URIs are accepted (upstream expects `ClientError`
    // only on Vertex, `exception_if_vertex`).
    let server = MockServer::start().await;
    mount_always(
        &server,
        ResponseTemplate::new(200).set_body_json(model_reply("a logo")),
    )
    .await;
    let mut chat = chat_on(&server, None);
    let uri = "https://generativelanguage.googleapis.com/v1beta/files/az606f58k7zj";

    chat.send_message(
        vec![
            Part::from_text("what is the image about?"),
            Part::from_uri(uri, "image/png"),
        ],
        None,
    )
    .await
    .unwrap();

    let bodies = request_bodies(&server).await;
    let file = field(
        &bodies[0]["contents"][0]["parts"][1],
        "fileData",
        "file_data",
    );
    assert_eq!(field(file, "fileUri", "file_uri"), uri);
    assert_eq!(field(file, "mimeType", "mime_type"), "image/png");
}

// ---- config override ---------------------------------------------------

fn candidates_reply(count: usize) -> Value {
    let candidate = json!({
        "content": {"role": "model", "parts": [{"text": "story"}]},
        "finishReason": "STOP"
    });
    json!({"candidates": vec![candidate; count]})
}

async fn config_override_case() {
    let server = MockServer::start().await;
    mount_next(
        &server,
        ResponseTemplate::new(200).set_body_json(candidates_reply(2)),
    )
    .await;
    mount_next(
        &server,
        ResponseTemplate::new(200).set_body_json(candidates_reply(1)),
    )
    .await;
    let mut chat = chat_on(
        &server,
        Some(GenerateContentConfig {
            candidate_count: Some(1),
            ..Default::default()
        }),
    );

    let request_config_response = chat
        .send_message(
            "tell me a story in 100 words",
            Some(GenerateContentConfig {
                candidate_count: Some(2),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let default_config_response = chat
        .send_message("tell me a story in 100 words", None)
        .await
        .unwrap();

    assert_eq!(
        request_config_response.candidates.as_ref().map(Vec::len),
        Some(2)
    );
    assert_eq!(
        default_config_response.candidates.as_ref().map(Vec::len),
        Some(1)
    );
    // The per-request config replaces the chat's for that request only.
    let bodies = request_bodies(&server).await;
    assert_eq!(bodies[0]["generationConfig"]["candidateCount"], 2);
    assert_eq!(bodies[1]["generationConfig"]["candidateCount"], 1);
}

// upstream-test: chats/test_send_message.py::test_config_override
#[tokio::test]
async fn test_config_override() {
    config_override_case().await;
}

// ---- history -----------------------------------------------------------

async fn seeded_history_case() {
    let server = MockServer::start().await;
    mount_always(
        &server,
        ResponseTemplate::new(200).set_body_json(model_reply("15")),
    )
    .await;
    let history = vec![
        Content {
            role: Some("user".to_owned()),
            parts: Some(vec![Part::from_text("define a=5, b=10")]),
        },
        Content {
            role: Some("model".to_owned()),
            parts: Some(vec![Part::from_text("Hello there! how can I help you?")]),
        },
    ];
    let mut chat = test_client(server.uri())
        .chats()
        .create(MODEL, None, Some(history));

    chat.send_message("what is a + b?", None).await.unwrap();

    assert!(chat.get_history(false).len() > 2);
    let bodies = request_bodies(&server).await;
    assert_eq!(bodies[0]["contents"].as_array().map(Vec::len), Some(3));
}

// upstream-test: chats/test_send_message.py::test_history
#[tokio::test]
async fn test_history() {
    seeded_history_case().await;
}

// upstream-test: chats/test_send_message.py::test_send_2_messages
#[tokio::test]
async fn test_send_2_messages() {
    let server = MockServer::start().await;
    mount_always(
        &server,
        ResponseTemplate::new(200).set_body_json(model_reply("ok")),
    )
    .await;
    let mut chat = chat_on(&server, None);

    chat.send_message(
        "write a python function to check if a year is a leap year",
        None,
    )
    .await
    .unwrap();
    chat.send_message("write a unit test for the function", None)
        .await
        .unwrap();

    assert_eq!(chat.get_history(false).len(), 4);
    let bodies = request_bodies(&server).await;
    assert_eq!(bodies[1]["contents"].as_array().map(Vec::len), Some(3));
}

// ---- automatic function calling ----------------------------------------

/// Arguments of the upstream `divide_intergers_with_customized_math_rule`.
#[derive(Deserialize, JsonSchema)]
struct DivideArgs {
    numerator: i64,
    denominator: i64,
}

/// Arguments of the upstream `square_integer`.
#[derive(Deserialize, JsonSchema)]
struct SquareArgs {
    given_integer: i64,
}

fn divide_tool(name: &str) -> Tool {
    Tool::from_function(function_tool::<DivideArgs, _, _, _>(
        name,
        "Divides two integers with customized math rule.",
        |args: DivideArgs| async move { Ok(args.numerator / args.denominator + 1) },
    ))
}

fn tool_config(tool: Tool) -> GenerateContentConfig {
    GenerateContentConfig {
        tools: Some(vec![tool]),
        ..Default::default()
    }
}

fn function_call_of(content: &Content) -> &gemini_genai::types::FunctionCall {
    content.parts.as_ref().unwrap()[0]
        .function_call
        .as_ref()
        .unwrap()
}

/// Asserts the four turns Python records for one AFC round trip.
fn assert_afc_exchange(history: &[Content], name: &str, question: &str) {
    assert_eq!(history.len(), 4);
    assert_eq!(history[0].role.as_deref(), Some("user"));
    assert_eq!(text_of(&history[0], 0), Some(question));

    assert_eq!(history[1].role.as_deref(), Some("model"));
    let call = function_call_of(&history[1]);
    assert_eq!(call.name.as_deref(), Some(name));
    assert_eq!(
        call.args.as_ref().map(|args| json!(args)),
        Some(json!({"numerator": 100, "denominator": 2}))
    );

    assert_eq!(history[2].role.as_deref(), Some("user"));
    let response = history[2].parts.as_ref().unwrap()[0]
        .function_response
        .as_ref()
        .unwrap();
    assert_eq!(response.name.as_deref(), Some(name));
    assert_eq!(
        response.response.as_ref().map(|r| json!(r)),
        Some(json!({"result": 51}))
    );

    assert_eq!(history[3].role.as_deref(), Some("model"));
    assert!(text_of(&history[3], 0).is_some_and(|text| text.contains("51")));
}

async fn afc_history_case(name: &str) {
    let server = MockServer::start().await;
    mount_next(
        &server,
        ResponseTemplate::new(200).set_body_json(function_call_reply(
            name,
            &json!({"numerator": 100, "denominator": 2}),
        )),
    )
    .await;
    mount_next(
        &server,
        ResponseTemplate::new(200).set_body_json(model_reply("100 divided by 2 is 51")),
    )
    .await;
    let mut chat = chat_on(&server, Some(tool_config(divide_tool(name))));

    chat.send_message("what is the result of 100/2?", None)
        .await
        .unwrap();

    assert_afc_exchange(
        chat.get_history(false),
        name,
        "what is the result of 100/2?",
    );
    assert_eq!(request_bodies(&server).await.len(), 2);
}

// upstream-test: chats/test_send_message.py::test_with_afc_history
#[tokio::test]
async fn test_with_afc_history() {
    afc_history_case("chats_with_afc_history").await;
}

// upstream-test: chats/test_send_message.py::test_with_afc_history_async
#[tokio::test]
async fn test_with_afc_history_async() {
    afc_history_case("chats_with_afc_history_async").await;
}

// upstream-test: chats/test_send_message.py::test_existing_chat_history_extends_afc_history
#[tokio::test]
async fn test_existing_chat_history_extends_afc_history() {
    let name = "chats_existing_history_divide";
    let server = MockServer::start().await;
    mount_next(
        &server,
        ResponseTemplate::new(200).set_body_json(model_reply("Hi!")),
    )
    .await;
    mount_next(
        &server,
        ResponseTemplate::new(200).set_body_json(model_reply("Sure, what is the problem?")),
    )
    .await;
    mount_next(
        &server,
        ResponseTemplate::new(200).set_body_json(function_call_reply(
            name,
            &json!({"numerator": 100, "denominator": 2}),
        )),
    )
    .await;
    mount_next(
        &server,
        ResponseTemplate::new(200).set_body_json(model_reply("It is 51.")),
    )
    .await;
    let mut chat = chat_on(&server, Some(tool_config(divide_tool(name))));

    chat.send_message("hello", None).await.unwrap();
    chat.send_message("could you help me with a math problem?", None)
        .await
        .unwrap();
    chat.send_message("what is the result of 100/2?", None)
        .await
        .unwrap();

    // The history is not duplicated by the AFC turns being recorded.
    let serialized: Vec<String> = chat
        .get_history(false)
        .iter()
        .map(|content| serde_json::to_string(content).unwrap())
        .collect();
    let mut unique = serialized.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(serialized.len(), 8);
    assert_eq!(
        unique.len(),
        serialized.len(),
        "duplicated turn in {serialized:?}"
    );
}

/// The three disco functions of the upstream house-party test.
#[derive(Deserialize, JsonSchema)]
#[expect(
    dead_code,
    reason = "only the derived JSON schema and deserialization use the fields"
)]
struct PowerArgs {
    power: bool,
}

#[derive(Deserialize, JsonSchema)]
#[expect(
    dead_code,
    reason = "only the derived JSON schema and deserialization use the fields"
)]
struct MusicArgs {
    energetic: bool,
    loud: bool,
    bpm: i64,
}

#[derive(Deserialize, JsonSchema)]
#[expect(
    dead_code,
    reason = "only the derived JSON schema and deserialization use the fields"
)]
struct DimArgs {
    brightness: f64,
}

fn counted<A, R>(name: &str, calls: &Arc<AtomicUsize>, result: R) -> Tool
where
    A: serde::de::DeserializeOwned + JsonSchema + Send + 'static,
    R: serde::Serialize + Clone + Send + Sync + 'static,
{
    let calls = Arc::clone(calls);
    Tool::from_function(function_tool::<A, _, _, _>(
        name,
        "A house function.",
        move |_args: A| {
            calls.fetch_add(1, Ordering::SeqCst);
            let result = result.clone();
            async move { Ok(result) }
        },
    ))
}

async fn multiple_remote_calls_case(suffix: &str) {
    let calls = Arc::new(AtomicUsize::new(0));
    let (power, music, dim) = (
        format!("power_disco_ball_{suffix}"),
        format!("start_music_{suffix}"),
        format!("dim_lights_{suffix}"),
    );
    let config = GenerateContentConfig {
        tools: Some(vec![
            counted::<PowerArgs, _>(&power, &calls, true),
            counted::<MusicArgs, _>(&music, &calls, "Never gonna give you up."),
            counted::<DimArgs, _>(&dim, &calls, true),
        ]),
        // Force the model to act (call 'any' function), instead of chatting.
        tool_config: Some(ToolConfig {
            function_calling_config: Some(FunctionCallingConfig {
                mode: Some(FunctionCallingConfigMode::Any),
                ..Default::default()
            }),
            ..Default::default()
        }),
        automatic_function_calling: Some(AutomaticFunctionCallingConfig {
            maximum_remote_calls: Some(3),
            ..Default::default()
        }),
        ..Default::default()
    };
    let server = MockServer::start().await;
    let three_calls = json!({"candidates": [{
        "content": {"role": "model", "parts": [
            {"functionCall": {"name": power, "args": {"power": true}}},
            {"functionCall": {"name": music, "args": {"energetic": true, "loud": true, "bpm": 120}}},
            {"functionCall": {"name": dim, "args": {"brightness": 0.5}}}
        ]},
        "finishReason": "STOP"
    }]});
    mount_always(
        &server,
        ResponseTemplate::new(200).set_body_json(three_calls),
    )
    .await;
    let mut chat = chat_on(&server, Some(config));

    chat.send_message("Turn this place into a party!", None)
        .await
        .unwrap();

    // A budget of 3 buys 3 requests. The third is spent being asked for
    // functions that no request is left to answer, so the turn ends on that
    // call and only the first two rounds' functions run.
    assert_eq!(request_bodies(&server).await.len(), 3);
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    let history = chat.get_history(true);
    assert_eq!(history.len(), 6);
    assert_eq!(history[0].role.as_deref(), Some("user"));
    assert_eq!(
        text_of(&history[0], 0),
        Some("Turn this place into a party!")
    );
    for (index, (role, is_call)) in [
        ("model", true),
        ("user", false),
        ("model", true),
        ("user", false),
        ("model", true),
    ]
    .into_iter()
    .enumerate()
    {
        let content = &history[index + 1];
        let parts = content.parts.as_ref().unwrap();
        assert_eq!(content.role.as_deref(), Some(role), "turn {}", index + 1);
        assert_eq!(parts.len(), 3, "turn {}", index + 1);
        for part in parts {
            let present = if is_call {
                part.function_call.is_some()
            } else {
                part.function_response.is_some()
            };
            assert!(present, "turn {} part {part:?}", index + 1);
        }
    }
}

// upstream-test: chats/test_send_message.py::test_with_afc_multiple_remote_calls
#[tokio::test]
async fn test_with_afc_multiple_remote_calls() {
    multiple_remote_calls_case("sync").await;
}

// upstream-test: chats/test_send_message.py::test_with_afc_multiple_remote_calls_async
#[tokio::test]
async fn test_with_afc_multiple_remote_calls_async() {
    multiple_remote_calls_case("async").await;
}

async fn afc_disabled_case(name: &str) {
    let invocations = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&invocations);
    let tool = Tool::from_function(function_tool::<SquareArgs, _, _, _>(
        name,
        "Squares an integer.",
        move |args: SquareArgs| {
            counter.fetch_add(1, Ordering::SeqCst);
            async move { Ok(args.given_integer * args.given_integer) }
        },
    ));
    let server = MockServer::start().await;
    mount_always(
        &server,
        ResponseTemplate::new(200)
            .set_body_json(function_call_reply(name, &json!({"given_integer": 3}))),
    )
    .await;
    let mut chat = chat_on(
        &server,
        Some(GenerateContentConfig {
            tools: Some(vec![tool]),
            automatic_function_calling: Some(AutomaticFunctionCallingConfig {
                disable: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        }),
    );

    chat.send_message("Do the square of 3.", None)
        .await
        .unwrap();

    let history = chat.get_history(false);
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].role.as_deref(), Some("user"));
    assert_eq!(text_of(&history[0], 0), Some("Do the square of 3."));
    assert_eq!(history[1].role.as_deref(), Some("model"));
    let call = function_call_of(&history[1]);
    assert_eq!(call.name.as_deref(), Some(name));
    assert_eq!(
        call.args.as_ref().map(|args| json!(args)),
        Some(json!({"given_integer": 3}))
    );
    assert_eq!(
        invocations.load(Ordering::SeqCst),
        0,
        "the tool must not run"
    );
    assert_eq!(request_bodies(&server).await.len(), 1);
}

// upstream-test: chats/test_send_message.py::test_with_afc_disabled
#[tokio::test]
async fn test_with_afc_disabled() {
    afc_disabled_case("chats_square_disabled").await;
}

// upstream-test: chats/test_send_message.py::test_with_afc_disabled_async
#[tokio::test]
async fn test_with_afc_disabled_async() {
    afc_disabled_case("chats_square_disabled_async").await;
}

// ---- streaming ---------------------------------------------------------

async fn stream_text_case(message: impl Into<gemini_genai::types::Contents>, chunk_count: usize) {
    let server = MockServer::start().await;
    let chunks: Vec<Value> = (0..chunk_count)
        .map(|index| text_chunk(&format!("part {index} "), index + 1 == chunk_count))
        .collect();
    mount_always(&server, sse_response(&chunks)).await;
    let mut chat = chat_on(&server, None);

    let received = stream_chunks(&mut chat, message, None).await;

    assert_eq!(received.len(), chunk_count);
    // Python records one model turn per streamed chunk.
    assert_eq!(chat.get_history(true).len(), 1 + chunk_count);
}

// upstream-test: chats/test_send_message.py::test_stream_text
#[tokio::test]
async fn test_stream_text() {
    stream_text_case("tell me a story in 100 words", 2).await;
}

// upstream-test: chats/test_send_message.py::test_stream_part
#[tokio::test]
async fn test_stream_part() {
    stream_text_case(Part::from_text("tell me a story in 100 words"), 2).await;
}

// upstream-test: chats/test_send_message.py::test_stream_parts
#[tokio::test]
async fn test_stream_parts() {
    stream_text_case(
        vec![
            Part::from_text("tell me a story in 100 words"),
            Part::from_text("the story is about a car"),
        ],
        3,
    )
    .await;
}

async fn stream_config_override_case() {
    let server = MockServer::start().await;
    mount_next(
        &server,
        sse_response(&[
            text_chunk("{\"story\": ", false),
            text_chunk("\"once\"}", true),
        ]),
    )
    .await;
    mount_next(
        &server,
        sse_response(&[text_chunk("Once upon ", false), text_chunk("a time.", true)]),
    )
    .await;
    let mut chat = chat_on(
        &server,
        Some(GenerateContentConfig {
            response_mime_type: Some("text/plain".to_owned()),
            ..Default::default()
        }),
    );

    let request_config_text: String = stream_chunks(
        &mut chat,
        "tell me a story in 100 words",
        Some(GenerateContentConfig {
            response_mime_type: Some("application/json".to_owned()),
            ..Default::default()
        }),
    )
    .await
    .iter()
    .filter_map(GenerateContentResponse::text)
    .collect();
    let default_config_text: String =
        stream_chunks(&mut chat, "tell me a story in 100 words", None)
            .await
            .iter()
            .filter_map(GenerateContentResponse::text)
            .collect();

    assert!(serde_json::from_str::<Value>(&request_config_text).is_ok());
    assert!(serde_json::from_str::<Value>(&default_config_text).is_err());
    let bodies = request_bodies(&server).await;
    assert_eq!(
        bodies[0]["generationConfig"]["responseMimeType"],
        "application/json"
    );
    assert_eq!(
        bodies[1]["generationConfig"]["responseMimeType"],
        "text/plain"
    );
}

// upstream-test: chats/test_send_message.py::test_stream_config_override
#[tokio::test]
async fn test_stream_config_override() {
    stream_config_override_case().await;
}

async fn stream_function_calling_case(name: &str) {
    let server = MockServer::start().await;
    let call = |numerator: i64| {
        sse_response(&[function_call_reply(
            name,
            &json!({"numerator": numerator, "denominator": 2}),
        )])
    };
    // Each message is two rounds: the model asks for the tool, then answers.
    mount_next(&server, call(100)).await;
    mount_next(
        &server,
        sse_response(&[text_chunk("The result is 51.", true)]),
    )
    .await;
    mount_next(&server, call(50)).await;
    mount_next(
        &server,
        sse_response(&[text_chunk("The result is 26.", true)]),
    )
    .await;
    let mut chat = chat_on(&server, Some(tool_config(divide_tool(name))));

    // Streaming runs AFC, like Python ("Now we support AFC").
    stream_chunks(&mut chat, "what is the result of 100/2?", None).await;
    stream_chunks(&mut chat, "what is the result of 50/2?", None).await;

    let history = chat.get_history(false);
    assert_eq!(history.len(), 8);
    assert_eq!(history[0].role.as_deref(), Some("user"));
    assert_eq!(
        text_of(&history[0], 0),
        Some("what is the result of 100/2?")
    );
    assert_eq!(history[1].role.as_deref(), Some("model"));
    let first_call = function_call_of(&history[1]);
    assert_eq!(first_call.name.as_deref(), Some(name));
    assert_eq!(
        first_call.args.as_ref().map(|args| json!(args)),
        Some(json!({"numerator": 100, "denominator": 2}))
    );
    let response = history[2].parts.as_ref().unwrap()[0]
        .function_response
        .as_ref()
        .unwrap();
    assert_eq!(
        response.response.as_ref().map(|r| json!(r)),
        Some(json!({"result": 51}))
    );
    assert_eq!(text_of(&history[3], 0), Some("The result is 51."));
    assert_eq!(request_bodies(&server).await.len(), 4);
}

// upstream-test: chats/test_send_message.py::test_stream_function_calling
#[tokio::test]
async fn test_stream_function_calling() {
    stream_function_calling_case("chats_stream_divide").await;
}

async fn stream_send_2_messages_case() {
    let server = MockServer::start().await;
    mount_always(&server, sse_response(&[text_chunk("ok", true)])).await;
    let mut chat = chat_on(&server, None);

    stream_chunks(
        &mut chat,
        "write a python function to check if a year is a leap year",
        None,
    )
    .await;
    stream_chunks(&mut chat, "write a unit test for the function", None).await;

    assert_eq!(chat.get_history(false).len(), 4);
    let bodies = request_bodies(&server).await;
    assert_eq!(bodies[1]["contents"].as_array().map(Vec::len), Some(3));
}

// upstream-test: chats/test_send_message.py::test_stream_send_2_messages
#[tokio::test]
async fn test_stream_send_2_messages() {
    stream_send_2_messages_case().await;
}

// ---- upstream `test_async_*` twins ---------------------------------------

// upstream-test: chats/test_send_message.py::test_async_text
#[tokio::test]
async fn test_async_text() {
    let body = text_message_case("tell me a story in 100 words").await;

    assert_eq!(request_text(&body), Some("tell me a story in 100 words"));
}

// upstream-test: chats/test_send_message.py::test_async_part
#[tokio::test]
async fn test_async_part() {
    let body = text_message_case(Part::from_text("tell me a story in 100 words")).await;

    assert_eq!(request_text(&body), Some("tell me a story in 100 words"));
}

// upstream-test: chats/test_send_message.py::test_async_parts
#[tokio::test]
async fn test_async_parts() {
    let body = text_message_case(vec![
        Part::from_text("tell me a US city"),
        Part::from_text("the city is in west coast"),
    ])
    .await;

    assert_eq!(
        body["contents"][0]["parts"].as_array().map(Vec::len),
        Some(2)
    );
}

// upstream-test: chats/test_send_message.py::test_async_config_override
#[tokio::test]
async fn test_async_config_override() {
    config_override_case().await;
}

// upstream-test: chats/test_send_message.py::test_async_history
#[tokio::test]
async fn test_async_history() {
    seeded_history_case().await;
}

// upstream-test: chats/test_send_message.py::test_async_stream_text
#[tokio::test]
async fn test_async_stream_text() {
    stream_text_case("tell me a story in 100 words", 2).await;
}

// upstream-test: chats/test_send_message.py::test_async_stream_part
#[tokio::test]
async fn test_async_stream_part() {
    stream_text_case(Part::from_text("tell me a story in 100 words"), 2).await;
}

// upstream-test: chats/test_send_message.py::test_async_stream_parts
#[tokio::test]
async fn test_async_stream_parts() {
    stream_text_case(
        vec![
            Part::from_text("tell me a story in 100 words"),
            Part::from_text("the story is about a car"),
        ],
        2,
    )
    .await;
}

// upstream-test: chats/test_send_message.py::test_async_stream_config_override
#[tokio::test]
async fn test_async_stream_config_override() {
    stream_config_override_case().await;
}

// upstream-test: chats/test_send_message.py::test_async_stream_function_calling
#[tokio::test]
async fn test_async_stream_function_calling() {
    stream_function_calling_case("chats_async_stream_divide").await;
}

// upstream-test: chats/test_send_message.py::test_async_stream_send_2_messages
#[tokio::test]
async fn test_async_stream_send_2_messages() {
    stream_send_2_messages_case().await;
}

// ---- tool definitions without a callable (MCP) ----------------------------

/// What upstream's `mcp_types.Tool(name='get_weather', ...)` becomes once
/// adapted for the request: a declaration carrying the MCP input schema.
/// Without a session behind it there is no callable, so AFC stays out of it.
fn get_weather_declaration_tool() -> Tool {
    Tool {
        function_declarations: Some(vec![FunctionDeclaration {
            name: Some("get_weather".to_owned()),
            description: Some("Get the weather in a city.".to_owned()),
            parameters_json_schema: Some(json!({
                "type": "object",
                "properties": {"location": {"type": "string"}}
            })),
            ..Default::default()
        }]),
        ..Default::default()
    }
}

async fn mcp_tool_definition_case(stream: bool) {
    let server = MockServer::start().await;
    let ask = function_call_reply("get_weather", &json!({"location": "Boston"}));
    if stream {
        mount_always(&server, sse_response(&[ask])).await;
    } else {
        mount_always(&server, ResponseTemplate::new(200).set_body_json(ask)).await;
    }
    let mut chat = chat_on(&server, Some(tool_config(get_weather_declaration_tool())));

    for city in ["Boston", "San Francisco"] {
        let message = format!("What is the weather in {city}?");
        if stream {
            stream_chunks(&mut chat, message, None).await;
        } else {
            chat.send_message(message, None).await.unwrap();
        }
    }

    // One request per message: the function call is handed back, not run.
    let bodies = request_bodies(&server).await;
    assert_eq!(bodies.len(), 2);
    for body in &bodies {
        let declaration = &field(
            &body["tools"][0],
            "functionDeclarations",
            "function_declarations",
        )[0];
        assert_eq!(declaration["name"], "get_weather");
        let schema = field(
            declaration,
            "parametersJsonSchema",
            "parameters_json_schema",
        );
        assert_eq!(schema["properties"]["location"]["type"], "string");
    }
    assert_eq!(chat.get_history(false).len(), 4);
    assert_eq!(
        function_call_of(&chat.get_history(false)[1])
            .name
            .as_deref(),
        Some("get_weather")
    );
}

// upstream-test: chats/test_send_message.py::test_mcp_tools
#[tokio::test]
async fn test_mcp_tools() {
    mcp_tool_definition_case(false).await;
}

// upstream-test: chats/test_send_message.py::test_mcp_tools_stream
#[tokio::test]
async fn test_mcp_tools_stream() {
    mcp_tool_definition_case(true).await;
}

// upstream-test: chats/test_send_message.py::test_async_mcp_tools
#[tokio::test]
async fn test_async_mcp_tools() {
    mcp_tool_definition_case(false).await;
}

// upstream-test: chats/test_send_message.py::test_async_mcp_tools_stream
#[tokio::test]
async fn test_async_mcp_tools_stream() {
    mcp_tool_definition_case(true).await;
}

// ---- server-side MCP tools -------------------------------------------------

fn server_side_mcp_config() -> GenerateContentConfig {
    tool_config(Tool {
        mcp_servers: Some(vec![McpServer {
            name: Some("weather_server".to_owned()),
            streamable_http_transport: Some(StreamableHttpTransport {
                url: Some("https://gemini-api-demos.uc.r.appspot.com/mcp".to_owned()),
                headers: Some(
                    [(
                        "AUTHORIZATION".to_owned(),
                        "Bearer github_pat_XXXX".to_owned(),
                    )]
                    .into_iter()
                    .collect(),
                ),
                timeout: Some("10s".to_owned()),
                ..Default::default()
            }),
        }]),
        ..Default::default()
    })
}

async fn server_side_mcp_case(stream: bool) {
    let server = MockServer::start().await;
    if stream {
        mount_always(&server, sse_response(&[text_chunk("Sunny.", true)])).await;
    } else {
        mount_always(
            &server,
            ResponseTemplate::new(200).set_body_json(model_reply("Sunny.")),
        )
        .await;
    }
    let mut chat = chat_on(&server, Some(server_side_mcp_config()));

    for message in [
        "What is the weather in Boston on 02/02/2026?",
        "What is the weather in San Francisco on 02/02/2026?",
    ] {
        if stream {
            stream_chunks(&mut chat, message, None).await;
        } else {
            chat.send_message(message, None).await.unwrap();
        }
    }

    // The MCP server is called by the API, so the server definition is sent
    // as-is on every request and the client runs no AFC round.
    let bodies = request_bodies(&server).await;
    assert_eq!(bodies.len(), 2);
    for body in &bodies {
        let mcp_server = &field(&body["tools"][0], "mcpServers", "mcp_servers")[0];
        assert_eq!(mcp_server["name"], "weather_server");
        let transport = field(
            mcp_server,
            "streamableHttpTransport",
            "streamable_http_transport",
        );
        assert_eq!(
            transport["url"],
            "https://gemini-api-demos.uc.r.appspot.com/mcp"
        );
        assert_eq!(transport["timeout"], "10s");
    }
    assert_eq!(chat.get_history(true).len(), 4);
}

// upstream-test: chats/test_send_message.py::test_server_side_mcp_tools
#[tokio::test]
async fn test_server_side_mcp_tools() {
    server_side_mcp_case(false).await;
}

// upstream-test: chats/test_send_message.py::test_server_side_mcp_tools_stream
#[tokio::test]
async fn test_server_side_mcp_tools_stream() {
    server_side_mcp_case(true).await;
}

// upstream-test: chats/test_send_message.py::test_async_server_side_mcp_tools
#[tokio::test]
async fn test_async_server_side_mcp_tools() {
    server_side_mcp_case(false).await;
}
