//! Ports of `client/test_http_options.py`: merging client-level and
//! per-request `HttpOptions`, and the `X-Server-Timeout` header.
//!
//! Python inspects the merged options object (`patch_http_options`) and the
//! built request (`_build_request`). Neither is public in Rust, so each test
//! checks what the server receives. `client_args`, `async_client_args` and
//! `base_url_resource_scope` (httpx / Vertex AI options) do not exist on the
//! Rust `HttpOptions`.

use std::collections::HashMap;

use gemini_genai::{
    Client,
    types::{HttpOptions, HttpRetryOptions},
};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, method, path_regex},
};

use crate::support::{MODEL, get_model, header_value, model_server, received};

fn header_map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn client_with(server: &MockServer, options: HttpOptions) -> Client {
    Client::builder()
        .api_key("test_api_key")
        .http_options(HttpOptions {
            base_url: Some(server.uri()),
            ..options
        })
        .build()
        .unwrap()
}

// upstream-test: client/test_http_options.py::test_patch_http_options_with_copies_all_fields
#[tokio::test]
async fn test_patch_http_options_with_copies_all_fields() {
    // A client with default options plus a per-request override of every
    // field the crate supports: each must take effect on the request.
    let default_server = MockServer::start().await;
    let patch_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/models/.+:generateContent$"))
        .and(body_partial_json(json!({"key": "value"})))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&patch_server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/models/.+:generateContent$"))
        .and(body_partial_json(json!({"key": "value"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{"content": {"role": "model", "parts": [{"text": "ok"}]}}]
        })))
        .mount(&patch_server)
        .await;

    let client = client_with(&default_server, HttpOptions::default());
    let patch = HttpOptions {
        base_url: Some(patch_server.uri()),
        api_version: Some("v1".to_owned()),
        headers: Some(header_map(&[("X-Custom-Header", "custom_value")])),
        timeout: Some(10_000),
        extra_body: json!({"key": "value"}).as_object().cloned(),
        retry_options: Some(HttpRetryOptions {
            attempts: Some(10),
            initial_delay: Some(0.0),
            max_delay: Some(0.001),
            ..Default::default()
        }),
    };
    let config = gemini_genai::types::GenerateContentConfig {
        http_options: Some(patch),
        ..Default::default()
    };
    let response = client
        .models()
        .generate_content(MODEL, "hi", Some(config))
        .await
        .unwrap();
    assert_eq!(response.text().as_deref(), Some("ok"));

    assert!(
        received(&default_server).await.is_empty(),
        "base_url must be overridden"
    );
    let requests = received(&patch_server).await;
    assert_eq!(requests.len(), 2, "retry_options: one 503, then success");
    for request in &requests {
        assert_eq!(
            request.url.path(),
            format!("/v1/models/{MODEL}:generateContent")
        );
        assert_eq!(
            header_value(request, "X-Custom-Header").as_deref(),
            Some("custom_value")
        );
        assert_eq!(
            header_value(request, "X-Server-Timeout").as_deref(),
            Some("10")
        );
    }
}

// upstream-test: client/test_http_options.py::test_patch_http_options_merges_headers
#[tokio::test]
async fn test_patch_http_options_merges_headers() {
    let default_server = MockServer::start().await;
    let patch_server = model_server().await;
    let client = client_with(
        &default_server,
        HttpOptions {
            headers: Some(header_map(&[
                ("X-Custom-Header", "different_value"),
                ("X-different-header", "different_value"),
            ])),
            ..Default::default()
        },
    );
    let patch = HttpOptions {
        base_url: Some(patch_server.uri()),
        api_version: Some("v1".to_owned()),
        headers: Some(header_map(&[("X-Custom-Header", "custom_value")])),
        timeout: Some(10_000),
        ..Default::default()
    };

    get_model(&client, Some(patch)).await.unwrap();

    let requests = received(&patch_server).await;
    assert_eq!(requests.len(), 1);
    // A header present in both takes the per-request value; the rest merge.
    assert_eq!(
        header_value(&requests[0], "X-Custom-Header").as_deref(),
        Some("custom_value")
    );
    assert_eq!(
        header_value(&requests[0], "X-different-header").as_deref(),
        Some("different_value")
    );
    assert_eq!(requests[0].url.path(), format!("/v1/models/{MODEL}"));
    assert_eq!(
        header_value(&requests[0], "X-Server-Timeout").as_deref(),
        Some("10")
    );
}

// upstream-test: client/test_http_options.py::test_patch_http_options_appends_version_headers
#[tokio::test]
async fn test_patch_http_options_appends_version_headers() {
    let server = model_server().await;
    let client = client_with(
        &server,
        HttpOptions {
            headers: Some(header_map(&[("X-Custom-Header", "different_value")])),
            ..Default::default()
        },
    );
    let patch = HttpOptions {
        headers: Some(header_map(&[("X-Custom-Header", "custom_value")])),
        ..Default::default()
    };

    get_model(&client, Some(patch)).await.unwrap();

    let requests = received(&server).await;
    assert!(header_value(&requests[0], "user-agent").is_some());
    assert!(header_value(&requests[0], "x-goog-api-client").is_some());
}

/// Sends `models().get` and returns the `X-Server-Timeout` header, if any.
async fn server_timeout_header(
    client_options: HttpOptions,
    per_request: Option<HttpOptions>,
) -> Option<String> {
    let server = model_server().await;
    let client = client_with(&server, client_options);
    get_model(&client, per_request).await.unwrap();
    let requests = received(&server).await;
    header_value(&requests[0], "X-Server-Timeout")
}

// upstream-test: client/test_http_options.py::test_setting_timeout_populates_server_timeout_header
#[tokio::test]
async fn test_setting_timeout_populates_server_timeout_header() {
    let header = server_timeout_header(
        HttpOptions {
            timeout: Some(10_000),
            ..Default::default()
        },
        None,
    )
    .await;
    assert_eq!(header.as_deref(), Some("10"));
}

// upstream-test: client/test_http_options.py::test_timeout_rounded_to_nearest_second
#[tokio::test]
async fn test_timeout_rounded_to_nearest_second() {
    // 7.3 s becomes 8: the header is rounded *up* (Python's `math.ceil`).
    let header = server_timeout_header(
        HttpOptions::default(),
        Some(HttpOptions {
            timeout: Some(7300),
            ..Default::default()
        }),
    )
    .await;
    assert_eq!(header.as_deref(), Some("8"));
}

// upstream-test: client/test_http_options.py::test_server_timeout_not_overwritten
#[tokio::test]
async fn test_server_timeout_not_overwritten() {
    let header = server_timeout_header(
        HttpOptions::default(),
        Some(HttpOptions {
            headers: Some(header_map(&[("X-Server-Timeout", "3")])),
            timeout: Some(11_000),
            ..Default::default()
        }),
    )
    .await;
    assert_eq!(header.as_deref(), Some("3"));
}

// upstream-test: client/test_http_options.py::test_server_timeout_not_set_by_default
#[tokio::test]
async fn test_server_timeout_not_set_by_default() {
    let header = server_timeout_header(HttpOptions::default(), None).await;
    assert_eq!(header, None);
}

// upstream-test: client/test_http_options.py::test_retry_options_not_set_by_default
#[test]
fn test_retry_options_not_set_by_default() {
    assert!(HttpOptions::default().retry_options.is_none());
}
