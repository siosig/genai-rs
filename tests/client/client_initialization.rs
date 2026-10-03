//! Ports of `client/test_client_initialization.py`: API-key resolution from
//! the constructor and environment, `http_options` at construction, and base
//! URL precedence.
//!
//! Python reads the resolved values back off `client.models._api_client`;
//! the Rust client keeps them private, so each test observes them where they
//! matter -- on the request that reaches a mock server.

use gemini_genai::{Client, Error, base_url::set_default_base_urls, types::HttpOptions};
use wiremock::MockServer;

use crate::{
    env_guard::{GEMINI_API_KEY, GOOGLE_API_KEY, GOOGLE_GEMINI_BASE_URL, with_env},
    support::{MODEL, get_model, header_value, model_server, received},
};

const API_KEY_HEADER: &str = "x-goog-api-key";

fn options_for(server: &MockServer) -> HttpOptions {
    HttpOptions {
        base_url: Some(server.uri()),
        ..Default::default()
    }
}

/// Builds a client (with `env` set and, if given, an explicit key) against a
/// fresh mock server, makes one request, and returns the `x-goog-api-key`
/// header it carried.
async fn api_key_sent(env: &[(&str, &str)], explicit_key: Option<&str>) -> Result<String, Error> {
    let server = model_server().await;
    let options = options_for(&server);
    let client = with_env(env, || {
        let builder = Client::builder().http_options(options);
        match explicit_key {
            Some(key) => builder.api_key(key).build(),
            None => builder.build(),
        }
    })?;
    get_model(&client, None).await?;
    let requests = received(&server).await;
    assert_eq!(requests.len(), 1, "exactly one request is expected");
    Ok(header_value(&requests[0], API_KEY_HEADER).unwrap_or_default())
}

// upstream-test: client/test_client_initialization.py::test_ml_dev_from_gemini_env_only
#[tokio::test]
async fn test_ml_dev_from_gemini_env_only() {
    let key = api_key_sent(&[(GEMINI_API_KEY, "gemini_api_key")], None)
        .await
        .unwrap();
    assert_eq!(key, "gemini_api_key");
}

// upstream-test: client/test_client_initialization.py::test_ml_dev_from_gemini_env_with_google_env_empty
#[tokio::test]
async fn test_ml_dev_from_gemini_env_with_google_env_empty() {
    let key = api_key_sent(
        &[(GEMINI_API_KEY, "gemini_api_key"), (GOOGLE_API_KEY, "")],
        None,
    )
    .await
    .unwrap();
    assert_eq!(key, "gemini_api_key");
}

// upstream-test: client/test_client_initialization.py::test_ml_dev_from_google_env_only
#[tokio::test]
async fn test_ml_dev_from_google_env_only() {
    let key = api_key_sent(&[(GOOGLE_API_KEY, "google_api_key")], None)
        .await
        .unwrap();
    assert_eq!(key, "google_api_key");
}

// upstream-test: client/test_client_initialization.py::test_ml_dev_both_env_key_set
#[tokio::test]
async fn test_ml_dev_both_env_key_set() {
    // Python also asserts the "Both GOOGLE_API_KEY and GEMINI_API_KEY are
    // set" log line via `caplog`; the crate emits it through `tracing`, which
    // this suite does not capture, so only the precedence is checked.
    let key = api_key_sent(
        &[
            (GOOGLE_API_KEY, "google_api_key"),
            (GEMINI_API_KEY, "gemini_api_key"),
        ],
        None,
    )
    .await
    .unwrap();
    assert_eq!(key, "google_api_key");
}

// upstream-test: client/test_client_initialization.py::test_api_key_with_new_line
#[tokio::test]
async fn test_api_key_with_new_line() {
    let key = api_key_sent(&[(GOOGLE_API_KEY, "gemini_api_key\r\n")], None)
        .await
        .unwrap();
    assert_eq!(key, "gemini_api_key", "surrounding whitespace is stripped");
}

// upstream-test: client/test_client_initialization.py::test_ml_dev_from_constructor
#[tokio::test]
async fn test_ml_dev_from_constructor() {
    let key = api_key_sent(&[], Some("google_api_key")).await.unwrap();
    assert_eq!(key, "google_api_key");
}

// upstream-test: client/test_client_initialization.py::test_mldev_explicit_arg_precedence
#[tokio::test]
async fn test_mldev_explicit_arg_precedence() {
    let key = api_key_sent(
        &[
            (GOOGLE_API_KEY, "google_env_api_key"),
            (GEMINI_API_KEY, "gemini_env_api_key"),
        ],
        Some("constructor_api_key"),
    )
    .await
    .unwrap();
    assert_eq!(key, "constructor_api_key");
}

// upstream-test: client/test_client_initialization.py::test_invalid_mldev_constructor_empty
#[test]
fn test_invalid_mldev_constructor_empty() {
    let result = with_env(&[(GOOGLE_API_KEY, ""), (GEMINI_API_KEY, "")], Client::new);
    assert!(
        matches!(result, Err(Error::Validation(_))),
        "empty API-key variables must not count as a key, got {result:?}"
    );
}

// upstream-test: client/test_client_initialization.py::test_invalid_mldev_constructor
#[test]
fn test_invalid_mldev_constructor() {
    // Python raises ValueError for project/location on the Gemini Developer
    // API. The crate rejects them the same way it rejects every Vertex AI
    // request: `Error::UnsupportedBackend` (documented in
    // docs/upstream-sync.md).
    let result = with_env(&[], || {
        Client::builder()
            .project("fake_project_id")
            .location("fake-location")
            .api_key("fake-api_key")
            .build()
    });
    assert!(
        matches!(result, Err(Error::UnsupportedBackend(_))),
        "project/location must be rejected, got {result:?}"
    );
}

// upstream-test: client/test_client_initialization.py::test_gemini_project_location_invalid
#[test]
fn test_gemini_project_location_invalid() {
    // Python's message is "Gemini API does not support project/location.";
    // here the error is the crate-wide `UnsupportedBackend` (see
    // `test_invalid_mldev_constructor`).
    let result = with_env(&[(GOOGLE_API_KEY, "test_key")], || {
        Client::builder()
            .project("fake_project_id")
            .vertexai(false)
            .build()
    });
    assert!(
        matches!(result, Err(Error::UnsupportedBackend(_))),
        "project with vertexai(false) must be rejected, got {result:?}"
    );
}

// upstream-test: client/test_client_initialization.py::test_constructor_with_http_options
#[tokio::test]
async fn test_constructor_with_http_options() {
    // Only the Gemini Developer API half: the Vertex AI client the Python
    // test also builds is out of scope. `get_read_only_http_options()` has no
    // counterpart, so each option is checked on the request it shapes.
    let server = model_server().await;
    let options = HttpOptions {
        api_version: Some("v1main".to_owned()),
        base_url: Some(server.uri()),
        headers: Some(
            [(
                "X-Custom-Header".to_owned(),
                "custom_value_mldev".to_owned(),
            )]
            .into(),
        ),
        timeout: Some(10_000),
        ..Default::default()
    };
    let client = with_env(&[], || {
        Client::builder()
            .api_key("google_api_key")
            .http_options(options)
            .build()
    })
    .unwrap();

    get_model(&client, None).await.unwrap();

    let requests = received(&server).await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].url.path(),
        format!("/v1main/models/{MODEL}"),
        "api_version"
    );
    assert_eq!(
        header_value(&requests[0], "X-Custom-Header").as_deref(),
        Some("custom_value_mldev")
    );
    assert_eq!(
        header_value(&requests[0], "X-Server-Timeout").as_deref(),
        Some("10"),
        "timeout 10000 ms"
    );
}

// upstream-test: client/test_client_initialization.py::test_constructor_with_http_options_as_pydantic_type
#[tokio::test]
async fn test_constructor_with_http_options_as_pydantic_type() {
    // The typed-options half of the Python test (the Vertex AI half is out of
    // scope): `HttpOptions` is the only form Rust has, so this is the
    // construction path every caller uses.
    let server = model_server().await;
    let options = HttpOptions {
        api_version: Some("v1".to_owned()),
        base_url: Some(server.uri()),
        headers: Some([("X-Custom-Header".to_owned(), "custom_value".to_owned())].into()),
        ..Default::default()
    };
    let client = with_env(&[], || {
        Client::builder()
            .api_key("google_api_key")
            .http_options(options)
            .build()
    })
    .unwrap();

    get_model(&client, None).await.unwrap();

    let requests = received(&server).await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url.path(), format!("/v1/models/{MODEL}"));
    assert_eq!(
        header_value(&requests[0], "X-Custom-Header").as_deref(),
        Some("custom_value")
    );
}

/// An address nothing listens on: a request that reaches it fails, so a test
/// that expects the *other* URL to win fails loudly if precedence is wrong.
const UNUSED_BASE_URL: &str = "http://127.0.0.1:1/";

async fn assert_single_request_on(server: &MockServer, client: &Client) {
    get_model(client, None).await.unwrap();
    assert_eq!(received(server).await.len(), 1);
}

// upstream-test: client/test_client_initialization.py::test_constructor_with_base_url_from_http_options
#[tokio::test]
async fn test_constructor_with_base_url_from_http_options() {
    let server = model_server().await;
    let options = options_for(&server);
    let client = with_env(&[], || {
        Client::builder()
            .api_key("google_api_key")
            .http_options(options)
            .build()
    })
    .unwrap();
    assert_single_request_on(&server, &client).await;
}

// upstream-test: client/test_client_initialization.py::test_constructor_with_base_url_from_set_default_base_urls
#[tokio::test]
async fn test_constructor_with_base_url_from_set_default_base_urls() {
    let server = model_server().await;
    let uri = server.uri();
    let client = with_env(&[], || {
        set_default_base_urls(Some(uri));
        Client::builder().api_key("google_api_key").build()
    })
    .unwrap();
    assert_single_request_on(&server, &client).await;
}

// upstream-test: client/test_client_initialization.py::test_constructor_with_constructor_base_url_overrides_set_default_base_urls
#[tokio::test]
async fn test_constructor_with_constructor_base_url_overrides_set_default_base_urls() {
    let server = model_server().await;
    let options = options_for(&server);
    let client = with_env(&[], || {
        set_default_base_urls(Some(UNUSED_BASE_URL.to_owned()));
        Client::builder()
            .api_key("google_api_key")
            .http_options(options)
            .build()
    })
    .unwrap();
    assert_single_request_on(&server, &client).await;
}

// upstream-test: client/test_client_initialization.py::test_constructor_with_constructor_base_url_overrides_environment_variables
#[tokio::test]
async fn test_constructor_with_constructor_base_url_overrides_environment_variables() {
    let server = model_server().await;
    let options = options_for(&server);
    let client = with_env(&[(GOOGLE_GEMINI_BASE_URL, UNUSED_BASE_URL)], || {
        Client::builder()
            .api_key("google_api_key")
            .http_options(options)
            .build()
    })
    .unwrap();
    assert_single_request_on(&server, &client).await;
}

// upstream-test: client/test_client_initialization.py::test_constructor_with_base_url_from_set_default_base_urls_overrides_environment_variables
#[tokio::test]
async fn test_constructor_with_base_url_from_set_default_base_urls_overrides_environment_variables()
{
    let server = model_server().await;
    let uri = server.uri();
    let client = with_env(&[(GOOGLE_GEMINI_BASE_URL, UNUSED_BASE_URL)], || {
        set_default_base_urls(Some(uri));
        Client::builder().api_key("google_api_key").build()
    })
    .unwrap();
    assert_single_request_on(&server, &client).await;
}

// upstream-test: client/test_client_initialization.py::test_constructor_with_base_url_from_environment_variables
#[tokio::test]
async fn test_constructor_with_base_url_from_environment_variables() {
    let server = model_server().await;
    let uri = server.uri();
    let client = with_env(&[(GOOGLE_GEMINI_BASE_URL, uri.as_str())], || {
        Client::builder().api_key("google_api_key").build()
    })
    .unwrap();
    assert_single_request_on(&server, &client).await;
}
