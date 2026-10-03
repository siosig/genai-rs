//! Ports of the plain tests in `google/genai/tests/models/test_generate_content_tools.py`.
//!
//! Upstream runs these against recorded API replays; here a `wiremock`
//! server plays the model and the assertions check the requests sent as well
//! as what the caller gets back. Rust has a single (async) `Models`, so the
//! upstream sync/async twins share their body and differ only in name. A
//! Python callable becomes a [`function_tool`] over a typed argument struct.
//!
//! The AFC tool registry is keyed by function name and shared by every test
//! in this binary, so each test registers its tools under a name of its own.

use std::sync::Arc;

use futures_util::StreamExt;
use gemini_genai::{
    Error, function_tool,
    types::{
        AutomaticFunctionCallingConfig, CodeExecutionResult, Content, FunctionCallingConfig,
        FunctionCallingConfigMode, FunctionDeclaration, GenerateContentConfig,
        GenerateContentResponse, GoogleSearch, McpServer, Part, StreamableHttpTransport,
        ThinkingConfig, Tool, ToolCodeExecution, ToolConfig,
    },
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use crate::common::test_client;

const MODEL: &str = "gemini-2.5-flash";

// ---------------------------------------------------------------- helpers

fn text_reply(text: &str) -> Value {
    json!({"candidates": [{
        "content": {"role": "model", "parts": [{"text": text}]},
        "finishReason": "STOP"
    }]})
}

fn calls_reply(calls: &[(&str, Value)]) -> Value {
    let parts: Vec<Value> = calls
        .iter()
        .map(|(name, args)| json!({"functionCall": {"name": name, "args": args}}))
        .collect();
    json!({"candidates": [{
        "content": {"role": "model", "parts": parts},
        "finishReason": "STOP"
    }]})
}

fn json_reply(value: &Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(value)
}

fn sse_reply(chunks: &[Value]) -> ResponseTemplate {
    let body: String = chunks
        .iter()
        .map(|c| format!("data: {c}\n\n"))
        .collect::<Vec<_>>()
        .concat();
    ResponseTemplate::new(200)
        .set_body_string(body)
        .insert_header("content-type", "text/event-stream")
}

/// A server answering its n-th request with the n-th of `replies`.
async fn serve(replies: Vec<ResponseTemplate>) -> MockServer {
    let server = MockServer::start().await;
    for reply in replies {
        Mock::given(method("POST"))
            .respond_with(reply)
            .up_to_n_times(1)
            .mount(&server)
            .await;
    }
    server
}

async fn bodies(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| request.body_json().unwrap())
        .collect()
}

fn config(tools: Vec<Tool>) -> GenerateContentConfig {
    GenerateContentConfig {
        tools: Some(tools),
        ..Default::default()
    }
}

fn with_afc(
    mut config: GenerateContentConfig,
    disable: Option<bool>,
    maximum_remote_calls: Option<i64>,
    ignore_call_history: Option<bool>,
) -> GenerateContentConfig {
    config.automatic_function_calling = Some(AutomaticFunctionCallingConfig {
        disable,
        maximum_remote_calls,
        ignore_call_history,
    });
    config
}

fn function_response(body: &Value, content: usize, part: usize) -> Value {
    body["contents"][content]["parts"][part]["functionResponse"].clone()
}

async fn generate(
    server: &MockServer,
    contents: &str,
    config: GenerateContentConfig,
) -> Result<GenerateContentResponse, Error> {
    test_client(server.uri())
        .models()
        .generate_content(MODEL, contents, Some(config))
        .await
}

async fn collect_stream(
    server: &MockServer,
    contents: &str,
    config: GenerateContentConfig,
) -> Vec<GenerateContentResponse> {
    test_client(server.uri())
        .models()
        .generate_content_stream(MODEL, contents, Some(config))
        .await
        .unwrap()
        .map(|chunk| chunk.unwrap())
        .collect()
        .await
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DivideArgs {
    a: i64,
    b: i64,
}

/// `divide_integers`: errors on a zero divisor like Python's `ZeroDivisionError`.
fn divide_tool(name: &str) -> Tool {
    Tool::from_function(function_tool::<DivideArgs, _, _, _>(
        name,
        "Divide two integers.",
        |args: DivideArgs| async move {
            if args.b == 0 {
                Err(Error::Validation("integer division by zero".to_owned()))
            } else {
                Ok(args.a / args.b)
            }
        },
    ))
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FloatArgs {
    a: f64,
    b: f64,
}

fn divide_floats_tool(name: &str) -> Tool {
    Tool::from_function(function_tool::<FloatArgs, _, _, _>(
        name,
        "Divide two floats.",
        |args: FloatArgs| async move { Ok(args.a / args.b) },
    ))
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CityArgs {
    city: String,
}

fn weather_tool(name: &str, answer: &'static str) -> Tool {
    Tool::from_function(function_tool::<CityArgs, _, _, _>(
        name,
        "Returns the current weather in the city.",
        move |args: CityArgs| async move { Ok(format!("The weather in {} is {answer}.", args.city)) },
    ))
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SymbolArgs {
    symbol: String,
}

fn stock_tool(name: &str) -> Tool {
    Tool::from_function(function_tool::<SymbolArgs, _, _, _>(
        name,
        "Returns a stock price.",
        |args: SymbolArgs| async move {
            Ok(if args.symbol == "GOOG" { "1000" } else { "100" }.to_owned())
        },
    ))
}

/// Serves `[divide(1000, 2) call, "500"]` and runs `generate_content`.
async fn divide_round(
    tag: &str,
    afc: Option<(Option<bool>, Option<i64>)>,
) -> (GenerateContentResponse, Vec<Value>) {
    let name = format!("divide_integers_{tag}");
    let server = serve(vec![
        json_reply(&calls_reply(&[(&name, json!({"a": 1000, "b": 2}))])),
        json_reply(&text_reply("500")),
    ])
    .await;
    let mut cfg = config(vec![divide_tool(&name)]);
    if let Some((disable, max)) = afc {
        cfg = with_afc(cfg, disable, max, Some(true));
    }
    let response = generate(&server, "what is the result of 1000/2?", cfg)
        .await
        .unwrap();
    let sent = bodies(&server).await;
    (response, sent)
}

/// Streaming twin of [`divide_round`].
async fn divide_stream_round(
    tag: &str,
    afc: Option<(Option<bool>, Option<i64>)>,
) -> (Vec<GenerateContentResponse>, Vec<Value>) {
    let name = format!("divide_integers_stream_{tag}");
    let server = serve(vec![
        sse_reply(&[calls_reply(&[(&name, json!({"a": 1000, "b": 2}))])]),
        sse_reply(&[text_reply("500")]),
    ])
    .await;
    let mut cfg = config(vec![divide_tool(&name)]);
    if let Some((disable, max)) = afc {
        cfg = with_afc(cfg, disable, max, Some(true));
    }
    let chunks = collect_stream(&server, "what is the result of 1000/2?", cfg).await;
    (chunks, bodies(&server).await)
}

// ------------------------------------------------------- built-in tools

// upstream-test: models/test_generate_content_tools.py::test_function_google_search
#[tokio::test]
async fn test_function_google_search() {
    // The service rejects google_search combined with a function declaration;
    // the 400 surfaces as an API error and both tools reach the wire.
    let server = serve(vec![ResponseTemplate::new(400).set_body_json(
        json!({"error": {
            "code": 400,
            "message": "Tool use with function calling is unsupported",
            "status": "INVALID_ARGUMENT"
        }}),
    )])
    .await;
    let cfg = ToolConfig {
        function_calling_config: Some(FunctionCallingConfig {
            mode: Some(FunctionCallingConfigMode::Auto),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut config = config(vec![
        Tool {
            google_search: Some(GoogleSearch::default()),
            ..Default::default()
        },
        stock_tool("get_stock_price_google_search"),
    ]);
    config.tool_config = Some(cfg);

    let error = generate(&server, "What is the price of GOOG?.", config)
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Api(_)), "got {error:?}");
    let sent = bodies(&server).await;
    assert_eq!(sent[0]["tools"][0], json!({"googleSearch": {}}));
    assert_eq!(
        sent[0]["tools"][1]["functionDeclarations"][0]["name"],
        "get_stock_price_google_search"
    );
}

fn schedule_meeting() -> FunctionDeclaration {
    serde_json::from_value(json!({
        "name": "schedule_meeting",
        "description": "Schedule a meeting",
        "parameters": {
            "type": "OBJECT",
            "properties": {"reason": {"type": "STRING"}},
            "required": ["reason"]
        }
    }))
    .unwrap()
}

fn server_side_invocations_config() -> ToolConfig {
    ToolConfig {
        include_server_side_tool_invocations: Some(true),
        ..Default::default()
    }
}

// upstream-test: models/test_generate_content_tools.py::test_function_google_search_server_side_tool_invocations
#[tokio::test]
async fn test_function_google_search_server_side_tool_invocations() {
    let server = serve(vec![json_reply(&text_reply("ok"))]).await;
    let mut cfg = config(vec![
        Tool {
            google_search: Some(GoogleSearch::default()),
            ..Default::default()
        },
        Tool {
            function_declarations: Some(vec![schedule_meeting()]),
            ..Default::default()
        },
    ]);
    cfg.tool_config = Some(server_side_invocations_config());

    generate(&server, "What is the weather in Buenos Aires?", cfg)
        .await
        .unwrap();

    let sent = bodies(&server).await;
    assert_eq!(
        sent[0]["tools"],
        json!([
            {"googleSearch": {}},
            {"functionDeclarations": [{
                "name": "schedule_meeting",
                "description": "Schedule a meeting",
                "parameters": {
                    "type": "OBJECT",
                    "properties": {"reason": {"type": "STRING"}},
                    "required": ["reason"]
                }
            }]}
        ])
    );
    assert_eq!(
        sent[0]["toolConfig"],
        json!({"includeServerSideToolInvocations": true})
    );
}

// upstream-test: models/test_generate_content_tools.py::test_function_google_search_server_side_tool_invocations_one_tool
#[tokio::test]
async fn test_function_google_search_server_side_tool_invocations_one_tool() {
    let server = serve(vec![json_reply(&text_reply("ok"))]).await;
    let mut cfg = config(vec![Tool {
        google_search: Some(GoogleSearch::default()),
        function_declarations: Some(vec![schedule_meeting()]),
        ..Default::default()
    }]);
    cfg.tool_config = Some(server_side_invocations_config());

    generate(&server, "What is the weather in Buenos Aires?", cfg)
        .await
        .unwrap();

    let sent = bodies(&server).await;
    assert_eq!(sent[0]["tools"].as_array().unwrap().len(), 1);
    assert_eq!(sent[0]["tools"][0]["googleSearch"], json!({}));
    assert_eq!(
        sent[0]["tools"][0]["functionDeclarations"][0]["name"],
        "schedule_meeting"
    );
    assert_eq!(
        sent[0]["toolConfig"]["includeServerSideToolInvocations"],
        true
    );
}

async fn google_search_stream_request(contents: &str) {
    let server = serve(vec![sse_reply(&[text_reply("Rayleigh scattering.")])]).await;
    let cfg = config(vec![Tool {
        google_search: Some(GoogleSearch::default()),
        ..Default::default()
    }]);

    let chunks = collect_stream(&server, contents, cfg).await;

    assert_eq!(chunks.len(), 1);
    let sent = bodies(&server).await;
    assert_eq!(sent[0]["tools"], json!([{"googleSearch": {}}]));
    assert_eq!(sent[0]["contents"][0]["parts"][0]["text"], contents);
}

// upstream-test: models/test_generate_content_tools.py::test_google_search_stream
#[tokio::test]
async fn test_google_search_stream() {
    google_search_stream_request("Why is the sky blue?").await;
}

// upstream-test: models/test_generate_content_tools.py::test_google_search_stream_async
#[tokio::test]
async fn test_google_search_stream_async() {
    google_search_stream_request("Why is the sky blue?").await;
}

async fn google_search_request() {
    let server = serve(vec![json_reply(&text_reply("Rayleigh scattering."))]).await;
    let cfg = config(vec![Tool {
        google_search: Some(GoogleSearch::default()),
        ..Default::default()
    }]);

    generate(&server, "Why is the sky blue?", cfg)
        .await
        .unwrap();

    let sent = bodies(&server).await;
    assert_eq!(sent[0]["tools"], json!([{"googleSearch": {}}]));
}

// upstream-test: models/test_generate_content_tools.py::test_google_search_async
#[tokio::test]
async fn test_google_search_async() {
    google_search_request().await;
}

// upstream-test: models/test_generate_content_tools.py::test_empty_tools
#[tokio::test]
async fn test_empty_tools() {
    let server = serve(vec![json_reply(&text_reply("ok"))]).await;

    generate(&server, "What is the price of GOOG?.", config(vec![]))
        .await
        .unwrap();

    // Python sends an empty `tools` array rather than omitting the key.
    assert_eq!(bodies(&server).await[0]["tools"], json!([]));
}

// upstream-test: models/test_generate_content_tools.py::test_with_1_empty_tool
#[tokio::test]
async fn test_with_1_empty_tool() {
    // On the Developer API an empty Tool is not rejected client-side; it is
    // sent as `{}` next to the callable's declaration (the service answers
    // 400, which upstream only asserts on Vertex).
    let server = serve(vec![json_reply(&text_reply("ok"))]).await;
    let cfg = with_afc(
        config(vec![
            Tool::default(),
            stock_tool("get_stock_price_one_empty"),
        ]),
        None,
        None,
        Some(true),
    );

    generate(&server, "What is the price of GOOG?.", cfg)
        .await
        .unwrap();

    let sent = bodies(&server).await;
    assert_eq!(sent[0]["tools"][0], json!({}));
    assert_eq!(
        sent[0]["tools"][1]["functionDeclarations"][0]["name"],
        "get_stock_price_one_empty"
    );
}

// upstream-test: models/test_generate_content_tools.py::test_vai_search_stream_async
#[tokio::test]
async fn test_vai_search_stream_async() {
    // Vertex AI Search retrieval is Vertex-only: the Developer API client
    // rejects it before any request is sent.
    let server = serve(vec![sse_reply(&[text_reply("unused")])]).await;
    let cfg = config(vec![serde_json::from_value(json!({
        "retrieval": {"vertex_ai_search": {"datastore": "projects/p/locations/global/collections/default_collection/dataStores/d"}}
    }))
    .unwrap()]);

    let result = test_client(server.uri())
        .models()
        .generate_content_stream(MODEL, "Why is the sky blue?", Some(cfg))
        .await;
    let error = match result {
        Err(error) => error,
        Ok(mut stream) => stream.next().await.unwrap().unwrap_err(),
    };

    assert!(error.to_string().contains("retrieval"), "got {error}");
    assert!(bodies(&server).await.is_empty());
}

// upstream-test: models/test_generate_content_tools.py::test_code_execution_tool
#[tokio::test]
async fn test_code_execution_tool() {
    let server = serve(vec![json_reply(&json!({"candidates": [{
        "content": {"role": "model", "parts": [
            {"executableCode": {"language": "PYTHON", "code": "print(sum(primes))"}},
            {"codeExecutionResult": {"outcome": "OUTCOME_OK", "output": "5117\n"}}
        ]},
        "finishReason": "STOP"
    }]}))])
    .await;
    let cfg = config(vec![Tool {
        code_execution: Some(ToolCodeExecution::default()),
        ..Default::default()
    }]);

    let response = generate(
        &server,
        "What is the sum of the first 50 prime numbers?",
        cfg,
    )
    .await
    .unwrap();

    assert!(response.executable_code().is_some());
    let result: &CodeExecutionResult = response.code_execution_result().unwrap();
    assert!(result.output.as_deref().unwrap().contains("5117"));
    assert_eq!(
        bodies(&server).await[0]["tools"],
        json!([{"codeExecution": {}}])
    );
}

fn mcp_config() -> GenerateContentConfig {
    config(vec![Tool {
        mcp_servers: Some(vec![McpServer {
            name: Some("get_weather".to_owned()),
            streamable_http_transport: Some(StreamableHttpTransport {
                url: Some("https://gemini-api-demos.uc.r.appspot.com/mcp".to_owned()),
                headers: Some(
                    [(
                        "AUTHORIZATION".to_owned(),
                        "Bearer github_pat_XXXX".to_owned(),
                    )]
                    .into(),
                ),
                ..Default::default()
            }),
        }]),
        ..Default::default()
    }])
}

fn assert_server_side_mcp_request(body: &Value) {
    // Server-side MCP servers go to the service verbatim (the transport keeps
    // its snake_case name on the wire, as in Python).
    let server = &body["tools"][0]["mcpServers"][0];
    assert_eq!(server["name"], "get_weather");
    assert_eq!(
        server["streamable_http_transport"]["url"],
        "https://gemini-api-demos.uc.r.appspot.com/mcp"
    );
    assert_eq!(
        server["streamable_http_transport"]["headers"]["AUTHORIZATION"],
        "Bearer github_pat_XXXX"
    );
}

async fn server_side_mcp_unary() {
    let server = serve(vec![json_reply(&text_reply("Sunny."))]).await;

    let response = generate(
        &server,
        "What is the weather like in New York?",
        mcp_config(),
    )
    .await
    .unwrap();

    assert_eq!(response.text().as_deref(), Some("Sunny."));
    let sent = bodies(&server).await;
    assert_eq!(sent.len(), 1, "server-side MCP must not trigger AFC");
    assert_server_side_mcp_request(&sent[0]);
}

// upstream-test: models/test_generate_content_tools.py::test_server_side_mcp_only
#[tokio::test]
async fn test_server_side_mcp_only() {
    server_side_mcp_unary().await;
}

// upstream-test: models/test_generate_content_tools.py::test_server_side_mcp_only_async
#[tokio::test]
async fn test_server_side_mcp_only_async() {
    server_side_mcp_unary().await;
}

// upstream-test: models/test_generate_content_tools.py::test_server_side_mcp_only_stream
#[tokio::test]
async fn test_server_side_mcp_only_stream() {
    let server = serve(vec![sse_reply(&[text_reply("Sunny.")])]).await;

    let chunks = collect_stream(
        &server,
        "What is the weather like in New York?",
        mcp_config(),
    )
    .await;

    assert_eq!(chunks.len(), 1);
    assert_server_side_mcp_request(&bodies(&server).await[0]);
}

// ----------------------------------------------- function declarations

// upstream-test: models/test_generate_content_tools.py::test_function_calling_without_implementation
#[tokio::test]
async fn test_function_calling_without_implementation() {
    // A declaration with no registered callable: AFC has nothing to run, so
    // the model's function call comes back to the caller after one request.
    let server = serve(vec![json_reply(&calls_reply(&[(
        "get_weather_declaration_only",
        json!({"city": "Boston"}),
    )]))])
    .await;
    let declaration = FunctionDeclaration {
        name: Some("get_weather_declaration_only".to_owned()),
        description: Some("Get the current weather in a given city.".to_owned()),
        ..Default::default()
    };
    let cfg = with_afc(
        config(vec![Tool {
            function_declarations: Some(vec![declaration]),
            ..Default::default()
        }]),
        None,
        None,
        Some(true),
    );

    let response = generate(&server, "What is the weather in Boston?", cfg)
        .await
        .unwrap();

    assert_eq!(response.function_calls().len(), 1);
    assert_eq!(bodies(&server).await.len(), 1);
}

async fn two_functions_round(tag: &str) {
    let weather = format!("get_weather_{tag}");
    let stock = format!("get_stock_price_{tag}");
    let server = serve(vec![
        json_reply(&calls_reply(&[
            (&stock, json!({"symbol": "GOOG"})),
            (&weather, json!({"city": "Boston"})),
        ])),
        json_reply(&text_reply(
            "GOOG is 1000. The weather in Boston is sunny and 100 degrees.",
        )),
    ])
    .await;
    let cfg = with_afc(
        config(vec![
            weather_tool(&weather, "sunny and 100 degrees"),
            stock_tool(&stock),
        ]),
        None,
        None,
        Some(true),
    );

    let response = generate(
        &server,
        "What is the price of GOOG? And what is the weather in Boston?",
        cfg,
    )
    .await
    .unwrap();

    let text = response.text().unwrap();
    assert!(text.contains("1000") && text.contains("Boston") && text.contains("sunny"));
    assert!(response.automatic_function_calling_history.is_none());
    let sent = bodies(&server).await;
    assert_eq!(sent.len(), 2);
    assert_eq!(
        function_response(&sent[1], 2, 0)["response"],
        json!({"result": "1000"})
    );
    assert_eq!(
        function_response(&sent[1], 2, 1)["response"],
        json!({"result": "The weather in Boston is sunny and 100 degrees."})
    );
}

// upstream-test: models/test_generate_content_tools.py::test_2_function
#[tokio::test]
async fn test_2_function() {
    two_functions_round("2fn").await;
}

// upstream-test: models/test_generate_content_tools.py::test_2_function_async
#[tokio::test]
async fn test_2_function_async() {
    two_functions_round("2fn_async").await;
}

async fn two_functions_history(tag: &str) {
    let weather = format!("get_weather_{tag}");
    let stock = format!("get_stock_price_{tag}");
    let prompt = "What is the price of GOOG? And what is the weather in Boston?";
    let server = serve(vec![
        json_reply(&calls_reply(&[
            (&stock, json!({"symbol": "GOOG"})),
            (&weather, json!({"city": "Boston"})),
        ])),
        json_reply(&text_reply("done")),
    ])
    .await;
    let cfg = with_afc(
        config(vec![
            weather_tool(&weather, "sunny and 100 degrees"),
            stock_tool(&stock),
        ]),
        None,
        None,
        Some(false),
    );

    let response = generate(&server, prompt, cfg).await.unwrap();

    let history = response.automatic_function_calling_history.unwrap();
    assert_eq!(history.len(), 3);
    let part = |content: usize, index: usize| -> &Part {
        &history[content].parts.as_ref().unwrap()[index]
    };
    assert_eq!(history[0].role.as_deref(), Some("user"));
    assert_eq!(part(0, 0).text.as_deref(), Some(prompt));
    assert_eq!(history[1].role.as_deref(), Some("model"));
    assert_eq!(
        serde_json::to_value(part(1, 0).function_call.as_ref().unwrap()).unwrap(),
        json!({"name": stock, "args": {"symbol": "GOOG"}})
    );
    assert_eq!(
        serde_json::to_value(part(1, 1).function_call.as_ref().unwrap()).unwrap(),
        json!({"name": weather, "args": {"city": "Boston"}})
    );
    assert_eq!(history[2].role.as_deref(), Some("user"));
    assert_eq!(
        serde_json::to_value(part(2, 0).function_response.as_ref().unwrap()).unwrap(),
        json!({"name": stock, "response": {"result": "1000"}})
    );
    assert_eq!(
        serde_json::to_value(part(2, 1).function_response.as_ref().unwrap()).unwrap(),
        json!({
            "name": weather,
            "response": {"result": "The weather in Boston is sunny and 100 degrees."}
        })
    );
}

// upstream-test: models/test_generate_content_tools.py::test_2_function_with_history
#[tokio::test]
async fn test_2_function_with_history() {
    two_functions_history("hist").await;
}

// upstream-test: models/test_generate_content_tools.py::test_2_function_with_history_async
#[tokio::test]
async fn test_2_function_with_history_async() {
    two_functions_history("hist_async").await;
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_with_customized_math_rule
#[tokio::test]
async fn test_automatic_function_calling_with_customized_math_rule() {
    let name = "customized_divide_integers";
    let tool = Tool::from_function(function_tool::<DivideArgs, _, _, _>(
        name,
        "Divide two integers with customized math rule.",
        |args: DivideArgs| async move { Ok(args.a / args.b + 1) },
    ));
    let server = serve(vec![
        json_reply(&calls_reply(&[(name, json!({"a": 1000, "b": 2}))])),
        json_reply(&text_reply("501")),
    ])
    .await;

    let response = generate(&server, "what is the result of 1000/2?", config(vec![tool]))
        .await
        .unwrap();

    assert!(response.text().unwrap().contains("501"));
    assert_eq!(
        function_response(&bodies(&server).await[1], 2, 0)["response"],
        json!({"result": 501})
    );
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling
#[tokio::test]
async fn test_automatic_function_calling() {
    let (response, sent) = divide_round("afc", Some((None, None))).await;

    assert!(response.text().unwrap().contains("500"));
    assert_eq!(sent.len(), 2);
    assert_eq!(
        function_response(&sent[1], 2, 0)["response"],
        json!({"result": 500})
    );
    // ignore_call_history: the history is not attached to the response.
    assert!(response.automatic_function_calling_history.is_none());
}

async fn async_float_division(tag: &str) {
    let name = format!("divide_floats_async_{tag}");
    let tool = Tool::from_function(function_tool::<FloatArgs, _, _, _>(
        &name,
        "Divide two floats.",
        |args: FloatArgs| async move {
            tokio::task::yield_now().await;
            Ok(args.a / args.b)
        },
    ));
    let server = serve(vec![
        json_reply(&calls_reply(&[(&name, json!({"a": 1001.0, "b": 2.0}))])),
        json_reply(&text_reply("500.5")),
    ])
    .await;
    let cfg = with_afc(config(vec![tool]), None, None, Some(true));

    let response = generate(&server, "what is the result of 1001.0/2.0?", cfg)
        .await
        .unwrap();

    assert!(response.text().unwrap().contains("500.5"));
    assert_eq!(
        function_response(&bodies(&server).await[1], 2, 0)["response"]["result"],
        500.5
    );
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_with_async_function
#[tokio::test]
async fn test_automatic_function_calling_with_async_function() {
    async_float_division("with_async").await;
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_with_coroutine_function_async
#[tokio::test]
async fn test_automatic_function_calling_with_coroutine_function_async() {
    let (response, sent) = divide_round("coroutine_async", Some((None, None))).await;

    assert!(response.text().unwrap().contains("500"));
    assert_eq!(sent.len(), 2);
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_async
#[tokio::test]
async fn test_automatic_function_calling_async() {
    let (response, sent) = divide_round("async", Some((None, None))).await;

    assert!(response.text().unwrap().contains("500"));
    assert_eq!(sent.len(), 2);
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_with_exception
#[tokio::test]
async fn test_automatic_function_calling_with_exception() {
    let name = "divide_integers_exception";
    let server = serve(vec![
        json_reply(&calls_reply(&[(name, json!({"a": 1000, "b": 0}))])),
        json_reply(&text_reply("Cannot divide by zero.")),
    ])
    .await;
    let cfg = with_afc(config(vec![divide_tool(name)]), None, None, Some(true));

    let response = generate(&server, "what is the result of 1000/0?", cfg)
        .await
        .unwrap();

    // The failure is reported to the model as an `error` response, not raised.
    assert_eq!(response.text().as_deref(), Some("Cannot divide by zero."));
    let reported = function_response(&bodies(&server).await[1], 2, 0);
    assert!(reported["response"]["error"].is_string(), "got {reported}");
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_async_with_exception
#[tokio::test]
async fn test_automatic_function_calling_async_with_exception() {
    let name = "divide_integers_async_exception";
    let server = serve(vec![
        json_reply(&calls_reply(&[(name, json!({"a": 1000, "b": 0}))])),
        json_reply(&text_reply("Cannot divide by zero.")),
    ])
    .await;

    let response = generate(
        &server,
        "what is the result of 1000/0?",
        config(vec![divide_tool(name)]),
    )
    .await
    .unwrap();

    let history = response.automatic_function_calling_history.unwrap();
    let last = history.last().unwrap();
    let reported = last.parts.as_ref().unwrap()[0]
        .function_response
        .as_ref()
        .unwrap()
        .response
        .as_ref()
        .unwrap();
    assert!(reported["error"].is_string(), "got {reported:?}");
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_float_without_decimal
#[tokio::test]
async fn test_automatic_function_calling_float_without_decimal() {
    let floats = "divide_floats_no_decimal";
    let server = serve(vec![
        json_reply(&calls_reply(&[(floats, json!({"a": 1000.0, "b": 2.0}))])),
        json_reply(&text_reply("500.0")),
    ])
    .await;
    let cfg = with_afc(
        config(vec![
            divide_floats_tool(floats),
            divide_tool("divide_integers_no_decimal"),
        ]),
        None,
        None,
        Some(true),
    );

    let response = generate(&server, "what is the result of 1000.0/2.0?", cfg)
        .await
        .unwrap();

    assert!(response.text().unwrap().contains("500.0"));
    assert_eq!(
        function_response(&bodies(&server).await[1], 2, 0)["response"]["result"].as_f64(),
        Some(500.0)
    );
}

// ------------------------------------------------ typed (pydantic) args

#[derive(Debug, Deserialize, JsonSchema)]
struct CityObject {
    city_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WeatherObjectArgs {
    city_object: CityObject,
    is_winter: bool,
}

async fn pydantic_model_round(tag: &str) {
    let name = format!("get_weather_pydantic_model_{tag}");
    let tool = Tool::from_function(function_tool::<WeatherObjectArgs, _, _, _>(
        &name,
        "Weather for a city object.",
        |args: WeatherObjectArgs| async move {
            Ok(if args.is_winter {
                format!(
                    "The weather in {} is cold and 10 degrees.",
                    args.city_object.city_name
                )
            } else {
                format!(
                    "The weather in {} is sunny and 100 degrees.",
                    args.city_object.city_name
                )
            })
        },
    ));
    let server = serve(vec![
        json_reply(&calls_reply(&[(
            &name,
            json!({"city_object": {"city_name": "Boston"}, "is_winter": true}),
        )])),
        json_reply(&text_reply("It is cold in Boston.")),
    ])
    .await;
    let cfg = with_afc(config(vec![tool]), None, None, Some(true));

    let response = generate(
        &server,
        "it is winter now, what is the weather in Boston?",
        cfg,
    )
    .await
    .unwrap();

    let text = response.text().unwrap();
    assert!(text.contains("cold") && text.contains("Boston"));
    let sent = bodies(&server).await;
    assert_eq!(
        function_response(&sent[1], 2, 0)["response"]["result"],
        "The weather in Boston is cold and 10 degrees."
    );
    // The declaration carries the nested object's schema.
    let schema = &sent[0]["tools"][0]["functionDeclarations"][0]["parameters_json_schema"];
    assert_eq!(schema["properties"]["is_winter"]["type"], "boolean");
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_with_pydantic_model
#[tokio::test]
async fn test_automatic_function_calling_with_pydantic_model() {
    pydantic_model_round("sync").await;
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_async_with_pydantic_model
#[tokio::test]
async fn test_automatic_function_calling_async_with_pydantic_model() {
    pydantic_model_round("async").await;
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CityListArgs {
    city_object_list: Vec<CityObject>,
    is_winter: bool,
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_with_pydantic_model_in_list_type
#[tokio::test]
async fn test_automatic_function_calling_with_pydantic_model_in_list_type() {
    let name = "get_weather_from_list_of_cities";
    let tool = Tool::from_function(function_tool::<CityListArgs, _, _, _>(
        name,
        "Weather for a list of city objects.",
        |args: CityListArgs| async move {
            let season = if args.is_winter {
                "cold and 10"
            } else {
                "sunny and 100"
            };
            Ok(args
                .city_object_list
                .iter()
                .map(|city| format!("The weather in {} is {season} degrees.\n", city.city_name))
                .collect::<Vec<_>>()
                .concat())
        },
    ));
    let server = serve(vec![
        json_reply(&calls_reply(&[(
            name,
            json!({
                "city_object_list": [{"city_name": "Boston"}, {"city_name": "New York"}],
                "is_winter": true
            }),
        )])),
        json_reply(&text_reply("Cold in Boston and New York.")),
    ])
    .await;
    let cfg = with_afc(config(vec![tool]), None, None, Some(true));

    let response = generate(
        &server,
        "it is winter now, weather in Boston and New York?",
        cfg,
    )
    .await
    .unwrap();

    assert!(response.text().unwrap().contains("New York"));
    assert_eq!(
        function_response(&bodies(&server).await[1], 2, 0)["response"]["result"],
        "The weather in Boston is cold and 10 degrees.\nThe weather in New York is cold and 10 degrees.\n"
    );
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AnimalObject {
    name: String,
    age: i64,
    species: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
enum StrOrAnimal {
    Text(String),
    Animal(AnimalObject),
}

#[derive(Debug, Deserialize, JsonSchema)]
struct InformationArgs {
    object_of_interest: StrOrAnimal,
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_with_union_operator
#[tokio::test]
async fn test_automatic_function_calling_with_union_operator() {
    let name = "get_information_union_operator";
    let tool = Tool::from_function(function_tool::<InformationArgs, _, _, _>(
        name,
        "Describes an object.",
        |args: InformationArgs| async move {
            Ok(match args.object_of_interest {
                StrOrAnimal::Animal(animal) => format!(
                    "The animal is of {} species and is named {} is {} years old",
                    animal.species, animal.name, animal.age
                ),
                StrOrAnimal::Text(text) => format!("The object of interest is {text}"),
            })
        },
    ));
    let server = serve(vec![
        json_reply(&calls_reply(&[(
            name,
            json!({"object_of_interest": {"name": "Sundae", "age": 1, "species": "cat"}}),
        )])),
        json_reply(&text_reply("Sundae is a one year old cat.")),
    ])
    .await;
    let cfg = with_afc(config(vec![tool]), None, None, Some(true));

    let response = generate(&server, "I have a one year old cat named Sundae", cfg)
        .await
        .unwrap();

    assert!(response.text().is_some());
    assert_eq!(
        function_response(&bodies(&server).await[1], 2, 0)["response"]["result"],
        "The animal is of cat species and is named Sundae is 1 years old"
    );
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LatLngArgs {
    latlng: (f64, f64),
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_with_tuple_param
#[tokio::test]
async fn test_automatic_function_calling_with_tuple_param() {
    let name = "output_latlng";
    let tool = Tool::from_function(function_tool::<LatLngArgs, _, _, _>(
        name,
        "Formats coordinates.",
        |args: LatLngArgs| async move {
            Ok(format!(
                "The latitude is {} and the longitude is {}",
                args.latlng.0, args.latlng.1
            ))
        },
    ));
    let server = serve(vec![
        json_reply(&calls_reply(&[(name, json!({"latlng": [51.509, -0.118]}))])),
        json_reply(&text_reply("Latitude 51.509, longitude -0.118.")),
    ])
    .await;
    let cfg = with_afc(config(vec![tool]), None, None, Some(true));

    let response = generate(&server, "The coordinates are (51.509, -0.118).", cfg)
        .await
        .unwrap();

    assert!(response.text().is_some());
    assert_eq!(
        function_response(&bodies(&server).await[1], 2, 0)["response"]["result"],
        "The latitude is 51.509 and the longitude is -0.118"
    );
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DescribeCitiesArgs {
    country: String,
    #[serde(default)]
    cities: Option<Vec<String>>,
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_with_parameterized_generic_union_type
#[tokio::test]
async fn test_automatic_function_calling_with_parameterized_generic_union_type() {
    let name = "describe_cities";
    let tool = Tool::from_function(function_tool::<DescribeCitiesArgs, _, _, _>(
        name,
        "Given a country and an optional list of cities, describe the cities.",
        |args: DescribeCitiesArgs| async move {
            Ok(match args.cities {
                None => "There are no cities to describe.".to_owned(),
                Some(cities) => format!(
                    "The cities in {} are: {} and they are nice.",
                    args.country,
                    cities.join(", ")
                ),
            })
        },
    ));
    let server = serve(vec![
        // The model omits the optional `cities` argument.
        json_reply(&calls_reply(&[(name, json!({"country": "USA"}))])),
        json_reply(&text_reply(
            "There are no cities to describe for San Francisco.",
        )),
    ])
    .await;
    let cfg = with_afc(config(vec![tool]), None, None, Some(true));

    let response = generate(&server, "Can you describe San Francisco, USA?", cfg)
        .await
        .unwrap();

    assert!(response.text().unwrap().contains("San Francisco"));
    let sent = bodies(&server).await;
    assert_eq!(
        function_response(&sent[1], 2, 0)["response"]["result"],
        "There are no cities to describe."
    );
    let schema = &sent[0]["tools"][0]["functionDeclarations"][0]["parameters_json_schema"];
    assert_eq!(schema["required"], json!(["country"]));
}

// ---------------------------------------------------------- stream AFC

fn assert_text_or_finish(chunks: &[GenerateContentResponse]) {
    assert!(!chunks.is_empty());
    for chunk in chunks {
        let finished = chunk
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .is_some_and(|c| c.finish_reason.is_some());
        assert!(chunk.text().is_some() || finished, "chunk: {chunk:?}");
    }
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_stream
#[tokio::test]
async fn test_automatic_function_calling_stream() {
    let (chunks, sent) = divide_stream_round("afc", Some((None, None))).await;

    assert_text_or_finish(&chunks);
    assert_eq!(chunks.last().unwrap().text().as_deref(), Some("500"));
    assert_eq!(sent.len(), 2);
    assert_eq!(
        function_response(&sent[1], 2, 0)["response"],
        json!({"result": 500})
    );
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_stream_async
#[tokio::test]
async fn test_automatic_function_calling_stream_async() {
    let (chunks, sent) = divide_stream_round("afc_async", Some((None, None))).await;

    assert_text_or_finish(&chunks);
    assert_eq!(sent.len(), 2);
}

// upstream-test: models/test_generate_content_tools.py::test_disable_automatic_function_calling_stream
#[tokio::test]
async fn test_disable_automatic_function_calling_stream() {
    // With AFC disabled the stream is the model's function call, untouched.
    let (chunks, sent) = divide_stream_round("disabled", Some((Some(true), None))).await;

    assert_eq!(sent.len(), 1);
    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert_eq!(chunk.function_calls().len(), 1);
    }
}

// upstream-test: models/test_generate_content_tools.py::test_disable_automatic_function_calling_stream_async
#[tokio::test]
async fn test_disable_automatic_function_calling_stream_async() {
    let (chunks, sent) = divide_stream_round("disabled_async", Some((Some(true), None))).await;

    assert_eq!(sent.len(), 1);
    for chunk in &chunks {
        assert_eq!(chunk.function_calls().len(), 1);
    }
}

async fn no_function_call_stream(tag: &str) {
    // The model answers in text without calling the tool: one request, no AFC.
    let name = format!("divide_integers_no_call_{tag}");
    let server = serve(vec![sse_reply(&[text_reply("It is sunny in Boston.")])]).await;
    let cfg = with_afc(config(vec![divide_tool(&name)]), None, None, Some(true));

    let chunks = collect_stream(&server, "what is the weather in Boston?", cfg).await;

    assert_text_or_finish(&chunks);
    assert_eq!(bodies(&server).await.len(), 1);
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_no_function_response_stream
#[tokio::test]
async fn test_automatic_function_calling_no_function_response_stream() {
    no_function_call_stream("sync").await;
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_no_function_response_stream_async
#[tokio::test]
async fn test_automatic_function_calling_no_function_response_stream_async() {
    no_function_call_stream("async").await;
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_async_with_async_function
#[tokio::test]
async fn test_automatic_function_calling_async_with_async_function() {
    let name = "get_current_weather_async";
    let server = serve(vec![
        json_reply(&calls_reply(&[(name, json!({"city": "San Francisco"}))])),
        json_reply(&text_reply("It is windy in San Francisco.")),
    ])
    .await;
    let tool = Tool::from_function(function_tool::<CityArgs, _, _, _>(
        name,
        "Returns the current weather in the city.",
        |_args: CityArgs| async move {
            tokio::task::yield_now().await;
            Ok("windy".to_owned())
        },
    ));
    let cfg = with_afc(config(vec![tool]), None, None, Some(true));

    let response = generate(&server, "what is the weather in San Francisco?", cfg)
        .await
        .unwrap();

    let text = response.text().unwrap();
    assert!(text.contains("windy") && text.contains("San Francisco"));
    assert_eq!(
        function_response(&bodies(&server).await[1], 2, 0)["response"],
        json!({"result": "windy"})
    );
}

// upstream-test: models/test_generate_content_tools.py::test_automatic_function_calling_async_with_async_function_stream
#[tokio::test]
async fn test_automatic_function_calling_async_with_async_function_stream() {
    let name = "get_current_weather_async_stream";
    let server = serve(vec![
        sse_reply(&[calls_reply(&[(name, json!({"city": "San Francisco"}))])]),
        sse_reply(&[text_reply("It is windy in San Francisco.")]),
    ])
    .await;
    let tool = Tool::from_function(function_tool::<CityArgs, _, _, _>(
        name,
        "Returns the current weather in the city.",
        |_args: CityArgs| async move { Ok("windy".to_owned()) },
    ));
    let cfg = with_afc(config(vec![tool]), None, None, Some(true));

    let chunks = collect_stream(&server, "what is the weather in San Francisco?", cfg).await;

    let calls: Vec<_> = chunks.iter().flat_map(|c| c.function_calls()).collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name.as_deref(), Some(name));
    assert_eq!(calls[0].args.as_ref().unwrap()["city"], "San Francisco");
}

// upstream-test: models/test_generate_content_tools.py::test_stream_afc_thoughts
#[tokio::test]
async fn test_stream_afc_thoughts() {
    let name = "add_numbers";
    let server = serve(vec![
        sse_reply(&[
            json!({"candidates": [{"content": {"role": "model", "parts": [
                {"text": "I should add them.", "thought": true}
            ]}}]}),
            calls_reply(&[(name, json!({"a": 1_234_567.89, "b": 9_876_543.21}))]),
        ]),
        sse_reply(&[text_reply("The sum is 11111111.1.")]),
    ])
    .await;
    let tool = Tool::from_function(function_tool::<FloatArgs, _, _, _>(
        name,
        "Adds two numbers and returns the sum.",
        |args: FloatArgs| async move { Ok(args.a + args.b) },
    ));
    let mut cfg = config(vec![tool]);
    cfg.thinking_config = Some(ThinkingConfig {
        include_thoughts: Some(true),
        ..Default::default()
    });

    let chunks = collect_stream(
        &server,
        "Calculate 1_234_567.89 + 9_876_543.21 with add_numbers.",
        cfg,
    )
    .await;

    let calls: Vec<_> = chunks.iter().flat_map(|c| c.function_calls()).collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name.as_deref(), Some(name));
    assert_eq!(calls[0].args.as_ref().unwrap()["a"], 1_234_567.89);
    assert_eq!(calls[0].args.as_ref().unwrap()["b"], 9_876_543.21);
    let sum_chunks = chunks
        .iter()
        .filter_map(GenerateContentResponse::text)
        .filter(|text| text.contains(".1."))
        .count();
    assert_eq!(sum_chunks, 1);
    let sent = bodies(&server).await;
    assert_eq!(sent.len(), 2);
    // Python leaves the nested config field snake_case on the wire.
    assert_eq!(
        sent[0]["generationConfig"]["thinkingConfig"]["include_thoughts"],
        true
    );
}

// ------------------------------------------------------ AFC configuration

/// Runs `divide_round` with an explicit AFC config and checks how many
/// requests were sent: 1 when AFC is off (the model's call is returned
/// unexecuted), 2 when the call was executed and its result sent back.
async fn check_afc_config(
    tag: &str,
    disable: Option<bool>,
    maximum_remote_calls: Option<i64>,
    requests: usize,
) {
    let (response, sent) = divide_round(tag, Some((disable, maximum_remote_calls))).await;

    assert_eq!(
        sent.len(),
        requests,
        "requests sent with disable={disable:?} maximum_remote_calls={maximum_remote_calls:?}"
    );
    if requests == 1 {
        assert_eq!(response.function_calls().len(), 1);
    } else {
        assert_eq!(response.text().as_deref(), Some("500"));
    }
}

// upstream-test: models/test_generate_content_tools.py::test_callable_tools_user_disable_afc
#[tokio::test]
async fn test_callable_tools_user_disable_afc() {
    check_afc_config("test_callable_tools_user_disable_afc", Some(true), None, 1).await;
}

// upstream-test: models/test_generate_content_tools.py::test_callable_tools_user_disable_afc_with_max_remote_calls
#[tokio::test]
async fn test_callable_tools_user_disable_afc_with_max_remote_calls() {
    check_afc_config(
        "test_callable_tools_user_disable_afc_with_max_remote_calls",
        Some(true),
        Some(2),
        1,
    )
    .await;
}

// upstream-test: models/test_generate_content_tools.py::test_callable_tools_user_disable_afc_with_max_remote_calls_negative
#[tokio::test]
async fn test_callable_tools_user_disable_afc_with_max_remote_calls_negative() {
    check_afc_config(
        "test_callable_tools_user_disable_afc_with_max_remote_calls_negative",
        Some(true),
        Some(-1),
        1,
    )
    .await;
}

// upstream-test: models/test_generate_content_tools.py::test_callable_tools_user_disable_afc_with_max_remote_calls_zero
#[tokio::test]
async fn test_callable_tools_user_disable_afc_with_max_remote_calls_zero() {
    check_afc_config(
        "test_callable_tools_user_disable_afc_with_max_remote_calls_zero",
        Some(true),
        Some(0),
        1,
    )
    .await;
}

// upstream-test: models/test_generate_content_tools.py::test_callable_tools_user_enable_afc
#[tokio::test]
async fn test_callable_tools_user_enable_afc() {
    check_afc_config("test_callable_tools_user_enable_afc", Some(false), None, 2).await;
}

// upstream-test: models/test_generate_content_tools.py::test_callable_tools_user_enable_afc_with_max_remote_calls
#[tokio::test]
async fn test_callable_tools_user_enable_afc_with_max_remote_calls() {
    check_afc_config(
        "test_callable_tools_user_enable_afc_with_max_remote_calls",
        Some(false),
        Some(2),
        2,
    )
    .await;
}

// upstream-test: models/test_generate_content_tools.py::test_callable_tools_user_enable_afc_with_max_remote_calls_negative
#[tokio::test]
async fn test_callable_tools_user_enable_afc_with_max_remote_calls_negative() {
    check_afc_config(
        "test_callable_tools_user_enable_afc_with_max_remote_calls_negative",
        Some(false),
        Some(-1),
        1,
    )
    .await;
}

// upstream-test: models/test_generate_content_tools.py::test_callable_tools_user_enable_afc_with_max_remote_calls_zero
#[tokio::test]
async fn test_callable_tools_user_enable_afc_with_max_remote_calls_zero() {
    check_afc_config(
        "test_callable_tools_user_enable_afc_with_max_remote_calls_zero",
        Some(false),
        Some(0),
        1,
    )
    .await;
}

fn any_mode_config(name: &str, afc: AutomaticFunctionCallingConfig) -> GenerateContentConfig {
    let mut cfg = config(vec![divide_tool(name)]);
    cfg.automatic_function_calling = Some(afc);
    cfg.tool_config = Some(ToolConfig {
        function_calling_config: Some(FunctionCallingConfig {
            mode: Some(FunctionCallingConfigMode::Any),
            ..Default::default()
        }),
        ..Default::default()
    });
    cfg
}

// upstream-test: models/test_generate_content_tools.py::test_disable_afc_in_any_mode
#[tokio::test]
async fn test_disable_afc_in_any_mode() {
    let name = "divide_integers_any_disabled";
    let server = serve(vec![json_reply(&calls_reply(&[(
        name,
        json!({"a": 1000, "b": 2}),
    )]))])
    .await;
    let cfg = any_mode_config(
        name,
        AutomaticFunctionCallingConfig {
            disable: Some(true),
            ..Default::default()
        },
    );

    let response = generate(&server, "what is the result of 1000/2?", cfg)
        .await
        .unwrap();

    assert_eq!(response.function_calls().len(), 1);
    let sent = bodies(&server).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0]["toolConfig"]["functionCallingConfig"]["mode"],
        "ANY"
    );
}

// upstream-test: models/test_generate_content_tools.py::test_afc_once_in_any_mode
#[tokio::test]
async fn test_afc_once_in_any_mode() {
    let name = "divide_integers_any_once";
    let server = serve(vec![
        json_reply(&calls_reply(&[(name, json!({"a": 1000, "b": 2}))])),
        json_reply(&text_reply("500")),
    ])
    .await;
    let cfg = any_mode_config(
        name,
        AutomaticFunctionCallingConfig {
            maximum_remote_calls: Some(2),
            ..Default::default()
        },
    );

    let response = generate(&server, "what is the result of 1000/2?", cfg)
        .await
        .unwrap();

    assert_eq!(response.text().as_deref(), Some("500"));
    let sent = bodies(&server).await;
    assert_eq!(sent.len(), 2);
    for body in &sent {
        assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], "ANY");
    }
}

// ------------------------------------------------- mixed declarations

struct FunctionHolder {
    name: &'static str,
}

impl FunctionHolder {
    fn is_a_duck(&self, number: i64) -> String {
        format!("{}says isOdd: {}", self.name, number % 2 == 1)
    }

    fn is_a_rabbit(&self, number: i64) -> String {
        format!("{}says isEven: {}", self.name, number % 2 == 0)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct NumberArgs {
    number: i64,
}

// upstream-test: models/test_generate_content_tools.py::test_class_method_tools
#[tokio::test]
async fn test_class_method_tools() {
    // Methods of a value are used as tools by capturing it in the closure.
    let holder = Arc::new(FunctionHolder {
        name: "FunctionHolder",
    });
    let (duck_holder, rabbit_holder) = (Arc::clone(&holder), Arc::clone(&holder));
    let duck = Tool::from_function(function_tool::<NumberArgs, _, _, _>(
        "is_a_duck",
        "Checks oddness.",
        move |args: NumberArgs| {
            let holder = Arc::clone(&duck_holder);
            async move { Ok(holder.is_a_duck(args.number)) }
        },
    ));
    let rabbit = Tool::from_function(function_tool::<NumberArgs, _, _, _>(
        "is_a_rabbit",
        "Checks evenness.",
        move |args: NumberArgs| {
            let holder = Arc::clone(&rabbit_holder);
            async move { Ok(holder.is_a_rabbit(args.number)) }
        },
    ));
    let server = serve(vec![
        json_reply(&calls_reply(&[
            ("is_a_duck", json!({"number": 100})),
            ("is_a_rabbit", json!({"number": 100})),
        ])),
        json_reply(&text_reply(
            "FunctionHoldersays isOdd: false; FunctionHoldersays isEven: true",
        )),
    ])
    .await;

    let response = generate(
        &server,
        "Print the verbatim output of is_a_duck and is_a_rabbit for the number 100.",
        config(vec![duck, rabbit]),
    )
    .await
    .unwrap();

    assert!(response.text().unwrap().contains("FunctionHolder"));
    let sent = bodies(&server).await;
    assert_eq!(
        function_response(&sent[1], 2, 0)["response"]["result"],
        "FunctionHoldersays isOdd: false"
    );
    assert_eq!(
        function_response(&sent[1], 2, 1)["response"]["result"],
        "FunctionHoldersays isEven: true"
    );
}

fn weather_declaration(parameters_key: &str) -> Tool {
    let parameters = if parameters_key == "parameters_json_schema" {
        json!({"type": "object", "properties": {
            "location": {"type": "string", "description": "The location to get the weather for"},
            "unit": {"type": "string", "enum": ["C", "F"]}
        }})
    } else {
        json!({"type": "OBJECT", "properties": {
            "location": {"type": "STRING", "description": "The location to get the weather for"},
            "unit": {"type": "STRING", "enum": ["C", "F"]}
        }})
    };
    serde_json::from_value(json!({"function_declarations": [{
        "name": "get_current_weather",
        "description": "Get the current weather in a city",
        parameters_key: parameters
    }]}))
    .unwrap()
}

async fn callable_with_declaration(tag: &str, parameters_key: &str, stream: bool) {
    // A callable and a raw function declaration side by side: Python merges
    // both into one tool and AFC stays out of the way when the model calls a
    // function it cannot run itself; here the model answers in text.
    let name = format!("divide_integers_mixed_{tag}");
    let reply = text_reply("1000 / 2 = 500. London: cloudy.");
    let server = serve(vec![if stream {
        sse_reply(&[reply])
    } else {
        json_reply(&reply)
    }])
    .await;
    let cfg = config(vec![
        divide_tool(&name),
        weather_declaration(parameters_key),
    ]);
    let prompt = "Divide 1000 by 2. And tell me the weather in London.";

    if stream {
        let chunks = collect_stream(&server, prompt, cfg).await;
        assert_eq!(chunks.len(), 1);
    } else {
        let response = generate(&server, prompt, cfg).await.unwrap();
        assert!(response.function_calls().is_empty());
    }

    let sent = bodies(&server).await;
    assert_eq!(sent.len(), 1);
    let tools = sent[0]["tools"].as_array().unwrap();
    let names: Vec<&Value> = tools
        .iter()
        .flat_map(|t| t["functionDeclarations"].as_array().unwrap())
        .map(|d| &d["name"])
        .collect();
    assert_eq!(names, [&json!(name), &json!("get_current_weather")]);
}

// upstream-test: models/test_generate_content_tools.py::test_function_declaration_with_callable
#[tokio::test]
async fn test_function_declaration_with_callable() {
    callable_with_declaration("unary", "parameters_json_schema", false).await;
}

// upstream-test: models/test_generate_content_tools.py::test_function_declaration_with_callable_stream_now
#[tokio::test]
async fn test_function_declaration_with_callable_stream_now() {
    callable_with_declaration("stream", "parameters_json_schema", true).await;
}

// upstream-test: models/test_generate_content_tools.py::test_function_declaration_with_callable_async
#[tokio::test]
async fn test_function_declaration_with_callable_async() {
    callable_with_declaration("async", "parameters_json_schema", false).await;
}

// upstream-test: models/test_generate_content_tools.py::test_function_declaration_with_callable_async_stream
#[tokio::test]
async fn test_function_declaration_with_callable_async_stream() {
    callable_with_declaration("async_stream", "parameters", true).await;
}

// -------------------------------------------------------------- chats

// upstream-test: models/test_generate_content_tools.py::test_tools_chat_curation
#[tokio::test]
async fn test_tools_chat_curation() {
    let server = serve(vec![
        json_reply(&text_reply("Argentina did not win a World Cup in 1955.")),
        json_reply(&text_reply("About 16 million.")),
    ])
    .await;
    let cfg = config(vec![weather_declaration("parameters")]);
    let mut chat = test_client(server.uri())
        .chats()
        .create(MODEL, Some(cfg), None);

    chat.send_message("Who won the 1955 world cup?", None)
        .await
        .unwrap();
    chat.send_message("What was the population of canada in 1955?", None)
        .await
        .unwrap();

    let history: &[Content] = chat.get_history(true);
    assert_eq!(history.len(), 4);
    let sent = bodies(&server).await;
    assert_eq!(
        sent[1]["tools"][0]["functionDeclarations"][0]["name"],
        "get_current_weather"
    );
}
