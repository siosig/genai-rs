//! Ports of `google/genai/tests/afc/test_get_function_response_parts.py`.

use std::collections::HashMap;

use gemini_genai::{
    __test_support::extra_utils::{get_function_map, get_function_response_parts},
    function_tool,
    types::{
        Candidate, Content, FunctionCall, GenerateContentConfig, GenerateContentResponse, Part,
        Tool,
    },
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize, JsonSchema)]
struct IntArg {
    a: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FloatArg {
    a: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StringArg {
    a: String,
}

fn response_calling(name: &str, args: Value) -> GenerateContentResponse {
    let args: HashMap<String, Value> = serde_json::from_value(args).expect("args is an object");
    GenerateContentResponse {
        candidates: Some(vec![Candidate {
            content: Some(Content {
                parts: Some(vec![Part {
                    function_call: Some(FunctionCall {
                        name: Some(name.to_owned()),
                        args: Some(args),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        }]),
        ..Default::default()
    }
}

fn expected_part(name: &str, result: Value) -> Part {
    Part::from_function_response(name, HashMap::from([("result".to_owned(), result)]))
}

/// Runs `response` against the one tool `tool` and compares the answers to
/// `expected` as JSON (upstream compares `model_dump_json(exclude_none=True)`).
async fn assert_parts(tool: Tool, response: &GenerateContentResponse, expected: &[Part]) {
    let config = GenerateContentConfig {
        tools: Some(vec![tool]),
        ..Default::default()
    };
    let function_map = get_function_map(Some(&config));

    let actual = get_function_response_parts(response, &function_map)
        .await
        .expect("the function call is answered");

    assert_eq!(
        serde_json::to_value(&actual).expect("parts serialize"),
        serde_json::to_value(expected).expect("parts serialize")
    );
}

// upstream-test: afc/test_get_function_response_parts.py::test_integer_value
#[tokio::test]
async fn test_integer_value() {
    let tool = Tool::from_function(function_tool::<IntArg, _, _, _>(
        "afc_parts_int_func_under_test",
        "Adds one.",
        |args: IntArg| async move { Ok(args.a + 1) },
    ));
    let response = response_calling("afc_parts_int_func_under_test", json!({"a": 1}));

    assert_parts(
        tool,
        &response,
        &[expected_part("afc_parts_int_func_under_test", json!(2))],
    )
    .await;
}

// upstream-test: afc/test_get_function_response_parts.py::test_float_value
#[tokio::test]
async fn test_float_value() {
    let tool = Tool::from_function(function_tool::<FloatArg, _, _, _>(
        "afc_parts_float_func_under_test",
        "Adds one.",
        |args: FloatArg| async move { Ok(args.a + 1.0) },
    ));
    let response = response_calling("afc_parts_float_func_under_test", json!({"a": 1.0}));

    assert_parts(
        tool,
        &response,
        &[expected_part("afc_parts_float_func_under_test", json!(2.0))],
    )
    .await;
}

// upstream-test: afc/test_get_function_response_parts.py::test_string_value
#[tokio::test]
async fn test_string_value() {
    let tool = Tool::from_function(function_tool::<StringArg, _, _, _>(
        "afc_parts_string_func_under_test",
        "Appends 1.",
        |args: StringArg| async move { Ok(format!("{}1", args.a)) },
    ));
    let response = response_calling("afc_parts_string_func_under_test", json!({"a": "1.0"}));

    assert_parts(
        tool,
        &response,
        &[expected_part(
            "afc_parts_string_func_under_test",
            json!("1.01"),
        )],
    )
    .await;
}
