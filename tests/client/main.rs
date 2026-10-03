//! Ports of the upstream `google/genai/tests/client/` suite: API-key and
//! environment resolution, `HttpOptions` merging, base URL precedence, SDK
//! headers, timeouts, retries, streaming error events and resumable-upload
//! errors, all driven through the public API against a `wiremock` server.
//!
//! One module per upstream test file. Tests whose subject exists only in
//! Python (httpx/aiohttp objects, event loops, SSL contexts, pickling) or
//! only on Vertex AI are recorded as exclusions in
//! `tools/codegen/upstream_tests.toml` instead of being ported here.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a failed mock, request or client build here is a test-setup bug, not a runtime condition"
)]

#[path = "../common/mod.rs"]
mod common;

mod async_stream;
mod client_initialization;
mod client_requests;
mod env_guard;
mod http_options;
mod retries;
mod support;
mod upload_errors;
