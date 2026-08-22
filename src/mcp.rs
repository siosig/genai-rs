//! MCP tool bridging: wraps every tool exposed by an MCP server (reached
//! through an `rmcp` client [`Peer`]) as a [`crate::afc::FunctionTool`],
//! so it can be registered with [`crate::types::Tool::from_function`] and
//! driven by the automatic function calling loop in [`crate::afc`], the
//! same as a plain Rust callable built with
//! [`function_tool`](crate::afc::function_tool).
//!
//! Mirrors Python's `_mcp_utils.py` `McpToGenAiToolAdapter`.

use std::sync::Arc;

use futures_util::future::BoxFuture;
use rmcp::{
    model::{CallToolRequestParams, CallToolResponse, ContentBlock},
    service::{Peer, RoleClient},
};
use serde_json::Value;

use crate::{
    afc::FunctionTool,
    error::{Error, FunctionCallError, Result},
    types::{FunctionDeclaration, Tool},
};

/// Bridges one tool exposed by an MCP server to a [`FunctionTool`],
/// delegating [`FunctionTool::call`] to a `tools/call` request over
/// `peer`. Mirrors Python's `McpToGenAiToolAdapter.call_tool`.
struct McpFunctionTool {
    peer: Peer<RoleClient>,
    name: String,
    description: Option<String>,
    input_schema: Value,
}

impl FunctionTool for McpFunctionTool {
    fn declaration(&self) -> FunctionDeclaration {
        FunctionDeclaration {
            name: Some(self.name.clone()),
            description: self.description.clone(),
            parameters_json_schema: Some(self.input_schema.clone()),
            ..Default::default()
        }
    }

    fn call(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let arguments = match args {
                Value::Object(map) => Some(map),
                Value::Null => None,
                other => {
                    let mut map = serde_json::Map::with_capacity(1);
                    map.insert("value".to_owned(), other);
                    Some(map)
                }
            };
            let mut params = CallToolRequestParams::new(self.name.clone());
            if let Some(arguments) = arguments {
                params = params.with_arguments(arguments);
            }
            let invocation_error = |message: String| {
                Error::FunctionCall(FunctionCallError::Invocation {
                    function: self.name.clone(),
                    message,
                })
            };
            let response = self
                .peer
                .call_tool_once(params)
                .await
                .map_err(|error| invocation_error(error.to_string()))?;
            let result = match response {
                CallToolResponse::Complete(result) => result,
                other => {
                    return Err(invocation_error(format!(
                        "MCP server returned an unsupported tools/call response \
                         (expected a complete result): {other:?}"
                    )));
                }
            };
            let content = content_blocks_to_value(&result.content);
            if result.is_error.unwrap_or(false) {
                return Err(invocation_error(content.to_string()));
            }
            Ok(result.structured_content.unwrap_or(content))
        })
    }
}

/// Renders MCP tool-call output content as a JSON value for a
/// `FunctionResponse`: a lone text block becomes a plain string, no
/// blocks become `null`, and anything else (images, resources, multiple
/// blocks, ...) is serialized structurally.
fn content_blocks_to_value(blocks: &[ContentBlock]) -> Value {
    let mut values: Vec<Value> = blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text(text) => Value::String(text.text.clone()),
            other => serde_json::to_value(other).unwrap_or(Value::Null),
        })
        .collect();
    match values.len() {
        0 => Value::Null,
        1 => values.remove(0),
        _ => Value::Array(values),
    }
}

/// Lists every tool exposed by the MCP server reached through `peer`, and
/// wraps each as a [`Tool`] registered for automatic function calling
/// (see [`Tool::from_function`]). Mirrors Python's
/// `mcp_to_genai_tool_adapters` construction in `Models.generate_content`.
///
/// # Errors
/// [`Error::FunctionCall`] (wrapping [`FunctionCallError::Invocation`]) if
/// the `tools/list` request fails.
pub async fn mcp_tools(peer: &Peer<RoleClient>) -> Result<Vec<Tool>> {
    let tools = peer.list_all_tools().await.map_err(|error| {
        Error::FunctionCall(FunctionCallError::Invocation {
            function: "tools/list".to_owned(),
            message: error.to_string(),
        })
    })?;
    Ok(tools
        .into_iter()
        .map(|tool| {
            let callable: Arc<dyn FunctionTool> = Arc::new(McpFunctionTool {
                peer: peer.clone(),
                name: tool.name.into_owned(),
                description: tool.description.map(std::borrow::Cow::into_owned),
                input_schema: Value::Object((*tool.input_schema).clone()),
            });
            Tool::from_function(callable)
        })
        .collect())
}
