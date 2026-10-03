//! Port of `transformers/test_t_tools.py`. The callable and MCP-tool cases
//! (`test_function`, `test_mcp_tool`, `test_multiple_tools`) are excluded.

use gemini_genai::__test_support::transformers as t;
use serde_json::json;

// upstream-test: transformers/test_t_tools.py::test_empty
#[test]
fn test_empty() {
    assert_eq!(t::t_tools(json!([])).unwrap(), json!([]));
}

// upstream-test: transformers/test_t_tools.py::test_tool
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
    assert_eq!(t::t_tools(json!([tool.clone()])).unwrap(), json!([tool]));
}
