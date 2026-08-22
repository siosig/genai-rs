//! Typed errors for `google_genai`, mirroring the Python SDK's `errors.py`.

use std::collections::HashMap;

use serde_json::Value;

/// Convenience alias for `Result<T, Error>`.
pub type Result<T> = core::result::Result<T, Error>;

/// Top-level error type returned by every fallible `google_genai` call.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A non-2xx response from the Gemini API. Corresponds to Python's
    /// `APIError` / `ClientError` / `ServerError`. Boxed: `ApiError` is
    /// large (a `String` message plus `Vec`/`HashMap` detail fields), and
    /// `clippy::result_large_err` flags an unboxed variant that size on
    /// every one of this crate's (numerous) `Result<T, Error>` returns.
    #[error("{0}")]
    Api(#[from] Box<ApiError>),

    /// A transport-level failure (connection, TLS, timeout).
    #[error("HTTP transport error: {0}")]
    Http(#[from] reqwest::Error),

    /// A JSON (de)serialization failure. Corresponds to Python's
    /// `UnknownApiResponseError`.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// A local filesystem I/O failure (e.g. reading a file to upload).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A request failed client-side validation before being sent.
    /// Corresponds to Python's `ValueError`.
    #[error("validation error: {0}")]
    Validation(String),

    /// A field was set that only the Vertex AI backend supports; this
    /// client is configured for the Gemini Developer API.
    /// Corresponds to Python's `ValueError('... only supported in ...')`.
    #[error("field `{field}` is only supported by the {backend} backend")]
    UnsupportedByBackend {
        /// The unsupported field's Python name (`snake_case`).
        field: &'static str,
        /// The backend the field requires.
        backend: Backend,
    },

    /// The client was configured to use a backend that is not yet
    /// implemented by this crate.
    #[error("backend `{0}` is not supported by this client yet")]
    UnsupportedBackend(&'static str),

    /// An automatic-function-calling failure. Corresponds to Python's
    /// `UnknownFunctionCallArgumentError` / `UnsupportedFunctionError` /
    /// `FunctionInvocationError`.
    #[error("function call error: {0}")]
    FunctionCall(#[from] FunctionCallError),

    /// A WebSocket (Live API) transport failure.
    #[cfg(feature = "live")]
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] Box<tokio_tungstenite::tungstenite::Error>),

    /// A streamed response (SSE or WebSocket) ended abnormally.
    #[error("stream error: {0}")]
    Stream(String),

    /// A resumable file upload failed.
    #[error("upload error: {0}")]
    Upload(String),

    /// [`Pager::next_page`](crate::Pager::next_page) was called with no
    /// further pages available. Corresponds to Python's `IndexError`.
    #[error("no more pages")]
    NoMorePages,

    /// A blocking client method was called from within an active Tokio
    /// runtime, which would deadlock.
    #[error("blocking call made from inside an async runtime")]
    BlockingInsideRuntime,
}

/// The backend a `google_genai` [`Client`](crate::Client) talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// The Gemini Developer API (`generativelanguage.googleapis.com`).
    GeminiApi,
    /// Vertex AI. Not yet implemented by this crate.
    VertexAi,
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Backend::GeminiApi => "Gemini Developer API",
            Backend::VertexAi => "Vertex AI",
        };
        f.write_str(name)
    }
}

/// An error response from the Gemini API, mirroring Python's `APIError`.
#[derive(Debug, Clone)]
pub struct ApiError {
    /// The HTTP status code of the response.
    pub code: u16,
    /// The API-reported status string (e.g. `INVALID_ARGUMENT`), if present.
    pub status: Option<String>,
    /// A human-readable error message.
    pub message: String,
    /// Additional structured error details, if present.
    pub details: Vec<Value>,
    /// Response headers, for diagnostics.
    pub response_headers: HashMap<String, String>,
}

impl ApiError {
    /// Builds an [`ApiError`] from an HTTP status code and a (possibly
    /// non-JSON) response body, following the Gemini API's
    /// `{"error": {"code", "message", "status", "details"}}` envelope.
    #[must_use]
    pub fn from_response(
        status_code: u16,
        reason_phrase: &str,
        response_headers: HashMap<String, String>,
        body: &[u8],
    ) -> Self {
        let parsed: Option<Value> = serde_json::from_slice(body).ok();
        let error_obj = parsed.as_ref().and_then(|v| v.get("error"));

        let message = error_obj
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .map_or_else(|| String::from_utf8_lossy(body).into_owned(), str::to_owned);

        let status = error_obj
            .and_then(|e| e.get("status"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| Some(reason_phrase.to_owned()));

        let details = error_obj
            .and_then(|e| e.get("details"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        Self {
            code: status_code,
            status,
            message,
            details,
            response_headers,
        }
    }

    /// Whether this is a 4xx client error.
    #[must_use]
    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.code)
    }

    /// Whether this is a 5xx server error.
    #[must_use]
    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.code)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = self.status.as_deref().unwrap_or("UNKNOWN");
        write!(f, "{} {status}. {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

/// Errors raised while resolving or invoking an automatic function-calling
/// tool. Corresponds to Python's `UnknownFunctionCallArgumentError` /
/// `UnsupportedFunctionError` / `FunctionInvocationError`.
#[derive(Debug, thiserror::Error)]
pub enum FunctionCallError {
    /// The model requested a function that was not registered as a tool.
    #[error("unsupported function call: `{0}`")]
    UnsupportedFunction(String),

    /// The model's function-call arguments did not match the tool's schema.
    #[error("unknown function call argument for `{function}`: {message}")]
    UnknownArgument {
        /// The function name.
        function: String,
        /// A description of the mismatch.
        message: String,
    },

    /// The tool's callable returned an error while executing.
    #[error("function `{function}` invocation failed: {message}")]
    Invocation {
        /// The function name.
        function: String,
        /// The underlying error message.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{ApiError, Backend, Error};

    #[test]
    fn api_error_display_matches_python_format() {
        let err = ApiError {
            code: 400,
            status: Some("INVALID_ARGUMENT".to_owned()),
            message: "bad request".to_owned(),
            details: vec![],
            response_headers: std::collections::HashMap::new(),
        };
        assert_eq!(err.to_string(), "400 INVALID_ARGUMENT. bad request");
    }

    #[test]
    fn api_error_from_json_body_extracts_status_and_message() {
        let body = br#"{"error":{"code":429,"message":"rate limited","status":"RESOURCE_EXHAUSTED","details":[{"reason":"x"}]}}"#;
        let err = ApiError::from_response(
            429,
            "Too Many Requests",
            std::collections::HashMap::new(),
            body,
        );
        assert_eq!(err.status.as_deref(), Some("RESOURCE_EXHAUSTED"));
        assert_eq!(err.message, "rate limited");
        assert_eq!(err.details.len(), 1);
        assert!(err.is_client_error());
        assert!(!err.is_server_error());
    }

    #[test]
    fn api_error_from_non_json_body_uses_raw_text_and_reason_phrase() {
        let err = ApiError::from_response(
            503,
            "Service Unavailable",
            std::collections::HashMap::new(),
            b"upstream down",
        );
        assert_eq!(err.status.as_deref(), Some("Service Unavailable"));
        assert_eq!(err.message, "upstream down");
        assert!(err.is_server_error());
    }

    #[test]
    fn unsupported_by_backend_error_names_field_and_backend() {
        let err = Error::UnsupportedByBackend {
            field: "labels",
            backend: Backend::VertexAi,
        };
        assert_eq!(
            err.to_string(),
            "field `labels` is only supported by the Vertex AI backend"
        );
    }
}
