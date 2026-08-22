//! HTTP configuration types (`HttpOptions`, `HttpRetryOptions`,
//! `HttpResponse`).
//!
//! Unlike most types in [`crate::types`], these are **hand-written, not
//! generated**: they configure the transport layer itself (base URL, retry
//! policy, timeouts) rather than describing a Gemini API request/response
//! body, so `gen_types.py` excludes them (see
//! `tools/codegen/types_overrides.toml`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Map;

/// Per-client or per-request HTTP configuration. Mirrors Python's
/// `types.HttpOptions`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HttpOptions {
    /// Overrides the API base URL (default:
    /// `https://generativelanguage.googleapis.com/`).
    pub base_url: Option<String>,
    /// Overrides the API version path segment (default: `v1beta`; an empty
    /// string omits the version segment entirely).
    pub api_version: Option<String>,
    /// Additional headers merged into every request.
    pub headers: Option<HashMap<String, String>>,
    /// Request timeout in milliseconds.
    pub timeout: Option<i64>,
    /// Extra fields deep-merged into every request body.
    pub extra_body: Option<Map<String, serde_json::Value>>,
    /// Retry policy; unset means no retries (a single attempt).
    pub retry_options: Option<HttpRetryOptions>,
}

/// Retry policy configuration. Mirrors Python's `types.HttpRetryOptions`.
///
/// When [`Client::builder`](crate::Client::builder) is not given retry
/// options at all, requests are attempted exactly once (no retries), which
/// matches the Python SDK's default. Setting `Some(HttpRetryOptions::default())`
/// (i.e. all fields `None`) opts into retries using the SDK's documented
/// defaults: 5 attempts, 1.0s initial delay, 60s max delay, exponential
/// base 2, jitter 1.0, retrying on 408/429/500/502/503/504.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HttpRetryOptions {
    /// Total number of attempts, including the first.
    pub attempts: Option<i64>,
    /// Initial backoff delay in seconds.
    pub initial_delay: Option<f64>,
    /// Maximum backoff delay in seconds.
    pub max_delay: Option<f64>,
    /// Exponential backoff base.
    pub exp_base: Option<f64>,
    /// Maximum random jitter added to each delay, in seconds.
    pub jitter: Option<f64>,
    /// HTTP status codes that should trigger a retry.
    pub http_status_codes: Option<Vec<i64>>,
}

/// A raw HTTP response, attached to typed responses for diagnostics.
/// Mirrors Python's `types.HttpResponse`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HttpResponse {
    /// Response headers.
    pub headers: Option<HashMap<String, String>>,
    /// Response body, if buffered (absent for streamed responses).
    pub body: Option<String>,
}
