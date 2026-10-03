//! Ports of `client/test_upload_errors.py`: the resumable-upload URL is
//! rewritten onto the configured base URL, and a rejection of the final
//! chunk surfaces as the server's API error.
//!
//! Python's sync tests exercise the blocking `_upload_fd`; they map to the
//! `blocking` client here. The `async` tests use the async client. The
//! aiohttp variant has no Rust counterpart (the crate uses one `reqwest`
//! transport).

use gemini_genai::{Error, files::UploadSource};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, headers, method, path, query_param},
};

use crate::common::test_client;

const GOOGLE_UPLOAD_URL: &str =
    "https://generativelanguage.googleapis.com/upload/v1beta/files?uploadType=resumable";
const UPLOAD_COMMAND: &str = "X-Goog-Upload-Command";
const UPLOAD_STATUS: &str = "x-goog-upload-status";
const UPLOAD_URL: &str = "X-Goog-Upload-URL";
const ERROR_MESSAGE: &str = "Unsupported MIME type: bad/mime_type";

fn source() -> UploadSource {
    UploadSource::Bytes {
        data: b"test".to_vec(),
        mime_type: "text/plain".to_owned(),
    }
}

/// Mounts the upload "start" step, answering with `upload_url`.
async fn mount_start(server: &MockServer, upload_url: &str) {
    Mock::given(method("POST"))
        .and(path("/upload/v1beta/files"))
        .and(header(UPLOAD_COMMAND, "start"))
        .respond_with(ResponseTemplate::new(200).insert_header(UPLOAD_URL, upload_url))
        .expect(1)
        .mount(server)
        .await;
}

/// Mounts a proxy that serves the whole flow on one host: the start step
/// returns the *Google* upload URL, so the chunk only reaches the proxy if
/// the URL was rewritten.
async fn mount_rewritten_flow(server: &MockServer) {
    mount_start(server, GOOGLE_UPLOAD_URL).await;
    Mock::given(method("POST"))
        .and(path("/upload/v1beta/files"))
        .and(query_param("uploadType", "resumable"))
        .and(headers(UPLOAD_COMMAND, vec!["upload", "finalize"]))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header(UPLOAD_STATUS, "final")
                .set_body_json(json!({"file": {"name": "files/abc"}})),
        )
        .expect(1)
        .mount(server)
        .await;
}

/// Mounts a flow whose final chunk is rejected with a 400 error body.
async fn mount_rejected_flow(server: &MockServer) {
    mount_start(server, &format!("{}/upload-session", server.uri())).await;
    Mock::given(method("POST"))
        .and(path("/upload-session"))
        .respond_with(
            ResponseTemplate::new(400)
                .insert_header(UPLOAD_STATUS, "final")
                .set_body_json(json!({
                    "error": {
                        "code": 400,
                        "message": ERROR_MESSAGE,
                        "status": "INVALID_ARGUMENT",
                    }
                })),
        )
        .expect(1)
        .mount(server)
        .await;
}

fn assert_mime_type_rejection(result: Result<gemini_genai::types::File, Error>) {
    let Err(Error::Api(error)) = result else {
        panic!("expected an API error, got {result:?}");
    };
    assert_eq!(error.code, 400);
    assert!(
        error.message.contains(ERROR_MESSAGE),
        "unexpected message: {}",
        error.message
    );
}

/// Runs `f` on a plain OS thread with no Tokio runtime context, as the
/// blocking client requires (see `tests/blocking_parity.rs`).
#[cfg(feature = "blocking")]
#[expect(
    clippy::unwrap_used,
    reason = "test helper: a panic on the worker thread is a test failure"
)]
fn run_off_runtime<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(f).join().unwrap()
}

#[cfg(feature = "blocking")]
fn blocking_upload(base_url: String) -> Result<gemini_genai::types::File, Error> {
    run_off_runtime(move || {
        crate::common::blocking_test_client(base_url)
            .files()
            .upload(source(), None)
    })
}

// upstream-test: client/test_upload_errors.py::test_upload_url_rewrite
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_upload_url_rewrite() {
    let server = MockServer::start().await;
    mount_rewritten_flow(&server).await;

    blocking_upload(server.uri()).unwrap();

    server.verify().await;
}

// upstream-test: client/test_upload_errors.py::test_upload_fd_error
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_upload_fd_error() {
    let server = MockServer::start().await;
    mount_rejected_flow(&server).await;

    assert_mime_type_rejection(blocking_upload(server.uri()));

    server.verify().await;
}

// upstream-test: client/test_upload_errors.py::test_async_upload_url_rewrite_httpx
#[tokio::test]
async fn test_async_upload_url_rewrite_httpx() {
    let server = MockServer::start().await;
    mount_rewritten_flow(&server).await;

    test_client(server.uri())
        .files()
        .upload(source(), None)
        .await
        .unwrap();

    server.verify().await;
}

// upstream-test: client/test_upload_errors.py::test_async_upload_fd_error_httpx
#[tokio::test]
async fn test_async_upload_fd_error_httpx() {
    let server = MockServer::start().await;
    mount_rejected_flow(&server).await;

    let result = test_client(server.uri())
        .files()
        .upload(source(), None)
        .await;

    assert_mime_type_rejection(result);
    server.verify().await;
}
