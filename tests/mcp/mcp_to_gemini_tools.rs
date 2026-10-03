//! Ports of upstream `mcp/test_mcp_to_gemini_tools.py`.
//!
//! Python converts each MCP `inputSchema` into a `types.Schema`
//! (upper-casing `type`, dropping unknown keys) unless `is_agent_platform`
//! is set, in which case the schema is forwarded intact as
//! `parameters_json_schema`. This crate always forwards the `inputSchema`
//! verbatim as `parameters_json_schema` (see `src/mcp_utils.rs`,
//! `McpFunctionTool::declaration`), so every ported case asserts the
//! pass-through contract that the upstream Agent Platform tests assert.

use gemini_genai::{mcp_utils::mcp_tools, types::Tool};
use rmcp::{
    ErrorData as McpError,
    handler::server::ServerHandler,
    model::{ListToolsResult, PaginatedRequestParams, ServerInfo, Tool as McpTool},
    service::{RequestContext, RoleServer},
};
use serde_json::{Value, json};

use super::connect;

/// An MCP server whose `tools/list` returns the given `(name, description,
/// inputSchema)` triples and nothing else.
pub(crate) struct StaticToolsServer(pub(crate) Vec<(String, String, Value)>);

impl ServerHandler for StaticToolsServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::default()
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = self
            .0
            .iter()
            .filter_map(|(name, description, schema)| {
                schema
                    .as_object()
                    .map(|schema| McpTool::new(name.clone(), description.clone(), schema.clone()))
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }
}

pub(crate) async fn tools_for(tools: Vec<(String, String, Value)>) -> Vec<Tool> {
    let peer = connect(StaticToolsServer(tools)).await;
    mcp_tools(&peer)
        .await
        .unwrap_or_else(|error| panic!("mcp_tools failed: {error}"))
}

/// Registers one tool named `tool` with `schema` and returns its declared
/// `parameters_json_schema`.
async fn declared_schema(schema: &Value) -> Value {
    let tools = tools_for(vec![(
        "tool".to_owned(),
        "tool-description".to_owned(),
        schema.clone(),
    )])
    .await;
    assert_eq!(tools.len(), 1);
    let declarations = tools[0]
        .function_declarations
        .as_deref()
        .unwrap_or_default();
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].name.as_deref(), Some("tool"));
    assert_eq!(
        declarations[0].description.as_deref(),
        Some("tool-description")
    );
    declarations[0]
        .parameters_json_schema
        .clone()
        .unwrap_or(Value::Null)
}

// upstream-test: mcp/test_mcp_to_gemini_tools.py::test_empty_mcp_tools_list
#[tokio::test]
async fn test_empty_mcp_tools_list() {
    assert!(tools_for(Vec::new()).await.is_empty());
}

// upstream-test: mcp/test_mcp_to_gemini_tools.py::test_unknown_field_conversion
#[tokio::test]
async fn test_unknown_field_conversion() {
    let schema = json!({
        "type": "object",
        "properties": {},
        "unknown_field": "unknownField",
        "unknown_object": {},
    });
    assert_eq!(declared_schema(&schema).await, schema);
}

// upstream-test: mcp/test_mcp_to_gemini_tools.py::test_items_conversion
#[tokio::test]
async fn test_items_conversion() {
    let schema = json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {"key1": {"type": "string"}, "key2": {"type": "number"}},
        },
    });
    assert_eq!(declared_schema(&schema).await, schema);
}

// upstream-test: mcp/test_mcp_to_gemini_tools.py::test_any_of_conversion
#[tokio::test]
async fn test_any_of_conversion() {
    let schema = json!({
        "type": "object",
        "any_of": [{"type": "string"}, {"type": "number"}],
    });
    assert_eq!(declared_schema(&schema).await, schema);
}

// upstream-test: mcp/test_mcp_to_gemini_tools.py::test_properties_conversion
#[tokio::test]
async fn test_properties_conversion() {
    let schema = json!({
        "type": "object",
        "properties": {"key1": {"type": "string"}, "key2": {"type": "number"}},
    });
    assert_eq!(declared_schema(&schema).await, schema);
}

// upstream-test: mcp/test_mcp_to_gemini_tools.py::test_defs_conversion
#[tokio::test]
async fn test_defs_conversion() {
    let schema = json!({
        "type": "object",
        "properties": {"machine_spec": {"$ref": "#/$defs/MachineSpec"}},
        "$defs": {"MachineSpec": {
            "type": "object",
            "properties": {"machine_type": {"type": "string"}},
        }},
    });
    let declared = declared_schema(&schema).await;
    assert!(declared.get("$defs").is_some(), "$defs dropped: {declared}");
    assert!(
        declared["$defs"].get("MachineSpec").is_some(),
        "MachineSpec dropped: {declared}"
    );
}

// upstream-test: mcp/test_mcp_to_gemini_tools.py::test_create_endpoint_one_of_conversion
#[tokio::test]
async fn test_create_endpoint_one_of_conversion() {
    let schema = json!({
        "type": "object",
        "properties": {"endpoint": {
            "type": "object",
            "oneOf": [
                {"title": "dedicated_resources", "type": "object"},
                {"title": "automatic_resources", "type": "object"},
            ],
        }},
    });
    let declared = declared_schema(&schema).await;
    let one_of = declared["properties"]["endpoint"]["oneOf"]
        .as_array()
        .unwrap_or_else(|| panic!("oneOf dropped: {declared}"));
    assert_eq!(one_of.len(), 2);
}

// upstream-test: mcp/test_mcp_to_gemini_tools.py::test_update_endpoint_labels_conversion
#[tokio::test]
async fn test_update_endpoint_labels_conversion() {
    let schema = json!({
        "type": "object",
        "properties": {"endpoint": {
            "type": "object",
            "properties": {"labels": {
                "type": "object",
                "additionalProperties": {"type": "string"},
            }},
        }},
    });
    let declared = declared_schema(&schema).await;
    let labels = &declared["properties"]["endpoint"]["properties"]["labels"];
    assert!(
        labels.get("additionalProperties").is_some(),
        "additionalProperties dropped: {declared}"
    );
}

// upstream-test: mcp/test_mcp_to_gemini_tools.py::test_agent_platform_preserves_unknown_fields
#[tokio::test]
async fn test_agent_platform_preserves_unknown_fields() {
    let schema = json!({
        "type": "object",
        "properties": {},
        "some_new_future_field": "value",
    });
    let declared = declared_schema(&schema).await;
    assert_eq!(declared["some_new_future_field"], "value");
}
