//! Request header construction: SDK identification headers, timeout header,
//! and `HttpOptions.headers` merging, mirroring Python's `_api_client.py`
//! `_append_library_version_headers`.

use std::collections::HashMap;

/// Header name the Gemini API uses to identify the calling SDK and language.
pub(crate) const API_CLIENT_HEADER: &str = "x-goog-api-client";
/// Standard `User-Agent` header.
pub(crate) const USER_AGENT_HEADER: &str = "user-agent";
/// API key header used for Gemini Developer API authentication.
pub(crate) const API_KEY_HEADER: &str = "x-goog-api-key";
/// Header used to request a specific per-request server-side timeout.
pub(crate) const SERVER_TIMEOUT_HEADER: &str = "X-Server-Timeout";

fn sdk_version_label() -> String {
    format!(
        "google-genai-sdk/{} gl-rust/{}",
        env!("CARGO_PKG_VERSION"),
        rustc_version_label()
    )
}

fn rustc_version_label() -> &'static str {
    // `rustc --version` is not available at compile time without a build
    // script; the crate version is the stable, reproducible identifier we
    // control, so we use it as the "language version" label as well.
    env!("CARGO_PKG_RUST_VERSION")
}

/// Merges SDK identification headers into `headers`, appending to any
/// existing `user-agent` / `x-goog-api-client` values rather than
/// overwriting them (mirrors the Python client's behavior when a caller
/// supplies their own headers).
pub(crate) fn apply_sdk_headers(headers: &mut HashMap<String, String>) {
    let label = sdk_version_label();
    for header in [USER_AGENT_HEADER, API_CLIENT_HEADER] {
        match headers.get_mut(header) {
            Some(existing) if !existing.contains(&label) => {
                *existing = format!("{label} {existing}");
            }
            Some(_) => {}
            None => {
                headers.insert(header.to_owned(), label.clone());
            }
        }
    }
}

/// Computes the `X-Server-Timeout` header value (whole seconds, rounded up)
/// for a millisecond timeout, if one is configured.
#[must_use]
pub(crate) fn server_timeout_seconds(timeout_ms: Option<i64>) -> Option<String> {
    timeout_ms.map(|ms| {
        let secs = ms.div_ceil(1000).max(1);
        secs.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::{API_CLIENT_HEADER, USER_AGENT_HEADER, apply_sdk_headers, server_timeout_seconds};
    use std::collections::HashMap;

    #[test]
    fn apply_sdk_headers_inserts_when_absent() {
        let mut headers = HashMap::new();
        apply_sdk_headers(&mut headers);
        assert!(headers[USER_AGENT_HEADER].starts_with("google-genai-sdk/"));
        assert!(headers[API_CLIENT_HEADER].starts_with("google-genai-sdk/"));
    }

    #[test]
    fn apply_sdk_headers_prepends_to_existing_value() {
        let mut headers = HashMap::new();
        headers.insert(USER_AGENT_HEADER.to_owned(), "custom-agent/1.0".to_owned());
        apply_sdk_headers(&mut headers);
        assert!(headers[USER_AGENT_HEADER].ends_with("custom-agent/1.0"));
        assert!(headers[USER_AGENT_HEADER].starts_with("google-genai-sdk/"));
    }

    #[test]
    fn server_timeout_seconds_rounds_up() {
        assert_eq!(server_timeout_seconds(Some(1500)), Some("2".to_owned()));
        assert_eq!(server_timeout_seconds(Some(1000)), Some("1".to_owned()));
        assert_eq!(server_timeout_seconds(None), None);
    }
}
