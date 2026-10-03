//! Ports of `client/test_client_requests.py`: how a base URL and path are
//! joined, and the SDK identification headers.
//!
//! Python calls `join_url_path` and `_build_request` directly. Their Rust
//! counterpart (`HttpClient::build_url` / `merged_headers`) is crate-private,
//! so each test checks the request a mock server actually receives.

use gemini_genai::{Client, types::HttpOptions};
use wiremock::MockServer;

use crate::support::{MODEL, get_model, header_value, model_server, received};

/// Sends one `models().get` through a client whose base URL is `base_url`
/// with no API-version segment, and returns the request path that arrived.
async fn path_for_base_url(server: &MockServer, base_url: String) -> String {
    let client = Client::builder()
        .api_key("test-key")
        .http_options(HttpOptions {
            base_url: Some(base_url),
            api_version: Some(String::new()),
            ..Default::default()
        })
        .build()
        .unwrap();
    get_model(&client, None).await.unwrap();
    let requests = received(server).await;
    assert_eq!(requests.len(), 1);
    requests[0].url.path().to_owned()
}

// upstream-test: client/test_client_requests.py::test_join_url_path_with_base_url_with_trailing_slash_and_path_without_leading_slash
#[tokio::test]
async fn test_join_url_path_with_base_url_with_trailing_slash_and_path_without_leading_slash() {
    let server = model_server().await;
    let path = path_for_base_url(&server, format!("{}/some_path/", server.uri())).await;
    assert_eq!(path, format!("/some_path/models/{MODEL}"));
}

// upstream-test: client/test_client_requests.py::test_join_url_path_with_base_url_without_trailing_slash_and_path_without_leading_slash
#[tokio::test]
async fn test_join_url_path_with_base_url_without_trailing_slash_and_path_without_leading_slash() {
    let server = model_server().await;
    let path = path_for_base_url(&server, format!("{}/some_path", server.uri())).await;
    assert_eq!(path, format!("/some_path/models/{MODEL}"));
}

// upstream-test: client/test_client_requests.py::test_join_url_path_base_url_without_path_with_trailing_slash
#[tokio::test]
async fn test_join_url_path_base_url_without_path_with_trailing_slash() {
    let server = model_server().await;
    let path = path_for_base_url(&server, format!("{}/", server.uri())).await;
    assert_eq!(path, format!("/models/{MODEL}"));
}

// upstream-test: client/test_client_requests.py::test_join_url_path_base_url_without_path_without_trailing_slash
#[tokio::test]
async fn test_join_url_path_base_url_without_path_without_trailing_slash() {
    let server = model_server().await;
    let path = path_for_base_url(&server, server.uri()).await;
    assert_eq!(path, format!("/models/{MODEL}"));
}

/// Issues a request with `headers` as the client-level custom headers and
/// returns the `(user-agent, x-goog-api-client)` values the server saw.
async fn sdk_headers_sent(server: &MockServer, headers: &[(&str, &str)]) -> (String, String) {
    let client = Client::builder()
        .api_key("test-key")
        .http_options(HttpOptions {
            base_url: Some(server.uri()),
            api_version: Some("1".to_owned()),
            headers: Some(
                headers
                    .iter()
                    .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                    .collect(),
            ),
            ..Default::default()
        })
        .build()
        .unwrap();
    get_model(&client, None).await.unwrap();
    let requests = received(server).await;
    let request = requests.last().unwrap();
    (
        header_value(request, "user-agent").unwrap(),
        header_value(request, "x-goog-api-client").unwrap(),
    )
}

/// The label this port sends in place of Python's `google-genai-sdk/`
/// (see `src/api_client/headers.rs`: an unofficial port must not impersonate
/// the upstream SDK) and the Rust counterpart of `gl-python/`.
const LIBRARY_LABEL: &str = "gemini-genai/";
const LANGUAGE_LABEL: &str = "gl-rust/";

// upstream-test: client/test_client_requests.py::test_build_request_sets_library_version_headers
#[tokio::test]
async fn test_build_request_sets_library_version_headers() {
    let server = model_server().await;
    let (user_agent, api_client) = sdk_headers_sent(&server, &[]).await;
    for value in [&user_agent, &api_client] {
        assert!(
            value.contains(LIBRARY_LABEL),
            "missing library label: {value}"
        );
        assert!(
            value.contains(LANGUAGE_LABEL),
            "missing language label: {value}"
        );
    }
}

// upstream-test: client/test_client_requests.py::test_build_request_appends_to_user_agent_headers
#[tokio::test]
async fn test_build_request_appends_to_user_agent_headers() {
    let server = model_server().await;
    let (user_agent, api_client) =
        sdk_headers_sent(&server, &[("user-agent", "test-user-agent")]).await;
    assert!(user_agent.contains("test-user-agent"));
    assert!(user_agent.contains(LIBRARY_LABEL));
    assert!(user_agent.contains(LANGUAGE_LABEL));
    assert!(api_client.contains(LIBRARY_LABEL));
}

// upstream-test: client/test_client_requests.py::test_build_request_appends_to_goog_api_client_headers
#[tokio::test]
async fn test_build_request_appends_to_goog_api_client_headers() {
    let server = model_server().await;
    let (user_agent, api_client) =
        sdk_headers_sent(&server, &[("x-goog-api-client", "test-goog-api-client")]).await;
    assert!(user_agent.contains(LIBRARY_LABEL));
    assert!(api_client.contains("test-goog-api-client"));
    assert!(api_client.contains(LIBRARY_LABEL));
    assert!(api_client.contains(LANGUAGE_LABEL));
}

// upstream-test: client/test_client_requests.py::test_build_request_keeps_sdk_version_headers
#[tokio::test]
async fn test_build_request_keeps_sdk_version_headers() {
    // Python pre-populates the headers with `append_library_version_headers`
    // and checks the labels survive (without being added twice). Feed the
    // headers one request produced into a second client to get the same
    // starting point.
    let server = model_server().await;
    let (first_user_agent, first_api_client) = sdk_headers_sent(&server, &[]).await;
    let (user_agent, api_client) = sdk_headers_sent(
        &server,
        &[
            ("user-agent", first_user_agent.as_str()),
            ("x-goog-api-client", first_api_client.as_str()),
        ],
    )
    .await;
    for value in [&user_agent, &api_client] {
        assert!(value.contains(LIBRARY_LABEL));
        assert!(value.contains(LANGUAGE_LABEL));
        assert_eq!(
            value.matches(LIBRARY_LABEL).count(),
            1,
            "the library label must not be appended twice: {value}"
        );
    }
}

/// Python's `EphemeralTokenAPIKeyError`: an `auth_tokens/...` key is only
/// valid on the Live API, so any other request is refused before it is sent.
#[tokio::test]
async fn ephemeral_token_is_rejected_outside_the_live_api() {
    let server = model_server().await;
    let client = Client::builder()
        .api_key("auth_tokens/abc123")
        .http_options(HttpOptions {
            base_url: Some(server.uri()),
            ..Default::default()
        })
        .build()
        .unwrap();

    let result = get_model(&client, None).await;

    assert!(
        matches!(&result, Err(gemini_genai::Error::Validation(message))
            if message.contains("only be used with the live API")),
        "got {result:?}"
    );
    assert!(received(&server).await.is_empty(), "nothing is sent");
}
