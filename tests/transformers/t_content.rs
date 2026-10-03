//! Port of `transformers/test_t_content.py`.

use gemini_genai::__test_support::transformers as t;
use serde_json::{Value, json};

use super::t_part::{file, file_part};

fn user(parts: Value) -> Value {
    let mut content = json!({"role": "user"});
    content["parts"] = parts;
    content
}

fn function_call_part() -> Value {
    json!({"function_call": {"name": "test_func", "args": {"arg1": "value1"}}})
}

// upstream-test: transformers/test_t_content.py::test_none
#[test]
fn test_none() {
    assert!(t::t_content(Value::Null).is_err());
}

// upstream-test: transformers/test_t_content.py::test_content
#[test]
fn test_content() {
    let content = json!({"parts": [{"text": "test"}]});
    assert_eq!(t::t_content(content.clone()).unwrap(), content);
}

// upstream-test: transformers/test_t_content.py::test_content_dict
#[test]
fn test_content_dict() {
    let content = json!({"role": "user", "parts": [{"text": "test"}]});
    assert_eq!(t::t_content(content.clone()).unwrap(), content);
}

// upstream-test: transformers/test_t_content.py::test_content_dict_invalid
#[test]
fn test_content_dict_invalid() {
    assert!(t::t_content(json!({"invalid_key": "test"})).is_err());
}

// upstream-test: transformers/test_t_content.py::test_text_part_dict
#[test]
fn test_text_part_dict() {
    assert_eq!(
        t::t_content(json!({"text": "test"})).unwrap(),
        user(json!([{"text": "test"}]))
    );
}

// upstream-test: transformers/test_t_content.py::test_function_call_part_dict
#[test]
fn test_function_call_part_dict() {
    assert_eq!(
        t::t_content(function_call_part()).unwrap(),
        json!({"parts": [function_call_part()], "role": "model"})
    );
}

// upstream-test: transformers/test_t_content.py::test_text_part
#[test]
fn test_text_part() {
    assert_eq!(
        t::t_content(json!({"text": "test"})).unwrap(),
        user(json!([{"text": "test"}]))
    );
}

// upstream-test: transformers/test_t_content.py::test_function_call_part
#[test]
fn test_function_call_part() {
    assert_eq!(
        t::t_content(function_call_part()).unwrap(),
        json!({"parts": [function_call_part()], "role": "model"})
    );
}

// upstream-test: transformers/test_t_content.py::test_string
#[test]
fn test_string() {
    assert_eq!(
        t::t_content(json!("test")).unwrap(),
        user(json!([{"text": "test"}]))
    );
}

// upstream-test: transformers/test_t_content.py::test_file
#[test]
fn test_file() {
    assert_eq!(
        t::t_content(file(Some("gs://test"), Some("image/png"))).unwrap(),
        user(json!([file_part()]))
    );
}

// upstream-test: transformers/test_t_content.py::test_file_no_uri
#[test]
fn test_file_no_uri() {
    assert!(t::t_content(file(None, Some("image/png"))).is_err());
}

// upstream-test: transformers/test_t_content.py::test_file_no_mime_type
#[test]
fn test_file_no_mime_type() {
    assert!(t::t_content(file(Some("gs://test"), None)).is_err());
}

// upstream-test: transformers/test_t_content.py::test_file_dict
#[test]
fn test_file_dict() {
    let input = json!({"file_uri": "gs://test", "mime_type": "image/png"});
    assert_eq!(t::t_content(input).unwrap(), user(json!([file_part()])));
}

// upstream-test: transformers/test_t_content.py::test_int
#[test]
fn test_int() {
    let err = t::t_content(json!(1)).unwrap_err();
    assert!(
        err.to_string()
            .contains("Unsupported content part type: <class 'int'>"),
        "unexpected error: {err}"
    );
}

// upstream-test: transformers/test_t_content.py::test_t_contents_strict_content
#[test]
fn test_t_contents_strict_content() {
    // `b"test"` is `dGVzdA==` once serialized.
    let content = json!({"parts": [{"inline_data": {"data": "dGVzdA=="}}]});
    let cases = [content.clone(), json!([content])];
    for (index, contents) in cases.into_iter().enumerate() {
        assert_eq!(
            t::t_contents_strict(contents).unwrap(),
            json!([content.clone()]),
            "case {index}"
        );
    }
}
