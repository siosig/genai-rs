//! Ported from upstream `tests/types/test_bytes_type.py`: how `bytes`
//! fields (`Blob.data`) are decoded from and encoded to base64. Python
//! models a `bytes` field with a throwaway pydantic model; the Rust
//! equivalent is `Blob.data`, which uses the `WireBase64` serde adapter.

#![expect(
    clippy::expect_used,
    reason = "helper functions outside #[test] bodies assert test preconditions; a failure there is a test bug"
)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gemini_genai::types::Blob;
use serde_json::{Value, json};

/// 64 distinct chars in url safe base64.
const URL_SAFE_BASE64: &str = "-_abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
/// 64 distinct chars in normal base64.
const BASE64: &str = "+/abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

fn raw_bytes() -> Vec<u8> {
    STANDARD
        .decode(BASE64)
        .expect("BASE64 is valid standard base64")
}

fn blob_json(data: &str) -> Value {
    json!({ "data": data })
}

fn blob_with(raw: Vec<u8>) -> Blob {
    Blob {
        data: Some(raw),
        ..Default::default()
    }
}

// A url-safe base64 string is decoded to raw bytes; serialization yields the
// url-safe base64 string again.
// upstream-test: types/test_bytes_type.py::test_urlsafe_base64_input_success
#[test]
fn test_urlsafe_base64_input_success() {
    let blob: Blob = serde_json::from_value(blob_json(URL_SAFE_BASE64)).expect("valid base64");
    assert_eq!(blob.data, Some(raw_bytes()));
    assert_eq!(
        serde_json::to_value(&blob).expect("serializes"),
        blob_json(URL_SAFE_BASE64)
    );
}

// Raw bytes serialize to url-safe base64.
// upstream-test: types/test_bytes_type.py::test_raw_bytes_input_success
#[test]
fn test_raw_bytes_input_success() {
    let blob = blob_with(raw_bytes());
    assert_eq!(blob.data.as_deref(), Some(raw_bytes().as_slice()));
    let value = serde_json::to_value(&blob).expect("serializes");
    assert_eq!(value, blob_json(URL_SAFE_BASE64));
    assert!(value["data"].is_string());
}

// Normal base64 with a stray `=` (the 64 characters are a complete quad) is
// invalid. Python has a separate test for model construction and for
// `model_validate`; serde has one entry point, so both map here.
// upstream-test: types/test_bytes_type.py::test_invalid_base64_pydantic_input_failure
#[test]
fn test_invalid_base64_pydantic_input_failure() {
    let invalid = format!("{BASE64}=");
    assert!(serde_json::from_value::<Blob>(blob_json(&invalid)).is_err());
    assert!(serde_json::from_str::<Blob>(&blob_json(&invalid).to_string()).is_err());
}

// upstream-test: types/test_bytes_type.py::test_invalid_base64_dict_input_failure
#[test]
fn test_invalid_base64_dict_input_failure() {
    let invalid = format!("{BASE64}=");
    let error = serde_json::from_value::<Blob>(blob_json(&invalid)).expect_err("invalid base64");
    assert!(!error.to_string().is_empty());
}

// Normal (not url safe) base64 decodes to the raw bytes.
// upstream-test: types/test_bytes_type.py::test_normal_base64_pydantic_input_success
#[test]
fn test_normal_base64_pydantic_input_success() {
    let blob: Blob = serde_json::from_value(blob_json(BASE64)).expect("valid base64");
    assert_eq!(blob.data, Some(raw_bytes()));
}

// upstream-test: types/test_bytes_type.py::test_normal_base64_dict_input_success
#[test]
fn test_normal_base64_dict_input_success() {
    let blob: Blob = serde_json::from_str(&blob_json(BASE64).to_string()).expect("valid base64");
    assert_eq!(blob.data, Some(raw_bytes()));
}

// Python has two upstream tests with this name. In an `Any`-typed field a
// str is kept verbatim (never decoded); the Rust equivalent of an `Any`
// field is `serde_json::Value`. (Python's raw-bytes-in-`Any` variant has no
// Rust form: `Value` holds no bytes.)
// upstream-test: types/test_bytes_type.py::test_any_type_urlsafe_base64_input
#[test]
fn test_any_type_urlsafe_base64_input() {
    let value: Value = serde_json::from_value(blob_json(URL_SAFE_BASE64)).expect("any json");
    assert_eq!(value, blob_json(URL_SAFE_BASE64));
    assert_eq!(value["data"].as_str(), Some(URL_SAFE_BASE64));
}
