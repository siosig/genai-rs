//! Port of `transformers/test_t_parts.py`.

use gemini_genai::__test_support::transformers as t;
use serde_json::{Value, json};

use super::t_part::{file, file_part};

// upstream-test: transformers/test_t_parts.py::test_none
#[test]
fn test_none() {
    assert!(t::t_parts(Value::Null).is_err());
}

// upstream-test: transformers/test_t_parts.py::test_empty_list
#[test]
fn test_empty_list() {
    assert!(t::t_parts(json!([])).is_err());
}

// upstream-test: transformers/test_t_parts.py::test_list
#[test]
fn test_list() {
    assert_eq!(
        t::t_parts(json!(["test1", "test2"])).unwrap(),
        json!([{"text": "test1"}, {"text": "test2"}])
    );
}

// upstream-test: transformers/test_t_parts.py::test_empty_dict
#[test]
fn test_empty_dict() {
    assert_eq!(t::t_parts(json!({})).unwrap(), json!([{}]));
}

// upstream-test: transformers/test_t_parts.py::test_dict
#[test]
fn test_dict() {
    assert_eq!(
        t::t_parts(json!({"text": "test"})).unwrap(),
        json!([{"text": "test"}])
    );
}

// upstream-test: transformers/test_t_parts.py::test_invalid_dict
#[test]
fn test_invalid_dict() {
    assert!(t::t_parts(json!({"invalid_key": "test"})).is_err());
}

// upstream-test: transformers/test_t_parts.py::test_string
#[test]
fn test_string() {
    assert_eq!(
        t::t_parts(json!("test")).unwrap(),
        json!([{"text": "test"}])
    );
}

// upstream-test: transformers/test_t_parts.py::test_file
#[test]
fn test_file() {
    let input = file(Some("gs://test"), Some("image/png"));
    assert_eq!(t::t_parts(input).unwrap(), json!([file_part()]));
}

// upstream-test: transformers/test_t_parts.py::test_file_no_uri
#[test]
fn test_file_no_uri() {
    assert!(t::t_parts(file(None, Some("image/png"))).is_err());
}

// upstream-test: transformers/test_t_parts.py::test_file_no_mime_type
#[test]
fn test_file_no_mime_type() {
    assert!(t::t_parts(file(Some("gs://test"), None)).is_err());
}

// upstream-test: transformers/test_t_parts.py::test_part
#[test]
fn test_part() {
    assert_eq!(
        t::t_parts(json!({"text": "test"})).unwrap(),
        json!([{"text": "test"}])
    );
}

// upstream-test: transformers/test_t_parts.py::test_int
#[test]
fn test_int() {
    let err = t::t_parts(json!(1)).unwrap_err();
    assert!(
        err.to_string()
            .contains("Unsupported content part type: <class 'int'>"),
        "unexpected error: {err}"
    );
}
