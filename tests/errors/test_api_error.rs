use std::collections::HashMap;

use gemini_genai::ApiError;

fn parse(code: u16, reason: &str, body: &str) -> ApiError {
    ApiError::from_response(code, reason, HashMap::new(), body.as_bytes())
}

// upstream-test: errors/test_api_error.py::test_constructor_code_exist_error_in_json
#[test]
fn test_constructor_code_exist_error_in_json() {
    let error = parse(
        400,
        "Bad Request",
        r#"{"error": {"code": 400, "message": "error message", "status": "INVALID_ARGUMENT"}}"#,
    );
    assert_eq!(error.code, 400);
    assert_eq!(error.message, "error message");
    assert_eq!(error.status.as_deref(), Some("INVALID_ARGUMENT"));
    assert!(error.details.is_empty());
}

// upstream-test: errors/test_api_error.py::test_constructor_error_not_in_json
#[test]
fn test_constructor_error_not_in_json() {
    let error = parse(
        400,
        "Bad Request",
        r#"{"message": "error message", "status": "INVALID_ARGUMENT", "code": 400}"#,
    );
    assert_eq!(error.code, 400);
    assert_eq!(error.message, "error message");
    assert_eq!(error.status.as_deref(), Some("INVALID_ARGUMENT"));
}

// upstream-test: errors/test_api_error.py::test_constructor_error_in_json_status_outside_error
#[test]
fn test_constructor_error_in_json_status_outside_error() {
    let error = parse(
        400,
        "Bad Request",
        r#"{"status": "OUTER_INVALID_ARGUMENT_STATUS", "error": {"code": 400,
            "message": "error message", "status": "INNER_INVALID_ARGUMENT_STATUS"}}"#,
    );
    assert_eq!(error.code, 400);
    assert_eq!(error.message, "error message");
    assert_eq!(
        error.status.as_deref(),
        Some("OUTER_INVALID_ARGUMENT_STATUS")
    );
}

// upstream-test: errors/test_api_error.py::test_constructor_status_not_present
#[test]
fn test_constructor_status_not_present() {
    let error = parse(
        400,
        "Bad Request",
        r#"{"error": {"code": 400, "message": "error message"}}"#,
    );
    assert_eq!(error.code, 400);
    assert_eq!(error.message, "error message");
    assert_eq!(error.status, None);
}

// upstream-test: errors/test_api_error.py::test_constructor_error_in_json_message_outside_error
#[test]
fn test_constructor_error_in_json_message_outside_error() {
    let error = parse(
        400,
        "Bad Request",
        r#"{"message": "OUTER_ERROR_MESSAGE", "error": {"code": 400,
            "message": "INNER_ERROR_MESSAGE", "status": "INVALID_ARGUMENT"}}"#,
    );
    assert_eq!(error.code, 400);
    assert_eq!(error.message, "OUTER_ERROR_MESSAGE");
    assert_eq!(error.status.as_deref(), Some("INVALID_ARGUMENT"));
}

// upstream-test: errors/test_api_error.py::test_constructor_message_not_present
#[test]
fn test_constructor_message_not_present() {
    let body = r#"{"error": {"code": 400, "status": "INVALID_ARGUMENT"}}"#;
    let error = parse(400, "Bad Request", body);
    assert_eq!(error.code, 400);
    assert_eq!(error.status.as_deref(), Some("INVALID_ARGUMENT"));
    // Python's `message` is `None`; Rust's is a `String` and falls back to
    // the raw response body.
    assert_eq!(error.message, body);
}

// upstream-test: errors/test_api_error.py::test_raise_for_response_code_exist_json_decoder_error
#[test]
fn test_raise_for_response_code_exist_json_decoder_error() {
    let body = r#"{"data": {"key1": "value1", "key2"}"#;
    let error = parse(503, "Service Unavailable", body);
    assert_eq!(error.code, 503);
    assert_eq!(error.message, body);
    assert_eq!(error.status.as_deref(), Some("Service Unavailable"));
    assert!(error.is_server_error());
}

// upstream-test: errors/test_api_error.py::test_raise_for_response_client_error
#[test]
fn test_raise_for_response_client_error() {
    let error = parse(
        400,
        "Bad Request",
        r#"{"error": {"code": 400, "message": "error message", "status": "INVALID_ARGUMENT"}}"#,
    );
    assert!(error.is_client_error());
    assert!(!error.is_server_error());
    assert_eq!(error.code, 400);
    assert_eq!(error.message, "error message");
    assert_eq!(error.status.as_deref(), Some("INVALID_ARGUMENT"));
}

// upstream-test: errors/test_api_error.py::test_raise_for_response_server_error
#[test]
fn test_raise_for_response_server_error() {
    let error = parse(
        500,
        "Internal Server Error",
        r#"{"error": {"code": 500, "message": "error message", "status": "SERVER_INTERNAL ERROR"}}"#,
    );
    assert!(error.is_server_error());
    assert!(!error.is_client_error());
    assert_eq!(error.code, 500);
    assert_eq!(error.message, "error message");
    assert_eq!(error.status.as_deref(), Some("SERVER_INTERNAL ERROR"));
}
