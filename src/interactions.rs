//! Interactions (`client.interactions()`): create (optionally streaming), get, cancel and delete interactions. Mirrors Python's `interactions.py`.
//!
//! The items are generated into `crate::gaos` (see `tools/codegen/gen_gaos.py`) and
//! re-exported here under the names the upstream module exports.

pub use crate::gaos::exports::interactions::*;

use serde_json::{Map, Value, json};

use crate::{errors::Result, gaos::CREATE_BODY_KEYS};

/// Validation failure of a request body (kept out of scope as `Error` is this module's re-exported model).
fn invalid(message: String) -> crate::errors::Error {
    crate::errors::Error::Validation(message)
}

/// `type` values that mark an input item as an interaction step rather than a content block.
const STEP_TYPES: &[&str] = &[
    "user_input",
    "model_output",
    "thought",
    "function_call",
    "code_execution_call",
    "url_context_call",
    "mcp_server_tool_call",
    "google_search_call",
    "file_search_call",
    "google_maps_call",
    "function_result",
    "code_execution_result",
    "url_context_result",
    "google_search_result",
    "mcp_server_tool_result",
    "file_search_result",
    "google_maps_result",
];

/// Normalizes an `Interactions.create` request body the way the Python SDK
/// does before sending (`_normalize_create_body`).
///
/// A non-empty `input` list made only of content blocks (objects that are not
/// steps and carry neither `role` nor `content`) is wrapped in one
/// `user_input` step, and a content block that has `text` but no `type`
/// gets `"type": "text"`. Any other `input` is left unchanged. Typed
/// requests already pass through this on [`Interactions::create`]; call it
/// directly to accept the shorthand forms from untyped JSON before decoding
/// them into a typed body.
///
/// # Errors
/// Returns [`crate::Error::Validation`] if `body` is not a JSON object or has a
/// top-level key that is not part of a create request (Python raises
/// `TypeError`; put extra fields in `HttpOptions::extra_body`).
///
/// # Examples
/// ```
/// use serde_json::json;
///
/// let body = gemini_genai::interactions::normalize_create_body(json!({
///     "model": "gemini-2.5-flash",
///     "input": [{"text": "Hello"}],
/// }))?;
/// assert_eq!(
///     body["input"],
///     json!([{"type": "user_input", "content": [{"type": "text", "text": "Hello"}]}])
/// );
/// # Ok::<(), gemini_genai::Error>(())
/// ```
pub fn normalize_create_body(body: Value) -> Result<Value> {
    let Value::Object(mut body) = body else {
        return Err(invalid("create() body must be a JSON object.".to_owned()));
    };
    let mut unknown: Vec<&str> = body
        .keys()
        .map(String::as_str)
        .filter(|key| !CREATE_BODY_KEYS.contains(key))
        .collect();
    if !unknown.is_empty() {
        unknown.sort_unstable();
        return Err(invalid(format!(
            "create() got unexpected keyword argument(s): {}. Use HttpOptions::extra_body to send additional request body fields.",
            unknown.join(", ")
        )));
    }
    if let Some(Value::Array(items)) = body.get("input")
        && is_content_list(items)
    {
        let content: Vec<Value> = items.iter().map(normalize_content_block).collect();
        body.insert(
            "input".to_owned(),
            json!([{ "type": "user_input", "content": content }]),
        );
    }
    Ok(Value::Object(body))
}

fn normalize_content_block(block: &Value) -> Value {
    let mut block = block.clone();
    if let Value::Object(map) = &mut block
        && map.contains_key("text")
        && !map.contains_key("type")
    {
        map.insert("type".to_owned(), Value::String("text".to_owned()));
    }
    block
}

fn is_content_list(items: &[Value]) -> bool {
    !items.is_empty() && items.iter().all(is_content_block)
}

fn is_content_block(value: &Value) -> bool {
    value.as_object().is_some_and(|map| {
        !is_step_block(map) && !map.contains_key("role") && !map.contains_key("content")
    })
}

fn is_step_block(map: &Map<String, Value>) -> bool {
    map.get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| STEP_TYPES.contains(&kind))
}
