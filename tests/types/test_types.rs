//! Ported from upstream `tests/types/test_types.py`: the `Part` factory
//! methods, enum parsing and plain (de)serialization of generated types.
//!
//! The Python-only groups of that file (`FunctionDeclaration.from_callable`
//! schema derivation from Python callables, `Schema.from_json_schema`,
//! pydantic `Part(...)` / `ModelContent(...)` constructors, MCP
//! `CallToolResult` objects, PIL images) are recorded as excluded rather
//! than ported.

use std::collections::HashMap;

use gemini_genai::types::{
    ComputerUse, Environment, File, FunctionResponsePart, GenerateContentResponse, HarmCategory,
    Language, Outcome, Part, SafetyPolicy, SafetyRating, Schema, Type,
};
use serde_json::json;

const SCONES_URI: &str = "gs://generativeai-downloads/images/scones.jpg";
const JPEG: &str = "image/jpeg";

fn args(key: &str, value: &str) -> HashMap<String, serde_json::Value> {
    HashMap::from([(key.to_owned(), json!(value))])
}

// upstream-test: types/test_types.py::test_factory_method_from_uri_part
#[test]
fn test_factory_method_from_uri_part() {
    let part = Part::from_uri(SCONES_URI, JPEG);
    let file_data = part.file_data.expect("file_data");
    assert_eq!(file_data.file_uri.as_deref(), Some(SCONES_URI));
    assert_eq!(file_data.mime_type.as_deref(), Some(JPEG));
}

// upstream-test: types/test_types.py::test_factory_method_from_uri_inferred_mime_type_part
#[test]
fn test_factory_method_from_uri_inferred_mime_type_part() {
    let part = Part::from_uri_inferred(SCONES_URI).expect("mime type is inferred from .jpg");
    let file_data = part.file_data.expect("file_data");
    assert_eq!(file_data.file_uri.as_deref(), Some(SCONES_URI));
    assert_eq!(file_data.mime_type.as_deref(), Some(JPEG));
    // Python raises ValueError when nothing can be inferred.
    assert!(Part::from_uri_inferred("gs://bucket/no-extension").is_err());
}

// upstream-test: types/test_types.py::test_factory_method_from_text_part
#[test]
fn test_factory_method_from_text_part() {
    assert_eq!(
        Part::from_text("What is your name?").text.as_deref(),
        Some("What is your name?")
    );
}

// upstream-test: types/test_types.py::test_factory_method_from_bytes_part
#[test]
fn test_factory_method_from_bytes_part() {
    let blob = Part::from_bytes(b"123".to_vec(), "text/plain")
        .inline_data
        .expect("inline_data");
    assert_eq!(blob.data.as_deref(), Some(b"123".as_slice()));
    assert_eq!(blob.mime_type.as_deref(), Some("text/plain"));
}

// upstream-test: types/test_types.py::test_factory_method_from_function_call_part
#[test]
fn test_factory_method_from_function_call_part() {
    let call = Part::from_function_call("func", args("arg", "value"))
        .function_call
        .expect("function_call");
    assert_eq!(call.name.as_deref(), Some("func"));
    assert_eq!(call.args, Some(args("arg", "value")));
}

// upstream-test: types/test_types.py::test_factory_method_from_function_response_part
#[test]
fn test_factory_method_from_function_response_part() {
    let response = Part::from_function_response("func", args("response", "value"))
        .function_response
        .expect("function_response");
    assert_eq!(response.name.as_deref(), Some("func"));
    assert_eq!(response.response, Some(args("response", "value")));
}

// Python passes `parts=[...]` to `Part.from_function_response`; the Rust
// constructor has no `parts` argument, so the multimodal parts are set on
// the built `FunctionResponse`.
// upstream-test: types/test_types.py::test_factory_method_part_from_function_response_with_multi_modal_parts
#[test]
fn test_factory_method_part_from_function_response_with_multi_modal_parts() {
    let mut part = Part::from_function_response("func", args("response", "value"));
    let response = part.function_response.as_mut().expect("function_response");
    response.parts = Some(vec![FunctionResponsePart::from_bytes(
        b"123".to_vec(),
        "image/png",
    )]);

    let response = part.function_response.expect("function_response");
    assert_eq!(response.name.as_deref(), Some("func"));
    assert_eq!(response.response, Some(args("response", "value")));
    let blob = response.parts.expect("parts")[0]
        .inline_data
        .clone()
        .expect("inline_data");
    assert_eq!(blob.data.as_deref(), Some(b"123".as_slice()));
    assert_eq!(blob.mime_type.as_deref(), Some("image/png"));
}

// upstream-test: types/test_types.py::test_factory_method_function_response_part_from_bytes
#[test]
fn test_factory_method_function_response_part_from_bytes() {
    let blob = FunctionResponsePart::from_bytes(b"123".to_vec(), "image/png")
        .inline_data
        .expect("inline_data");
    assert_eq!(blob.data.as_deref(), Some(b"123".as_slice()));
    assert_eq!(blob.mime_type.as_deref(), Some("image/png"));
}

// upstream-test: types/test_types.py::test_factory_method_function_response_part_from_uri
#[test]
fn test_factory_method_function_response_part_from_uri() {
    let file_data = FunctionResponsePart::from_uri(SCONES_URI, JPEG)
        .file_data
        .expect("file_data");
    assert_eq!(file_data.file_uri.as_deref(), Some(SCONES_URI));
    assert_eq!(file_data.mime_type.as_deref(), Some(JPEG));

    // `mime_type=None` form: inferred from the extension.
    let inferred = FunctionResponsePart::from_uri_inferred(SCONES_URI)
        .expect("inferred")
        .file_data
        .expect("file_data");
    assert_eq!(inferred.mime_type.as_deref(), Some(JPEG));
}

// upstream-test: types/test_types.py::test_factory_method_from_executable_code_part
#[test]
fn test_factory_method_from_executable_code_part() {
    let code = Part::from_executable_code("print(\"hello\")", Language::Python)
        .executable_code
        .expect("executable_code");
    assert_eq!(code.code.as_deref(), Some("print(\"hello\")"));
    assert_eq!(code.language, Some(Language::Python));
}

// upstream-test: types/test_types.py::test_factory_method_from_code_execution_result_part
#[test]
fn test_factory_method_from_code_execution_result_part() {
    let result = Part::from_code_execution_result(Outcome::OutcomeOk, "print(\"hello\")")
        .code_execution_result
        .expect("code_execution_result");
    assert_eq!(result.outcome, Some(Outcome::OutcomeOk));
    assert_eq!(result.output.as_deref(), Some("print(\"hello\")"));
}

// Rust spells `types.Part('hello')` as `Part::from("hello")`.
// upstream-test: types/test_types.py::test_part_constructor_with_string_value
#[test]
fn test_part_constructor_with_string_value() {
    let part = Part::from("hello");
    assert_eq!(part.text.as_deref(), Some("hello"));
    assert_eq!(part.file_data, None);
    assert_eq!(part.inline_data, None);
}

// upstream-test: types/test_types.py::test_part_constructor_with_file_value
#[test]
fn test_part_constructor_with_file_value() {
    let file = File {
        uri: Some("gs://my-bucket/my-file".to_owned()),
        mime_type: Some("text/plain".to_owned()),
        display_name: Some("test file".to_owned()),
        ..Default::default()
    };
    let file_data = Part::from(&file).file_data.expect("file_data");
    assert_eq!(
        file_data.file_uri.as_deref(),
        Some("gs://my-bucket/my-file")
    );
    assert_eq!(file_data.mime_type.as_deref(), Some("text/plain"));
    assert_eq!(file_data.display_name.as_deref(), Some("test file"));
}

// upstream-test: types/test_types.py::test_case_insensitive_enum
#[test]
fn test_case_insensitive_enum() {
    assert_eq!(Type::from("STRING".to_owned()), Type::String);
    assert_eq!(Type::from("string".to_owned()), Type::String);
}

// Python checks this through a pydantic model with a `types.Type` field;
// the Rust equivalent is serde deserialization of a generated struct.
// upstream-test: types/test_types.py::test_case_insensitive_enum_with_pydantic_model
#[test]
fn test_case_insensitive_enum_with_pydantic_model() {
    for value in ["STRING", "string"] {
        let schema: Schema =
            serde_json::from_value(json!({ "type": value })).expect("valid schema");
        assert_eq!(schema.r#type, Some(Type::String), "type = {value}");
    }
}

// Python additionally emits a warning, which has no Rust counterpart.
// upstream-test: types/test_types.py::test_unknown_enum_value
#[test]
fn test_unknown_enum_value() {
    let value = Type::from("float".to_owned());
    assert_eq!(value, Type::Unknown("float".to_owned()));
    assert_eq!(value.as_str(), "float");
}

// upstream-test: types/test_types.py::test_unknown_enum_value_in_nested_dict
#[test]
fn test_unknown_enum_value_in_nested_dict() {
    let rating: SafetyRating =
        serde_json::from_value(json!({ "category": "NEW_CATEGORY" })).expect("valid rating");
    let category = rating.category.expect("category");
    assert_eq!(category, HarmCategory::Unknown("NEW_CATEGORY".to_owned()));
    assert_eq!(category.as_str(), "NEW_CATEGORY");
}

// upstream-test: types/test_types.py::test_instantiate_response_from_batch_json
#[test]
fn test_instantiate_response_from_batch_json() {
    let batch_json = json!({
        "candidates": [{
            "citationMetadata": {
                "citationSources": [{
                    "endIndex": 2009,
                    "startIndex": 1880,
                    "uri": "http://someurl.com",
                }]
            },
            "content": {
                "parts": [{"text": "This recipe makes a moist and delicious banana bread!"}],
                "role": "model",
            },
            "finishReason": "STOP",
        }],
        "modelVersion": "gemini-1.5-flash-002@default",
    })
    .to_string();
    let parsed: GenerateContentResponse = serde_json::from_str(&batch_json).expect("valid json");

    let metadata = parsed.candidates.expect("candidates")[0]
        .citation_metadata
        .clone()
        .expect("citation_metadata");
    let citation = &metadata.citations.expect("citations")[0];
    assert_eq!(citation.uri.as_deref(), Some("http://someurl.com"));
    assert_eq!(citation.start_index, Some(1880));
    assert_eq!(citation.end_index, Some(2009));
}

// upstream-test: types/test_types.py::test_computer_use_types
#[test]
fn test_computer_use_types() {
    let computer_use = ComputerUse {
        environment: Some(Environment::EnvironmentMobile),
        enable_prompt_injection_detection: Some(true),
        disabled_safety_policies: Some(vec![
            SafetyPolicy::FinancialTransactions,
            SafetyPolicy::CommunicationTool,
        ]),
        ..Default::default()
    };
    assert_eq!(
        computer_use.environment,
        Some(Environment::EnvironmentMobile)
    );
    assert_eq!(computer_use.enable_prompt_injection_detection, Some(true));
    let policies = computer_use.disabled_safety_policies.expect("policies");
    assert_eq!(policies.len(), 2);
    assert!(policies.contains(&SafetyPolicy::FinancialTransactions));
}
