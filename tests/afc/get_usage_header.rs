//! Ports of `google/genai/tests/afc/test_get_usage_header.py`.
//!
//! The SDK label is this crate's own (`gemini-genai/<version>`, see
//! `src/api_client/headers.rs`), not Python's `google-genai-sdk/<version>`.
//! Python's dict-config cases (`{'temperature': 0.5}`) coerce a dict into a
//! config model; here the config is always typed, so those cases pass the
//! equivalent typed config.

use std::collections::HashMap;

use gemini_genai::{
    __test_support::extra_utils::get_usage_header,
    types::{GenerateContentConfig, GenerateImagesConfig, HttpOptions},
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn label() -> String {
    format!("gemini-genai/{VERSION}")
}

fn headers_of(config: &GenerateContentConfig) -> &HashMap<String, String> {
    config
        .http_options
        .as_ref()
        .and_then(|options| options.headers.as_ref())
        .expect("get_usage_header always sets http_options.headers")
}

fn config_with_headers(pairs: &[(&str, &str)]) -> GenerateContentConfig {
    GenerateContentConfig {
        http_options: Some(HttpOptions {
            headers: Some(
                pairs
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                    .collect(),
            ),
            ..Default::default()
        }),
        ..Default::default()
    }
}

// upstream-test: afc/test_get_usage_header.py::test_get_usage_header_none_config
#[test]
fn test_get_usage_header_none_config() {
    let config = get_usage_header::<GenerateContentConfig>(None, "afc");
    let expected = format!("{}+afc", label());
    assert_eq!(headers_of(&config)["user-agent"], expected);
    assert_eq!(headers_of(&config)["x-goog-api-client"], expected);
}

// upstream-test: afc/test_get_usage_header.py::test_get_usage_header_dict_config
#[test]
fn test_get_usage_header_dict_config() {
    let config = get_usage_header(
        Some(GenerateContentConfig {
            temperature: Some(0.5),
            ..Default::default()
        }),
        "afc",
    );
    let expected = format!("{}+afc", label());
    assert_eq!(config.temperature, Some(0.5));
    assert_eq!(headers_of(&config)["user-agent"], expected);
    assert_eq!(headers_of(&config)["x-goog-api-client"], expected);
}

// upstream-test: afc/test_get_usage_header.py::test_get_usage_header_custom_usage
#[test]
fn test_get_usage_header_custom_usage() {
    let config = get_usage_header::<GenerateContentConfig>(None, "chat");
    let expected = format!("{}+chat", label());
    assert_eq!(headers_of(&config)["user-agent"], expected);
    assert_eq!(headers_of(&config)["x-goog-api-client"], expected);
}

// upstream-test: afc/test_get_usage_header.py::test_get_usage_header_with_existing_sdk_headers
#[test]
fn test_get_usage_header_with_existing_sdk_headers() {
    let initial = format!("{} gl-rust/1.85", label());
    let config = config_with_headers(&[("user-agent", &initial), ("x-goog-api-client", &initial)]);
    let config = get_usage_header(Some(config), "afc");
    let expected = format!("{}+afc gl-rust/1.85", label());
    assert_eq!(headers_of(&config)["user-agent"], expected);
    assert_eq!(headers_of(&config)["x-goog-api-client"], expected);
}

// upstream-test: afc/test_get_usage_header.py::test_get_usage_header_idempotent_no_duplicate_usage
#[test]
fn test_get_usage_header_idempotent_no_duplicate_usage() {
    let config = get_usage_header::<GenerateContentConfig>(None, "afc");
    let config = get_usage_header(Some(config), "afc");
    let expected = format!("{}+afc", label());
    assert_eq!(headers_of(&config)["user-agent"], expected);
    assert_eq!(headers_of(&config)["x-goog-api-client"], expected);
}

// upstream-test: afc/test_get_usage_header.py::test_get_usage_header_multiple_usages_no_duplicate
#[test]
fn test_get_usage_header_multiple_usages_no_duplicate() {
    let config = get_usage_header::<GenerateContentConfig>(None, "chat");
    let config = get_usage_header(Some(config), "afc");
    // Call again with chat and afc to ensure no duplicate entries are appended.
    let config = get_usage_header(Some(config), "chat");
    let config = get_usage_header(Some(config), "afc");

    for header in ["user-agent", "x-goog-api-client"] {
        let value = &headers_of(&config)[header];
        assert!(value.contains("+chat"), "{header}: {value}");
        assert!(value.contains("+afc"), "{header}: {value}");
        assert_eq!(value.matches("+chat").count(), 1, "{header}: {value}");
        assert_eq!(value.matches("+afc").count(), 1, "{header}: {value}");
    }
}

// upstream-test: afc/test_get_usage_header.py::test_get_usage_header_with_custom_user_agent
#[test]
fn test_get_usage_header_with_custom_user_agent() {
    let config = config_with_headers(&[
        ("user-agent", "custom-agent/1.0"),
        ("x-goog-api-client", "custom-agent/1.0"),
    ]);
    let config = get_usage_header(Some(config), "afc");
    // Call again to ensure idempotency.
    let config = get_usage_header(Some(config), "afc");

    let expected_usage = format!("{}+afc", label());
    let user_agent = &headers_of(&config)["user-agent"];
    assert!(user_agent.contains("custom-agent/1.0"));
    assert!(user_agent.contains(&expected_usage));
    assert_eq!(user_agent.matches("+afc").count(), 1);
}

// upstream-test: afc/test_get_usage_header.py::test_get_usage_header_preserves_other_http_options
#[test]
fn test_get_usage_header_preserves_other_http_options() {
    let config = GenerateContentConfig {
        http_options: Some(HttpOptions {
            api_version: Some("v1alpha".to_owned()),
            headers: Some(HashMap::from([(
                "custom-header".to_owned(),
                "value".to_owned(),
            )])),
            ..Default::default()
        }),
        ..Default::default()
    };
    let config = get_usage_header(Some(config), "afc");

    let options = config.http_options.as_ref().expect("http_options is set");
    assert_eq!(options.api_version.as_deref(), Some("v1alpha"));
    assert_eq!(headers_of(&config)["custom-header"], "value");
    assert!(headers_of(&config)["user-agent"].contains("+afc"));
    assert!(headers_of(&config)["x-goog-api-client"].contains("+afc"));
}

// upstream-test: afc/test_get_usage_header.py::test_get_usage_header_with_generate_images_config
#[test]
fn test_get_usage_header_with_generate_images_config() {
    let config = get_usage_header(Some(GenerateImagesConfig::default()), "image");
    let expected = format!("{}+image", label());
    let headers = config
        .http_options
        .as_ref()
        .and_then(|options| options.headers.as_ref())
        .expect("headers are set");
    assert_eq!(headers["user-agent"], expected);
    assert_eq!(headers["x-goog-api-client"], expected);
}

// upstream-test: afc/test_get_usage_header.py::test_get_usage_header_dict_generate_images_config
#[test]
fn test_get_usage_header_dict_generate_images_config() {
    let config = get_usage_header(
        Some(GenerateImagesConfig {
            negative_prompt: Some("blue".to_owned()),
            ..Default::default()
        }),
        "image",
    );
    let expected = format!("{}+image", label());
    let headers = config
        .http_options
        .as_ref()
        .and_then(|options| options.headers.as_ref())
        .expect("headers are set");
    assert_eq!(config.negative_prompt.as_deref(), Some("blue"));
    assert_eq!(headers["user-agent"], expected);
    assert_eq!(headers["x-goog-api-client"], expected);
}
