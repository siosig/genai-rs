//! Port of `google/genai/tests/models/test_generate_content_mcp.py`.
//!
//! The MCP server is an in-process `rmcp` server over a `tokio::io::duplex`
//! transport (as in `tests/mcp/main.rs`). Python passes raw `mcp.types.Tool`
//! objects (declaration only) or a `ClientSession`; here every MCP tool goes
//! through [`gemini_genai::mcp_utils::mcp_tools`], and the "declaration only"
//! Python cases disable automatic function calling so the model's
//! `functionCall` is handed back to the caller.

use gemini_genai::{
    Error,
    mcp_utils::mcp_tools,
    types::{
        AutomaticFunctionCallingConfig, FunctionCall, GenerateContentConfig, HttpOptions, Tool,
    },
};
use rmcp::{
    ErrorData as McpError, ServiceExt,
    handler::server::ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
        PaginatedRequestParams, ServerInfo, Tool as McpTool,
    },
    service::{RequestContext, RoleServer},
};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method},
};

use crate::common::test_client;

const MODEL: &str = "gemini-2.5-flash";

/// Exposes one tool per entry of `tool_names` (duplicates allowed); `get_weather`
/// answers "Sunny", every other tool answers "100".
struct ToolsServer {
    tool_names: Vec<&'static str>,
}

impl ServerHandler for ToolsServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::default()
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = self
            .tool_names
            .iter()
            .map(|name| {
                let schema = match json!({"type": "object", "properties": {"location": {"type": "string"}}}) {
                    Value::Object(map) => map,
                    _ => serde_json::Map::new(),
                };
                McpTool::new(*name, "A test tool.", schema)
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let text = if request.name == "get_weather" {
            "Sunny"
        } else {
            "100"
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]).into())
    }
}

#[expect(
    clippy::unwrap_used,
    reason = "test helper: a failed in-process MCP handshake is a test-setup bug"
)]
async fn connect(tool_names: Vec<&'static str>) -> rmcp::service::Peer<rmcp::RoleClient> {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let server = ToolsServer { tool_names }.serve(server_io).await.unwrap();
        server.waiting().await.unwrap();
    });
    let client = ().serve(client_io).await.unwrap();
    let peer = client.peer().clone();
    // Dropping the running service would cancel the connection.
    Box::leak(Box::new(client));
    peer
}

fn function_call_reply(name: &str, args: &Value) -> Value {
    json!({"candidates": [{"content": {"role": "model", "parts": [{"functionCall": {"name": name, "args": args}}]}, "finishReason": "STOP"}]})
}

fn text_reply(text: &str) -> Value {
    json!({"candidates": [{"content": {"role": "model", "parts": [{"text": text}]}, "finishReason": "STOP"}]})
}

fn afc_disabled(tools: Vec<Tool>) -> GenerateContentConfig {
    GenerateContentConfig {
        tools: Some(tools),
        automatic_function_calling: Some(AutomaticFunctionCallingConfig {
            disable: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn boston_call() -> FunctionCall {
    FunctionCall {
        name: Some("get_weather".to_owned()),
        args: serde_json::from_value(json!({"location": "Boston"})).ok(),
        ..Default::default()
    }
}

async fn mount_weather_call(server: &MockServer) {
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(function_call_reply(
                "get_weather",
                &json!({"location": "Boston"}),
            )),
        )
        .expect(1)
        .mount(server)
        .await;
}

fn assert_declares_get_weather(body: &Value) {
    let declaration = &body["tools"][0]["functionDeclarations"][0];
    assert_eq!(declaration["name"], "get_weather");
    assert_eq!(
        declaration["parameters_json_schema"]["properties"]["location"]["type"],
        "string"
    );
}

// upstream-test: models/test_generate_content_mcp.py::test_mcp_tools_async
#[tokio::test]
async fn test_mcp_tools_async() {
    let tools = mcp_tools(&connect(vec!["get_weather"]).await)
        .await
        .unwrap();
    let server = MockServer::start().await;
    mount_weather_call(&server).await;

    let response = test_client(server.uri())
        .models()
        .generate_content(
            MODEL,
            "What is the weather in Boston?",
            Some(afc_disabled(tools)),
        )
        .await
        .unwrap();

    assert_eq!(response.function_calls(), vec![&boston_call()]);
    let requests = server.received_requests().await.unwrap();
    assert_declares_get_weather(&requests[0].body_json().unwrap());
}

// upstream-test: models/test_generate_content_mcp.py::test_mcp_tools_with_custom_headers_async
#[tokio::test]
async fn test_mcp_tools_with_custom_headers_async() {
    let tools = mcp_tools(&connect(vec!["get_weather"]).await)
        .await
        .unwrap();
    let server = MockServer::start().await;
    let client_header = "google-genai-sdk/1.0.0 gl-python/1.0.0";
    Mock::given(method("POST"))
        .and(header("x-goog-api-client", client_header))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(function_call_reply(
                "get_weather",
                &json!({"location": "Boston"}),
            )),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut config = afc_disabled(tools);
    config.http_options = Some(HttpOptions {
        headers: Some([("x-goog-api-client".to_owned(), client_header.to_owned())].into()),
        ..Default::default()
    });
    let untouched = config.clone();
    let response = test_client(server.uri())
        .models()
        .generate_content(
            MODEL,
            "What is the weather in Boston?",
            Some(config.clone()),
        )
        .await
        .unwrap();

    assert_eq!(response.function_calls(), vec![&boston_call()]);
    // The caller's config is not modified by the call.
    assert_eq!(config, untouched);
    server.verify().await;
}

// upstream-test: models/test_generate_content_mcp.py::test_mcp_tools_subsequent_calls_async
#[tokio::test]
async fn test_mcp_tools_subsequent_calls_async() {
    let tools = mcp_tools(&connect(vec!["get_weather", "add_numbers"]).await)
        .await
        .unwrap();
    let server = MockServer::start().await;
    let replies = [
        function_call_reply("get_weather", &json!({"location": "Boston"})),
        text_reply("It is Sunny in Boston."),
        function_call_reply("add_numbers", &json!({})),
        text_reply("50 + 50 is 100"),
    ];
    for reply in replies {
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(reply))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
    }
    let client = test_client(server.uri());
    let config = GenerateContentConfig {
        tools: Some(tools),
        ..Default::default()
    };

    let first = client
        .models()
        .generate_content(
            MODEL,
            "What is the weather in Boston?",
            Some(config.clone()),
        )
        .await
        .unwrap();
    assert!(
        first
            .text()
            .unwrap_or_default()
            .to_lowercase()
            .contains("sunny")
    );
    let second = client
        .models()
        .generate_content(MODEL, "What is 50 + 50?", Some(config))
        .await
        .unwrap();
    assert!(second.text().unwrap_or_default().contains("100"));

    // The second call's follow-up round carried the tool result ("100").
    let requests = server.received_requests().await.unwrap();
    let last: Value = requests[3].body_json().unwrap();
    assert_eq!(
        last["contents"][2]["parts"][0]["functionResponse"]["name"],
        "add_numbers"
    );
    server.verify().await;
}

// upstream-test: models/test_generate_content_mcp.py::test_mcp_tools_duplicate_tool_name_raises_error
#[tokio::test]
async fn test_mcp_tools_duplicate_tool_name_raises_error() {
    let peer = connect(vec!["get_weather", "get_weather"]).await;
    let result = mcp_tools(&peer).await;
    assert!(matches!(result, Err(Error::Validation(_))), "{result:?}");
}

/// Runs `f` on a plain OS thread with no Tokio runtime context, as the blocking
/// client refuses to run inside one.
#[expect(
    clippy::unwrap_used,
    reason = "test helper: a panicking worker thread is a test failure"
)]
fn run_off_runtime<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(f).join().unwrap()
}

// upstream-test: models/test_generate_content_mcp.py::test_mcp_tools_synchronous_call
#[tokio::test]
async fn test_mcp_tools_synchronous_call() {
    let tools = mcp_tools(&connect(vec!["get_weather"]).await)
        .await
        .unwrap();
    let server = MockServer::start().await;
    mount_weather_call(&server).await;
    let base_url = server.uri();

    let calls = run_off_runtime(move || {
        crate::common::blocking_test_client(base_url)
            .models()
            .generate_content(
                MODEL,
                "What is the weather in Boston?",
                Some(afc_disabled(tools)),
            )
            .unwrap()
            .function_calls()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
    });

    assert_eq!(calls, vec![boston_call()]);
    server.verify().await;
}

// upstream-test: models/test_generate_content_mcp.py::test_mcp_tools_synchronous_stream_call
#[tokio::test]
async fn test_mcp_tools_synchronous_stream_call() {
    let tools = mcp_tools(&connect(vec!["get_weather"]).await)
        .await
        .unwrap();
    let server = MockServer::start().await;
    let sse = format!(
        "data: {}\n\n",
        function_call_reply("get_weather", &json!({"location": "Boston"}))
    );
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse)
                .insert_header("content-type", "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let base_url = server.uri();

    let chunk_calls = run_off_runtime(move || {
        crate::common::blocking_test_client(base_url)
            .models()
            .generate_content_stream(
                MODEL,
                "What is the weather in Boston?",
                Some(afc_disabled(tools)),
            )
            .unwrap()
            .map(|chunk| {
                chunk
                    .unwrap()
                    .function_calls()
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    });

    assert_eq!(chunk_calls, vec![vec![boston_call()]]);
    server.verify().await;
}
