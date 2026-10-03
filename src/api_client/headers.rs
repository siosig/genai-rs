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

/// The library label (`gemini-genai/<version>`) alone, without the language
/// label. A usage suffix such as `+afc` is attached to this token (see
/// `crate::extra_utils::get_usage_header`).
pub(crate) fn library_label() -> String {
    format!("gemini-genai/{}", env!("CARGO_PKG_VERSION"))
}

/// The language label (`gl-rust/<rustc>`).
fn language_label() -> String {
    format!("gl-rust/{}", rustc_version_label())
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
    let library = library_label();
    let language = language_label();
    let label = format!("{library} {language}");
    for header in [USER_AGENT_HEADER, API_CLIENT_HEADER] {
        match headers.get_mut(header) {
            // Mirrors Python's `append_library_version_headers`: the library
            // label is looked for on its own, so a value that already carries
            // it with a usage suffix (`gemini-genai/<version>+afc`) is not
            // prefixed a second time; only the language label is added.
            Some(existing) if !existing.contains(&library) => {
                *existing = format!("{label} {existing}");
            }
            Some(existing) if !existing.contains(&language) => {
                existing.push(' ');
                existing.push_str(&language);
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
    // Python treats a zero timeout as unset (`if timeout_in_seconds ...`).
    timeout_ms
        .filter(|ms| *ms != 0)
        .map(|ms| ms.max(0).unsigned_abs().div_ceil(1000).max(1).to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        API_CLIENT_HEADER, USER_AGENT_HEADER, apply_sdk_headers, library_label,
        server_timeout_seconds,
    };

    #[test]
    fn apply_sdk_headers_inserts_when_absent() {
        let mut headers = HashMap::new();
        apply_sdk_headers(&mut headers);
        assert!(headers[USER_AGENT_HEADER].starts_with("gemini-genai/"));
        assert!(headers[API_CLIENT_HEADER].starts_with("gemini-genai/"));
    }

    /// This crate is an unofficial port, so it must not report itself as the
    /// upstream SDK: the label the Python SDK sends is `google-genai-sdk/...`,
    /// and reusing it would fold this port's traffic into upstream's own
    /// client statistics with no way to tell the two apart.
    #[test]
    fn apply_sdk_headers_does_not_impersonate_the_upstream_sdk() {
        let mut headers = HashMap::new();
        apply_sdk_headers(&mut headers);
        for header in [USER_AGENT_HEADER, API_CLIENT_HEADER] {
            assert!(
                !headers[header].starts_with("google-genai-sdk/"),
                "{header} must not claim to be the upstream Python SDK, got {}",
                headers[header]
            );
        }
    }

    #[test]
    fn apply_sdk_headers_prepends_to_existing_value() {
        let mut headers = HashMap::new();
        headers.insert(USER_AGENT_HEADER.to_owned(), "custom-agent/1.0".to_owned());
        apply_sdk_headers(&mut headers);
        assert!(headers[USER_AGENT_HEADER].ends_with("custom-agent/1.0"));
        assert!(headers[USER_AGENT_HEADER].starts_with("gemini-genai/"));
    }

    /// A usage suffix (`+afc`) sits on the library label, so the label is
    /// found and only the language label is appended.
    #[test]
    fn apply_sdk_headers_does_not_prefix_a_label_that_carries_a_usage_suffix() {
        let usage_label = format!("{}+afc", library_label());
        let mut headers = HashMap::new();
        headers.insert(USER_AGENT_HEADER.to_owned(), usage_label.clone());
        apply_sdk_headers(&mut headers);
        assert!(headers[USER_AGENT_HEADER].starts_with(&usage_label));
        assert_eq!(
            headers[USER_AGENT_HEADER].matches("gemini-genai/").count(),
            1
        );
        assert!(headers[USER_AGENT_HEADER].contains("gl-rust/"));
    }

    #[test]
    fn server_timeout_seconds_rounds_up() {
        assert_eq!(server_timeout_seconds(Some(1500)), Some("2".to_owned()));
        assert_eq!(server_timeout_seconds(Some(1000)), Some("1".to_owned()));
        assert_eq!(server_timeout_seconds(None), None);
    }
}
