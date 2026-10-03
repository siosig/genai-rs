//! Ported from upstream `tests/types/test_part_type.py`: the response
//! accessors and `Part` media-resolution constructors.
//!
//! Python's `caplog` assertions (warnings for multiple candidates or
//! non-text parts) have no Rust counterpart because this crate does not
//! emit those log records; the value assertions are kept.

#![expect(
    clippy::expect_used,
    reason = "helper functions outside #[test] bodies assert test preconditions; a failure there is a test bug"
)]

use std::collections::HashMap;

use gemini_genai::types::{
    Candidate, CodeExecutionResult, Content, ExecutableCode, FunctionCall, GenerateContentResponse,
    Language, Outcome, Part, PartMediaResolution, PartMediaResolutionLevel,
};
use serde_json::json;

fn text_part(text: &str) -> Part {
    Part::from_text(text)
}

fn candidate(parts: Vec<Part>) -> Candidate {
    Candidate {
        content: Some(Content {
            parts: Some(parts),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn response(candidates: Vec<Candidate>) -> GenerateContentResponse {
    GenerateContentResponse {
        candidates: Some(candidates),
        ..Default::default()
    }
}

fn call(name: &str, key: &str, value: &str) -> FunctionCall {
    FunctionCall {
        name: Some(name.to_owned()),
        args: Some(HashMap::from([(key.to_owned(), json!(value))])),
        ..Default::default()
    }
}

fn call_part(function_call: FunctionCall) -> Part {
    Part {
        function_call: Some(function_call),
        ..Default::default()
    }
}

fn code_part(code: &str) -> Part {
    Part::from_executable_code(code, Language::Python)
}

fn result_part(outcome: Outcome, output: &str) -> Part {
    Part::from_code_execution_result(outcome, output)
}

// upstream-test: types/test_part_type.py::test_candidate_empty_text
#[test]
fn test_candidate_empty_text() {
    assert_eq!(GenerateContentResponse::default().text(), None);
}

// upstream-test: types/test_part_type.py::test_first_candidate_empty_content_text
#[test]
fn test_first_candidate_empty_content_text() {
    assert_eq!(response(vec![]).text(), None);
}

// upstream-test: types/test_part_type.py::test_first_candidate_empty_parts_text
#[test]
fn test_first_candidate_empty_parts_text() {
    assert_eq!(response(vec![Candidate::default()]).text(), None);
}

// upstream-test: types/test_part_type.py::test_content_empty_parts_text
#[test]
fn test_content_empty_parts_text() {
    let candidate = Candidate {
        content: Some(Content::default()),
        ..Default::default()
    };
    assert_eq!(response(vec![candidate]).text(), None);
}

// upstream-test: types/test_part_type.py::test_two_candidates_text
#[test]
fn test_two_candidates_text() {
    let response = response(vec![
        candidate(vec![text_part("Hello1"), text_part("World1")]),
        candidate(vec![text_part("Hello2"), text_part("World2")]),
    ]);
    assert_eq!(response.text().as_deref(), Some("Hello1World1"));
}

// upstream-test: types/test_part_type.py::test_thought_signature_no_warning
#[test]
fn test_thought_signature_no_warning() {
    let signature = Part {
        thought_signature: Some(b"thought".to_vec()),
        ..Default::default()
    };
    let response = response(vec![candidate(vec![text_part("Hello"), signature])]);
    assert_eq!(response.text().as_deref(), Some("Hello"));
}

// upstream-test: types/test_part_type.py::test_thought_signature_no_warning_in_text
#[test]
fn test_thought_signature_no_warning_in_text() {
    let part = Part {
        text: Some("Hello".to_owned()),
        thought_signature: Some(b"thought".to_vec()),
        ..Default::default()
    };
    assert_eq!(
        response(vec![candidate(vec![part])]).text().as_deref(),
        Some("Hello")
    );
}

// upstream-test: types/test_part_type.py::test_1_candidate_text
#[test]
fn test_1_candidate_text() {
    let response = response(vec![candidate(vec![
        text_part("Hello1"),
        text_part("World1"),
    ])]);
    assert_eq!(response.text().as_deref(), Some("Hello1World1"));
}

// upstream-test: types/test_part_type.py::test_all_empty_text_in_parts
#[test]
fn test_all_empty_text_in_parts() {
    let response = response(vec![
        candidate(vec![text_part(""), text_part("")]),
        candidate(vec![text_part("Hello2"), text_part("World2")]),
    ]);
    // Must be an empty string, not None.
    assert_eq!(response.text().as_deref(), Some(""));
}

// upstream-test: types/test_part_type.py::test_one_empty_text_in_parts
#[test]
fn test_one_empty_text_in_parts() {
    let response = response(vec![
        candidate(vec![text_part(""), text_part("World1")]),
        candidate(vec![text_part("Hello2"), text_part("World2")]),
    ]);
    assert_eq!(response.text().as_deref(), Some("World1"));
}

// upstream-test: types/test_part_type.py::test_all_none_text
#[test]
fn test_all_none_text() {
    let response = response(vec![
        candidate(vec![Part::default(), Part::default()]),
        candidate(vec![text_part("Hello2"), text_part("World2")]),
    ]);
    assert_eq!(response.text(), None);
}

// upstream-test: types/test_part_type.py::test_none_empty_text
#[test]
fn test_none_empty_text() {
    let response = response(vec![
        candidate(vec![Part::default(), text_part("")]),
        candidate(vec![text_part("Hello2"), text_part("World2")]),
    ]);
    assert_eq!(response.text().as_deref(), Some(""));
}

// upstream-test: types/test_part_type.py::test_non_text_part_text
#[test]
fn test_non_text_part_text() {
    let response = response(vec![candidate(vec![call_part(FunctionCall::default())])]);
    assert_eq!(response.text(), None);
}

// upstream-test: types/test_part_type.py::test_non_text_part_and_text_part_text
#[test]
fn test_non_text_part_and_text_part_text() {
    let response = response(vec![candidate(vec![
        call_part(FunctionCall::default()),
        text_part("World1"),
    ])]);
    assert_eq!(response.text().as_deref(), Some("World1"));
}

// Deviation shared by the `*_function_calls` tests below: Python returns
// `None` when the first candidate has no function calls; the Rust accessor
// returns an empty `Vec` (an empty list already means "none").

// upstream-test: types/test_part_type.py::test_candidates_none_function_calls
#[test]
fn test_candidates_none_function_calls() {
    assert!(
        GenerateContentResponse::default()
            .function_calls()
            .is_empty()
    );
}

// upstream-test: types/test_part_type.py::test_candidates_empty_function_calls
#[test]
fn test_candidates_empty_function_calls() {
    assert!(response(vec![]).function_calls().is_empty());
}

// upstream-test: types/test_part_type.py::test_content_none_function_calls
#[test]
fn test_content_none_function_calls() {
    assert!(
        response(vec![Candidate::default()])
            .function_calls()
            .is_empty()
    );
}

// upstream-test: types/test_part_type.py::test_parts_none_function_calls
#[test]
fn test_parts_none_function_calls() {
    let candidate = Candidate {
        content: Some(Content::default()),
        ..Default::default()
    };
    assert!(response(vec![candidate]).function_calls().is_empty());
}

// upstream-test: types/test_part_type.py::test_parts_empty_function_calls
#[test]
fn test_parts_empty_function_calls() {
    assert!(
        response(vec![candidate(vec![])])
            .function_calls()
            .is_empty()
    );
}

// upstream-test: types/test_part_type.py::test_multiple_candidates_function_calls
#[test]
fn test_multiple_candidates_function_calls() {
    let response = response(vec![
        candidate(vec![call_part(call("funcCall1", "key1", "value1"))]),
        candidate(vec![call_part(call("funcCall2", "key2", "value2"))]),
    ]);
    // Only the first candidate's calls are returned.
    assert_eq!(
        response.function_calls(),
        vec![&call("funcCall1", "key1", "value1")]
    );
}

// upstream-test: types/test_part_type.py::test_multiple_function_calls
#[test]
fn test_multiple_function_calls() {
    let response = response(vec![candidate(vec![
        call_part(call("funcCall1", "key1", "value1")),
        call_part(call("funcCall2", "key2", "value2")),
    ])]);
    assert_eq!(
        response.function_calls(),
        vec![
            &call("funcCall1", "key1", "value1"),
            &call("funcCall2", "key2", "value2"),
        ]
    );
}

// upstream-test: types/test_part_type.py::test_no_function_calls
#[test]
fn test_no_function_calls() {
    let response = response(vec![candidate(vec![
        text_part("Hello1"),
        text_part("World1"),
    ])]);
    assert!(response.function_calls().is_empty());
}

// Deviation shared by the `executable_code` / `code_execution_result`
// tests below: Python's properties return the code / output string; the
// Rust accessors return the whole `ExecutableCode` / `CodeExecutionResult`
// (a superset), so the assertions read `.code` / `.output`.

// upstream-test: types/test_part_type.py::test_executable_code_empty_candidates
#[test]
fn test_executable_code_empty_candidates() {
    assert_eq!(GenerateContentResponse::default().executable_code(), None);
}

// upstream-test: types/test_part_type.py::test_executable_code_empty_content
#[test]
fn test_executable_code_empty_content() {
    assert_eq!(response(vec![]).executable_code(), None);
}

// upstream-test: types/test_part_type.py::test_executable_code_empty_parts
#[test]
fn test_executable_code_empty_parts() {
    let candidate = Candidate {
        content: Some(Content::default()),
        ..Default::default()
    };
    assert_eq!(response(vec![candidate]).executable_code(), None);
}

// upstream-test: types/test_part_type.py::test_executable_code_two_candidates
#[test]
fn test_executable_code_two_candidates() {
    let response = response(vec![
        candidate(vec![code_part("print(\"hello\")")]),
        candidate(vec![code_part("print(\"world\")")]),
    ]);
    let code: Option<&ExecutableCode> = response.executable_code();
    assert_eq!(
        code.and_then(|code| code.code.as_deref()),
        Some("print(\"hello\")")
    );
}

// upstream-test: types/test_part_type.py::test_executable_code_one_candidate
#[test]
fn test_executable_code_one_candidate() {
    let response = response(vec![candidate(vec![code_part("print(\"hello\")")])]);
    assert_eq!(
        response
            .executable_code()
            .and_then(|code| code.code.as_deref()),
        Some("print(\"hello\")")
    );
}

// upstream-test: types/test_part_type.py::test_code_execution_result_empty_candidates
#[test]
fn test_code_execution_result_empty_candidates() {
    assert_eq!(
        GenerateContentResponse::default().code_execution_result(),
        None
    );
}

// upstream-test: types/test_part_type.py::test_code_execution_result_empty_content
#[test]
fn test_code_execution_result_empty_content() {
    assert_eq!(response(vec![]).code_execution_result(), None);
}

// upstream-test: types/test_part_type.py::test_code_execution_result_empty_parts
#[test]
fn test_code_execution_result_empty_parts() {
    let candidate = Candidate {
        content: Some(Content::default()),
        ..Default::default()
    };
    assert_eq!(response(vec![candidate]).code_execution_result(), None);
}

// upstream-test: types/test_part_type.py::test_code_execution_result_two_candidates
#[test]
fn test_code_execution_result_two_candidates() {
    let response = response(vec![
        candidate(vec![result_part(Outcome::OutcomeOk, "\"hello\"")]),
        candidate(vec![result_part(
            Outcome::from("OUTCOME_ERROR".to_owned()),
            "\"world\"",
        )]),
    ]);
    let result: Option<&CodeExecutionResult> = response.code_execution_result();
    assert_eq!(
        result.and_then(|result| result.output.as_deref()),
        Some("\"hello\"")
    );
}

// upstream-test: types/test_part_type.py::test_code_execution_result_one_candidate
#[test]
fn test_code_execution_result_one_candidate() {
    let response = response(vec![candidate(vec![result_part(
        Outcome::OutcomeOk,
        "\"hello\"",
    )])]);
    assert_eq!(
        response
            .code_execution_result()
            .and_then(|result| result.output.as_deref()),
        Some("\"hello\"")
    );
}

const FILE_URI: &str = "gs://test";
const IMAGE_PNG: &str = "image/png";
const LOW: PartMediaResolutionLevel = PartMediaResolutionLevel::MediaResolutionLow;

fn assert_low_uri_part(part: &Part) {
    let file_data = part.file_data.as_ref().expect("file_data is set");
    assert_eq!(file_data.file_uri.as_deref(), Some(FILE_URI));
    assert_eq!(file_data.mime_type.as_deref(), Some(IMAGE_PNG));
    assert_eq!(
        part.media_resolution.as_ref().and_then(|m| m.level.clone()),
        Some(LOW)
    );
}

fn assert_low_bytes_part(part: &Part) {
    let blob = part.inline_data.as_ref().expect("inline_data is set");
    assert_eq!(blob.data.as_deref(), Some(b"1234".as_slice()));
    assert_eq!(blob.mime_type.as_deref(), Some(IMAGE_PNG));
    assert_eq!(
        part.media_resolution.as_ref().and_then(|m| m.level.clone()),
        Some(LOW)
    );
}

// Python accepts `media_resolution` as a str, an enum member or a
// `PartMediaResolution` object; Rust spells that `with_media_resolution`
// taking `impl Into<PartMediaResolution>`.

// upstream-test: types/test_part_type.py::test_from_file_media_resolution_str
#[test]
fn test_from_file_media_resolution_str() {
    let level = PartMediaResolutionLevel::from("MEDIA_RESOLUTION_LOW".to_owned());
    assert_low_uri_part(&Part::from_uri(FILE_URI, IMAGE_PNG).with_media_resolution(level));
}

// upstream-test: types/test_part_type.py::test_from_file_media_resolution_enum
#[test]
fn test_from_file_media_resolution_enum() {
    assert_low_uri_part(&Part::from_uri(FILE_URI, IMAGE_PNG).with_media_resolution(LOW));
}

// upstream-test: types/test_part_type.py::test_from_file_media_resolution_object
#[test]
fn test_from_file_media_resolution_object() {
    let object = PartMediaResolution {
        level: Some(LOW),
        ..Default::default()
    };
    assert_low_uri_part(&Part::from_uri(FILE_URI, IMAGE_PNG).with_media_resolution(object));
}

// upstream-test: types/test_part_type.py::test_from_bytes_media_resolution_str
#[test]
fn test_from_bytes_media_resolution_str() {
    let level = PartMediaResolutionLevel::from("MEDIA_RESOLUTION_LOW".to_owned());
    assert_low_bytes_part(
        &Part::from_bytes(b"1234".to_vec(), IMAGE_PNG).with_media_resolution(level),
    );
}

// upstream-test: types/test_part_type.py::test_from_bytes_media_resolution_enum
#[test]
fn test_from_bytes_media_resolution_enum() {
    assert_low_bytes_part(
        &Part::from_bytes(b"1234".to_vec(), IMAGE_PNG).with_media_resolution(LOW),
    );
}

// upstream-test: types/test_part_type.py::test_from_bytes_media_resolution_object
#[test]
fn test_from_bytes_media_resolution_object() {
    let object = PartMediaResolution {
        level: Some(LOW),
        ..Default::default()
    };
    assert_low_bytes_part(
        &Part::from_bytes(b"1234".to_vec(), IMAGE_PNG).with_media_resolution(object),
    );
}
