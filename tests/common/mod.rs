//! Shared helpers for integration tests.
//!
//! Every file under `tests/` compiles as its own crate, so each one that
//! wants these declares `mod common;` and imports only what it needs. That
//! makes the unused remainder look dead to the compiler in every single
//! test binary, hence the crate-wide `dead_code` allow below (the same
//! reason `ws_server` carries one).

#![allow(
    dead_code,
    reason = "each tests/*.rs is its own crate and pulls in only the helpers it needs, so the rest of this module is unused in that binary"
)]

pub mod ws_server;

use gemini_genai::{Client, types::HttpOptions};

/// Builds a [`Client`] talking to `base_url` (typically a `wiremock`
/// `MockServer`'s `uri()`) with a dummy API key.
#[expect(
    clippy::unwrap_used,
    reason = "test helper: a broken Client::builder() here is a test-setup bug, not a runtime condition"
)]
pub fn test_client(base_url: String) -> Client {
    Client::builder()
        .api_key("test-key")
        .http_options(HttpOptions {
            base_url: Some(base_url),
            ..Default::default()
        })
        .build()
        .unwrap()
}

/// Like [`test_client`], but with a caller-chosen API key. The Live API
/// tests assert on how the key reaches the WebSocket handshake (`?key=...`
/// vs an `Authorization` header), so they need to vary it.
#[expect(
    clippy::unwrap_used,
    reason = "test helper: a broken Client::builder() here is a test-setup bug, not a runtime condition"
)]
pub fn test_client_with_api_key(base_url: String, api_key: &str) -> Client {
    Client::builder()
        .api_key(api_key)
        .http_options(HttpOptions {
            base_url: Some(base_url),
            ..Default::default()
        })
        .build()
        .unwrap()
}

/// Builds a [`gemini_genai::blocking::Client`] talking to `base_url`.
///
/// Separate from [`test_client`] because `blocking::Client` is a distinct
/// type wrapping its own `Runtime`, not the async `Client`.
#[cfg(feature = "blocking")]
#[expect(
    clippy::unwrap_used,
    reason = "test helper: a broken Client::builder() here is a test-setup bug, not a runtime condition"
)]
pub fn blocking_test_client(base_url: String) -> gemini_genai::blocking::Client {
    gemini_genai::blocking::Client::builder()
        .api_key("test-key")
        .http_options(HttpOptions {
            base_url: Some(base_url),
            ..Default::default()
        })
        .build()
        .unwrap()
}
