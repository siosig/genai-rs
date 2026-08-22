//! Integration tests for `gemini_genai::mcp::mcp_tools`, the bridge from
//! an MCP server's tools to this crate's `FunctionTool`/`Tool`.
//!
//! Uses a real, in-process `rmcp` client/server pair connected over a
//! `tokio::io::duplex` byte-stream transport (no network, no external
//! process) -- `rmcp`'s own doc examples use exactly this transport shape
//! for a TCP/stdio connection, and `(DuplexStream, DuplexStream)` (a
//! single stream implementing both `AsyncRead` and `AsyncWrite`)
//! satisfies `IntoTransport` the same way. This exercises the real
//! `list_tools`/`call_tool` wire protocol end to end, not just the
//! `inputSchema -> parameters_json_schema` mapping in isolation.
#![cfg(feature = "mcp")]

mod common;

use common::test_client;
use gemini_genai::{
    afc::function_tool,
    mcp::mcp_tools,
    types::{GenerateContentConfig, Tool},
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
use schemars::JsonSchema;
use serde::Deserialize;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

fn model_reply(text: &str) -> serde_json::Value {
    serde_json::json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{"text": text}]},
            "finishReason": "STOP"
        }]
    })
}

fn function_call_reply(name: &str, args: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{"functionCall": {"name": name, "args": args}}]},
            "finishReason": "STOP"
        }]
    })
}

/// A minimal in-process MCP server exposing one tool, `get_weather(location:
/// string) -> string`. Every method besides `list_tools`/`call_tool` keeps
/// `ServerHandler`'s default (`ServerInfo::default()`, `method_not_found`,
/// ...).
struct WeatherServer;

impl ServerHandler for WeatherServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::default()
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"location": {"type": "string"}},
            "required": ["location"],
        });
        let serde_json::Value::Object(schema) = schema else {
            unreachable!("literal object above")
        };
        let tool = McpTool::new("get_weather", "Gets the weather for a location.", schema);
        Ok(ListToolsResult::with_all_items(vec![tool]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if request.name != "get_weather" {
            return Err(McpError::method_not_found::<
                rmcp::model::CallToolRequestMethod,
            >());
        }
        let location = request
            .arguments
            .as_ref()
            .and_then(|arguments| arguments.get("location"))
            .and_then(|value| value.as_str())
            .unwrap_or("an unknown location");
        Ok(
            CallToolResult::success(vec![ContentBlock::text(format!("sunny in {location}"))])
                .into(),
        )
    }
}

/// A server whose one tool always reports a tool-level error, to exercise
/// `McpFunctionTool::call`'s `is_error` handling.
struct FailingServer;

impl ServerHandler for FailingServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::default()
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let serde_json::Value::Object(schema) =
            serde_json::json!({"type": "object", "properties": {}})
        else {
            unreachable!("literal object above")
        };
        let tool = McpTool::new("always_fails", "Always fails.", schema);
        Ok(ListToolsResult::with_all_items(vec![tool]))
    }

    async fn call_tool(
        &self,
        _request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        Ok(
            CallToolResult::error(vec![ContentBlock::text("downstream service unavailable")])
                .into(),
        )
    }
}

/// Connects `handler` as an in-process MCP server over a
/// `tokio::io::duplex` transport and returns the connected client
/// [`rmcp::service::Peer`].
///
/// The client-side `RunningService` is intentionally leaked: dropping it
/// cancels the connection (see `RunningService`'s `Drop` impl), and this
/// helper's only job is to hand back a `Peer` that stays usable for the
/// rest of a short-lived test process.
#[expect(
    clippy::unwrap_used,
    reason = "test helper: a failed in-process MCP handshake here is a test-setup bug, not a runtime condition"
)]
async fn connect<H: ServerHandler>(handler: H) -> rmcp::service::Peer<rmcp::RoleClient> {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let server = handler.serve(server_io).await.unwrap();
        server.waiting().await.unwrap();
    });
    let client = ().serve(client_io).await.unwrap();
    let peer = client.peer().clone();
    Box::leak(Box::new(client));
    peer
}

#[tokio::test]
async fn mcp_tools_lists_and_declares_the_server_s_tools() {
    let peer = connect(WeatherServer).await;

    let tools = mcp_tools(&peer).await.unwrap_or_else(|error| {
        panic!("mcp_tools failed: {error}");
    });
    assert_eq!(tools.len(), 1);
    let declarations = tools[0].function_declarations.as_ref().unwrap();
    assert_eq!(declarations.len(), 1);
    let declaration = &declarations[0];
    assert_eq!(declaration.name.as_deref(), Some("get_weather"));
    assert_eq!(
        declaration.description.as_deref(),
        Some("Gets the weather for a location.")
    );
    let schema = declaration.parameters_json_schema.as_ref().unwrap();
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["location"]["type"], "string");
}

#[tokio::test]
async fn afc_loop_invokes_an_mcp_tool_through_the_in_process_server() {
    let peer = connect(WeatherServer).await;
    let tools = mcp_tools(&peer).await.unwrap();

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(function_call_reply(
                "get_weather",
                &serde_json::json!({"location": "Tokyo"}),
            )),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("It's sunny in Tokyo.")))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;

    let config = GenerateContentConfig {
        tools: Some(tools),
        ..Default::default()
    };
    let response = test_client(server.uri())
        .models()
        .generate_content(
            "gemini-2.5-flash",
            "What's the weather in Tokyo?",
            Some(config),
        )
        .await
        .unwrap();

    assert_eq!(response.text().as_deref(), Some("It's sunny in Tokyo."));
    assert_eq!(
        response
            .automatic_function_calling_history
            .as_ref()
            .unwrap()
            .len(),
        2
    );
    server.verify().await;
}

#[tokio::test]
async fn an_mcp_tool_level_error_becomes_an_error_function_response_and_the_loop_continues() {
    let peer = connect(FailingServer).await;
    let tools = mcp_tools(&peer).await.unwrap();

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(function_call_reply("always_fails", &serde_json::json!({}))),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("noted.")))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;

    let config = GenerateContentConfig {
        tools: Some(tools),
        ..Default::default()
    };
    let response = test_client(server.uri())
        .models()
        .generate_content("gemini-2.5-flash", "Try the flaky tool.", Some(config))
        .await
        .unwrap();

    assert_eq!(response.text().as_deref(), Some("noted."));
    let history = response.automatic_function_calling_history.unwrap();
    let response_part = &history[1].parts.as_ref().unwrap()[0];
    let error_field = response_part
        .function_response
        .as_ref()
        .unwrap()
        .response
        .as_ref()
        .unwrap()
        .get("error")
        .unwrap();
    assert!(
        error_field
            .as_str()
            .unwrap()
            .contains("downstream service unavailable")
    );
    server.verify().await;
}

/// Confirms a plain Rust [`function_tool`] and an MCP tool can be
/// registered side by side without interfering with each other's
/// callable registration (both funnel through the same process-wide
/// registry keyed by function name; see `gemini_genai::afc`'s docs).
#[tokio::test]
async fn a_native_function_tool_and_an_mcp_tool_coexist_in_one_request() {
    #[derive(Debug, Deserialize, JsonSchema)]
    struct NoArgs {}

    let peer = connect(WeatherServer).await;
    let mut tools = mcp_tools(&peer).await.unwrap();
    let native_tool = function_tool::<NoArgs, _, _, _>(
        "ping",
        "Always replies pong.",
        |_args: NoArgs| async move { Ok(serde_json::json!("pong")) },
    );
    tools.push(Tool::from_function(native_tool));
    assert_eq!(tools.len(), 2);

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(function_call_reply("ping", &serde_json::json!({}))),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("pong received.")))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;

    let config = GenerateContentConfig {
        tools: Some(tools),
        ..Default::default()
    };
    let response = test_client(server.uri())
        .models()
        .generate_content("gemini-2.5-flash", "Ping the native tool.", Some(config))
        .await
        .unwrap();

    assert_eq!(response.text().as_deref(), Some("pong received."));
    server.verify().await;
}
