//! Ports of `google/genai/tests/afc/test_get_function_map.py`.

use std::sync::Arc;

use gemini_genai::{
    __test_support::extra_utils::get_function_map,
    function_tool,
    types::{GenerateContentConfig, Tool},
};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
struct NoArgs {}

// upstream-test: afc/test_get_function_map.py::test_empty_config
#[test]
fn test_empty_config() {
    // Python's `{}` is falsy, like no config at all.
    assert!(get_function_map(None).is_empty());
}

// upstream-test: afc/test_get_function_map.py::test_empty_tools
#[test]
fn test_empty_tools() {
    let config = GenerateContentConfig {
        top_p: Some(0.5),
        ..Default::default()
    };
    assert!(get_function_map(Some(&config)).is_empty());
}

// upstream-test: afc/test_get_function_map.py::test_valid_function
#[test]
fn test_valid_function() {
    let callable = function_tool::<NoArgs, _, _, _>(
        "afc_map_func_under_test",
        "A function under test.",
        |_args: NoArgs| async move { Ok(serde_json::Value::Null) },
    );
    let config = GenerateContentConfig {
        tools: Some(vec![Tool::from_function(Arc::clone(&callable))]),
        ..Default::default()
    };

    let map = get_function_map(Some(&config));

    assert_eq!(map.len(), 1);
    assert!(Arc::ptr_eq(&map["afc_map_func_under_test"], &callable));
}
