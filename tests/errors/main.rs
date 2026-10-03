//! Integration tests for `gemini_genai::ApiError`, ported from upstream
//! `errors/test_api_error.py`.
//!
//! Deliberate shape differences from Python's `APIError(code, response_json)`
//! (typed Rust API): `ApiError::code` is always the HTTP status (never
//! `None`), `message` is a `String` that falls back to the raw body, and
//! `details` holds only the response's `error.details` array rather than the
//! whole decoded body.

mod test_api_error;
