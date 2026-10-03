//! Port of `transformers/test_t_part.py`. A `types.File` is the JSON its
//! Rust counterpart serializes to; it is told apart from a `FileData` by
//! its `File`-only keys (`uri`, `name`, ...), so the "no uri" case carries
//! a `name` instead of being a bare `{mime_type}` (which is a `FileData`).

use gemini_genai::__test_support::transformers as t;
use serde_json::{Value, json};

pub fn file(uri: Option<&str>, mime_type: Option<&str>) -> Value {
    // The JSON a `types::File` serializes to (`None` fields are skipped).
    let mut file = json!({});
    if let Some(uri) = uri {
        file["uri"] = json!(uri);
    } else {
        file["name"] = json!("files/abc");
    }
    if let Some(mime_type) = mime_type {
        file["mime_type"] = json!(mime_type);
    }
    file
}

pub fn file_part() -> Value {
    json!({"file_data": {"file_uri": "gs://test", "mime_type": "image/png"}})
}

// upstream-test: transformers/test_t_part.py::test_none
#[test]
fn test_none() {
    assert!(t::t_part(Value::Null).is_err());
}

// upstream-test: transformers/test_t_part.py::test_empty_string
#[test]
fn test_empty_string() {
    assert_eq!(t::t_part(json!("")).unwrap(), json!({"text": ""}));
}

// upstream-test: transformers/test_t_part.py::test_string
#[test]
fn test_string() {
    assert_eq!(t::t_part(json!("test")).unwrap(), json!({"text": "test"}));
}

// upstream-test: transformers/test_t_part.py::test_file
#[test]
fn test_file() {
    let input = file(Some("gs://test"), Some("image/png"));
    assert_eq!(t::t_part(input).unwrap(), file_part());
}

// upstream-test: transformers/test_t_part.py::test_file_dict
#[test]
fn test_file_dict() {
    let input = json!({"file_uri": "gs://test", "mime_type": "image/png"});
    assert_eq!(t::t_part(input).unwrap(), file_part());
}

// upstream-test: transformers/test_t_part.py::test_file_no_uri
#[test]
fn test_file_no_uri() {
    assert!(t::t_part(file(None, Some("image/png"))).is_err());
}

// upstream-test: transformers/test_t_part.py::test_file_no_mime_type
#[test]
fn test_file_no_mime_type() {
    assert!(t::t_part(file(Some("gs://test"), None)).is_err());
}

// upstream-test: transformers/test_t_part.py::test_empty_dict
#[test]
fn test_empty_dict() {
    assert_eq!(t::t_part(json!({})).unwrap(), json!({}));
}

// upstream-test: transformers/test_t_part.py::test_dict
#[test]
fn test_dict() {
    assert_eq!(
        t::t_part(json!({"text": "test"})).unwrap(),
        json!({"text": "test"})
    );
}

// upstream-test: transformers/test_t_part.py::test_invalid_dict
#[test]
fn test_invalid_dict() {
    assert!(t::t_part(json!({"invalid_key": "test"})).is_err());
}

// upstream-test: transformers/test_t_part.py::test_part
#[test]
fn test_part() {
    let part = serde_json::to_value(gemini_genai::types::Part {
        text: Some("test".to_owned()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(t::t_part(part).unwrap(), json!({"text": "test"}));
}

// upstream-test: transformers/test_t_part.py::test_int
#[test]
fn test_int() {
    let err = t::t_part(json!(1)).unwrap_err();
    assert!(
        err.to_string()
            .contains("Unsupported content part type: <class 'int'>"),
        "unexpected error: {err}"
    );
}
