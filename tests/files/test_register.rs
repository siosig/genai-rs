//! Ports of `google/genai/tests/files/test_register.py`.
//!
//! The upstream tests that pass `auth=FakeCredentials(...)` (bearer token,
//! token refresh, quota project) exercise `google.auth` credentials objects,
//! which `Files::register_files` deliberately does not take; they are
//! excluded as `python_only`. The sync/async pairs both map to the one
//! async Rust method.

use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path},
};

use super::test_client;

/// Registers `uris`, with the server answering `response_files`, and
/// returns the file URIs the client parsed.
async fn register(
    uris: &[&str],
    response_files: serde_json::Value,
) -> Result<Vec<Option<String>>, Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/files:register"))
        .and(body_json(serde_json::json!({ "uris": uris })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"files": response_files})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let response = test_client(server.uri())
        .files()
        .register_files(uris.iter().map(|u| (*u).to_owned()).collect(), None)
        .await?;
    server.verify().await;
    Ok(response
        .files
        .unwrap_or_default()
        .into_iter()
        .map(|f| f.uri)
        .collect())
}

// upstream-test: files/test_register.py::test_multiple_uris
#[tokio::test]
async fn test_multiple_uris() -> Result<(), Box<dyn std::error::Error>> {
    let uris = register(
        &[
            "gs://test-bucket/test-file-1.txt",
            "gs://test-bucket/test-file-2.txt",
        ],
        serde_json::json!([{"uri": "files/abc"}, {"uri": "files/def"}]),
    )
    .await?;
    assert_eq!(
        uris,
        [Some("files/abc".to_owned()), Some("files/def".to_owned())]
    );
    Ok(())
}

// upstream-test: files/test_register.py::test_async_single
#[tokio::test]
async fn test_async_single() -> Result<(), Box<dyn std::error::Error>> {
    let uris = register(
        &["gs://test-bucket/test-file-1.txt"],
        serde_json::json!([{"uri": "files/abc"}]),
    )
    .await?;
    assert_eq!(uris, [Some("files/abc".to_owned())]);
    Ok(())
}

// upstream-test: files/test_register.py::test_async_multiple_uris
#[tokio::test]
async fn test_async_multiple_uris() -> Result<(), Box<dyn std::error::Error>> {
    let uris = register(
        &[
            "gs://test-bucket/test-file-1.txt",
            "gs://test-bucket/test-file-2.txt",
        ],
        serde_json::json!([{"uri": "files/abc"}, {"uri": "files/def"}]),
    )
    .await?;
    assert_eq!(
        uris,
        [Some("files/abc".to_owned()), Some("files/def".to_owned())]
    );
    Ok(())
}
