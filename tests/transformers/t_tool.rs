//! Port of `transformers/test_t_tool.py`. `test_function` (a Python
//! callable) and `test_mcp_tool` (an `mcp.types.Tool` instance) are
//! excluded.

use gemini_genai::__test_support::transformers as t;
use serde_json::{Value, json};

// upstream-test: transformers/test_t_tool.py::test_none
#[test]
fn test_none() {
    assert_eq!(t::t_tool(Value::Null).unwrap(), Value::Null);
}

// upstream-test: transformers/test_t_tool.py::test_dictionary
#[test]
fn test_dictionary() {
    let tool = json!({
        "function_declarations": [{
            "name": "tool",
            "description": "tool-description",
            "parameters": {"type": "OBJECT", "properties": {}},
        }]
    });
    assert_eq!(t::t_tool(tool.clone()).unwrap(), tool);
}

// upstream-test: transformers/test_t_tool.py::test_tool
#[test]
fn test_tool() {
    let tool = json!({
        "function_declarations": [{
            "name": "tool",
            "description": "tool-description",
            "parameters": {
                "type": "OBJECT",
                "properties": {"key1": {"type": "STRING"}, "key2": {"type": "NUMBER"}},
            },
        }]
    });
    assert_eq!(t::t_tool(tool.clone()).unwrap(), tool);
}
