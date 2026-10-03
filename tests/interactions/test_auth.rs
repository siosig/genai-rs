//! Ported from `google/genai/tests/interactions/test_auth.py` (Developer API
//! tests; the Vertex AI auth tests have no Rust counterpart). The Rust client is
//! async-only, so the upstream sync and async variants both drive the one async API.
//!
//! Upstream resolves the key from `GOOGLE_API_KEY`; these tests pass it
//! explicitly instead of mutating the process environment.

use gemini_genai::{
    Client,
    types::{HttpOptions, HttpRetryOptions},
};
use std::collections::HashMap;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};

use crate::{
    common::test_client_with_api_key,
    recording::{completed, create_hello, header, received, server_answering},
};

const API_KEY: &str = "test-api-key";

async fn check_gemini_url() {
    let server = server_answering(ResponseTemplate::new(200).set_body_json(completed())).await;
    let client = test_client_with_api_key(server.uri(), API_KEY);

    create_hello(&client).await.unwrap();

    let requests = received(&server).await;
    assert_eq!(requests.len(), 1);
    assert!(requests[0].url.path().ends_with("/v1beta/interactions"));
    assert_eq!(header(&requests[0], "x-goog-api-key"), Some(API_KEY));
}

async fn check_gemini_retry() {
    let server = MockServer::start().await;
    // Python: `max_retries = 2` after the first attempt, so three attempts.
    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(500)
                .insert_header("retry-after-ms", "1")
                .set_body_json(completed()),
        )
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(completed()))
        .with_priority(2)
        .mount(&server)
        .await;
    let client = Client::builder()
        .api_key(API_KEY)
        .http_options(HttpOptions {
            base_url: Some(server.uri()),
            retry_options: Some(HttpRetryOptions {
                attempts: Some(3),
                initial_delay: Some(0.001),
                max_delay: Some(0.01),
                jitter: Some(0.0),
                ..Default::default()
            }),
            ..Default::default()
        })
        .build()
        .unwrap();

    create_hello(&client).await.unwrap();

    assert_eq!(received(&server).await.len(), 3);
}

async fn check_gemini_extra_headers() {
    let server = server_answering(ResponseTemplate::new(200).set_body_json(completed())).await;
    let client = test_client_with_api_key(server.uri(), API_KEY);

    client
        .interactions()
        .with_http_options(HttpOptions {
            headers: Some(HashMap::from([(
                "X-Custom-Header".to_owned(),
                "TestValue".to_owned(),
            )])),
            ..Default::default()
        })
        .create(&crate::recording::hello_body())
        .await
        .unwrap();

    let requests = received(&server).await;
    assert_eq!(requests.len(), 1);
    assert_eq!(header(&requests[0], "x-custom-header"), Some("TestValue"));
    assert_eq!(header(&requests[0], "x-goog-api-key"), Some(API_KEY));
}

// upstream-test: interactions/test_auth.py::test_interactions_gemini_url
#[tokio::test]
async fn test_interactions_gemini_url() {
    check_gemini_url().await;
}

// upstream-test: interactions/test_auth.py::test_interactions_gemini_retry
#[tokio::test]
async fn test_interactions_gemini_retry() {
    check_gemini_retry().await;
}

// upstream-test: interactions/test_auth.py::test_interactions_gemini_extra_headers
#[tokio::test]
async fn test_interactions_gemini_extra_headers() {
    check_gemini_extra_headers().await;
}

// upstream-test: interactions/test_auth.py::test_async_interactions_gemini_url
#[tokio::test]
async fn test_async_interactions_gemini_url() {
    check_gemini_url().await;
}

// upstream-test: interactions/test_auth.py::test_async_interactions_gemini_retry
#[tokio::test]
async fn test_async_interactions_gemini_retry() {
    check_gemini_retry().await;
}

// upstream-test: interactions/test_auth.py::test_async_interactions_gemini_extra_headers
#[tokio::test]
async fn test_async_interactions_gemini_extra_headers() {
    check_gemini_extra_headers().await;
}
