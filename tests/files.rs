//! Integration tests for `client.files()`: upload (resumable protocol,
//! multi-chunk), get, list (Pager), delete, and download.

mod common;

use common::test_client;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, header, headers, method, path, query_param, query_param_is_missing},
};

/// Writes `data` to a fresh temp file and returns its path. No `tempfile`
/// dependency is available, so tests remove the file themselves once done.
#[expect(
    clippy::unwrap_used,
    reason = "test helper: a failed temp-file write means the test environment is broken, not the code under test"
)]
async fn write_temp_file(name: &str, data: &[u8]) -> std::path::PathBuf {
    let mut file_path = std::env::temp_dir();
    file_path.push(format!(
        "gemini-genai-files-test-{name}-{}",
        uuid::Uuid::new_v4()
    ));
    tokio::fs::write(&file_path, data).await.unwrap();
    file_path
}

#[tokio::test]
async fn upload_from_a_temp_file_sends_resumable_start_headers() {
    let server = MockServer::start().await;
    let data = b"hello from disk".to_vec();
    let file_path = write_temp_file("small", &data).await;

    let upload_url = format!("{}/upload-session/small", server.uri());
    Mock::given(method("POST"))
        .and(path("/upload/v1beta/files"))
        .and(header("X-Goog-Upload-Protocol", "resumable"))
        .and(header("X-Goog-Upload-Command", "start"))
        .and(header(
            "X-Goog-Upload-Header-Content-Length",
            data.len().to_string().as_str(),
        ))
        .respond_with(
            ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", upload_url.as_str()),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/upload-session/small"))
        .and(headers("X-Goog-Upload-Command", vec!["upload", "finalize"]))
        .and(header("X-Goog-Upload-Offset", "0"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-status", "final")
                .set_body_json(serde_json::json!({
                    "file": {"name": "files/small", "mimeType": "text/plain", "sizeBytes": data.len()}
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let file = test_client(server.uri())
        .files()
        .upload(file_path.as_path(), None)
        .await
        .unwrap();
    assert_eq!(file.name.as_deref(), Some("files/small"));

    tokio::fs::remove_file(&file_path).await.ok();
    server.verify().await;
}

#[tokio::test]
async fn upload_a_nine_mebibyte_payload_sends_exactly_two_chunks() {
    let server = MockServer::start().await;
    let size = 9 * 1024 * 1024;
    let data = vec![7u8; size];

    let upload_url = format!("{}/upload-session/big", server.uri());
    Mock::given(method("POST"))
        .and(path("/upload/v1beta/files"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", upload_url.as_str()),
        )
        .expect(1)
        .mount(&server)
        .await;
    // First (non-final) chunk: 8 MiB, offset 0.
    Mock::given(method("POST"))
        .and(path("/upload-session/big"))
        .and(header("X-Goog-Upload-Command", "upload"))
        .and(header("X-Goog-Upload-Offset", "0"))
        .respond_with(ResponseTemplate::new(200).insert_header("x-goog-upload-status", "active"))
        .expect(1)
        .mount(&server)
        .await;
    // Final chunk: remaining 1 MiB, offset 8 MiB.
    Mock::given(method("POST"))
        .and(path("/upload-session/big"))
        .and(headers("X-Goog-Upload-Command", vec!["upload", "finalize"]))
        .and(header(
            "X-Goog-Upload-Offset",
            (8 * 1024 * 1024).to_string().as_str(),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-status", "final")
                .set_body_json(serde_json::json!({"file": {"name": "files/big"}})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let source = gemini_genai::files::UploadSource::Bytes {
        data,
        mime_type: "application/octet-stream".to_owned(),
    };
    let file = test_client(server.uri())
        .files()
        .upload(source, None)
        .await
        .unwrap();
    assert_eq!(file.name.as_deref(), Some("files/big"));
    server.verify().await;
}

#[tokio::test]
async fn upload_bytes_source_never_touches_the_filesystem() {
    let server = MockServer::start().await;
    let upload_url = format!("{}/upload-session/bytes", server.uri());
    Mock::given(method("POST"))
        .and(path("/upload/v1beta/files"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", upload_url.as_str()),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/upload-session/bytes"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-status", "final")
                .set_body_json(
                    serde_json::json!({"file": {"name": "files/bytes", "mimeType": "text/csv"}}),
                ),
        )
        .mount(&server)
        .await;

    let config = gemini_genai::types::UploadFileConfig {
        display_name: Some("my data".to_owned()),
        ..Default::default()
    };
    let source = gemini_genai::files::UploadSource::Bytes {
        data: b"a,b,c\n1,2,3\n".to_vec(),
        mime_type: "text/csv".to_owned(),
    };
    let file = test_client(server.uri())
        .files()
        .upload(source, Some(config))
        .await
        .unwrap();
    assert_eq!(file.name.as_deref(), Some("files/bytes"));
    assert_eq!(file.mime_type.as_deref(), Some("text/csv"));
}

#[tokio::test]
async fn upload_non_active_final_status_is_an_upload_error() {
    let server = MockServer::start().await;
    let upload_url = format!("{}/upload-session/bad", server.uri());
    Mock::given(method("POST"))
        .and(path("/upload/v1beta/files"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", upload_url.as_str()),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/upload-session/bad"))
        .respond_with(ResponseTemplate::new(200).insert_header("x-goog-upload-status", "cancelled"))
        .mount(&server)
        .await;

    let source = gemini_genai::files::UploadSource::Bytes {
        data: b"x".to_vec(),
        mime_type: "text/plain".to_owned(),
    };
    let err = test_client(server.uri())
        .files()
        .upload(source, None)
        .await
        .unwrap_err();
    assert!(matches!(err, gemini_genai::Error::Upload(_)));
}

#[tokio::test]
async fn get_returns_the_files_metadata() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/files/abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "files/abc123",
            "mimeType": "image/png",
            "state": "ACTIVE",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let file = test_client(server.uri())
        .files()
        .get("files/abc123", None)
        .await
        .unwrap();
    assert_eq!(file.name.as_deref(), Some("files/abc123"));
    assert_eq!(file.mime_type.as_deref(), Some("image/png"));
    server.verify().await;
}

#[tokio::test]
async fn list_returns_a_pager_that_fetches_the_next_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/files"))
        .and(query_param("pageSize", "1"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "files": [{"name": "files/one"}],
            "nextPageToken": "tok1",
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1beta/files"))
        .and(query_param("pageSize", "1"))
        .and(query_param("pageToken", "tok1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "files": [{"name": "files/two"}],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let config = gemini_genai::types::ListFilesConfig {
        page_size: Some(1),
        ..Default::default()
    };
    let mut pager = test_client(server.uri())
        .files()
        .list(Some(config))
        .await
        .unwrap();
    assert_eq!(pager.page().len(), 1);
    assert_eq!(pager.page()[0].name.as_deref(), Some("files/one"));

    let second = pager.next_page().await.unwrap();
    assert_eq!(second[0].name.as_deref(), Some("files/two"));

    let err = pager.next_page().await.unwrap_err();
    assert!(matches!(err, gemini_genai::Error::NoMorePages));
    server.verify().await;
}

#[tokio::test]
async fn delete_sends_a_delete_request_to_the_files_name_path() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v1beta/files/todelete"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(1)
        .mount(&server)
        .await;

    test_client(server.uri())
        .files()
        .delete("files/todelete", None)
        .await
        .unwrap();
    server.verify().await;
}

#[tokio::test]
async fn download_requests_alt_media_and_returns_raw_bytes() {
    let server = MockServer::start().await;
    let payload = b"raw generated bytes".to_vec();
    Mock::given(method("GET"))
        .and(path("/v1beta/files/gen123:download"))
        .and(query_param("alt", "media"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.clone()))
        .expect(1)
        .mount(&server)
        .await;

    let bytes = test_client(server.uri())
        .files()
        .download("files/gen123", None)
        .await
        .unwrap();
    assert_eq!(bytes.as_ref(), payload.as_slice());
    server.verify().await;
}

#[tokio::test]
async fn register_files_posts_the_uris_and_parses_the_returned_files() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/files:register"))
        .and(body_json(serde_json::json!({
            "uris": ["gs://bucket/a.txt", "gs://bucket/b.txt"]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "files": [
                {"name": "files/aaa", "mimeType": "text/plain", "sizeBytes": "11"},
                {"name": "files/bbb", "mimeType": "text/plain", "sizeBytes": "22"}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let response = test_client(server.uri())
        .files()
        .register_files(
            vec![
                "gs://bucket/a.txt".to_owned(),
                "gs://bucket/b.txt".to_owned(),
            ],
            None,
        )
        .await
        .unwrap();

    let files = response.files.unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].name.as_deref(), Some("files/aaa"));
    // `sizeBytes` arrives as a JSON *string* per proto3's int64 encoding;
    // the generated types accept both that and a bare number.
    assert_eq!(files[0].size_bytes, Some(11));
    assert_eq!(files[1].size_bytes, Some(22));
    server.verify().await;
}

// ============================================================================
// download_stream / download_to_path (spec 003-upstream-2-23-sync, US2)
// ============================================================================
//
// contracts/download-api.md's acceptance conditions B-1..B-10 map to the
// tests below as: B-1/B-2 -> streamed_content_matches_and_is_delivered_in_
// more_than_one_item, B-3 -> download_to_path_writes_the_same_bytes_the_
// server_sent, B-6 -> download_stream_rejects_a_file_with_no_download_uri_
// before_sending_anything, B-7 -> download_stream_does_not_pre_validate_a_
// bare_name, B-10 -> dropping_the_stream_early_does_not_hang. B-4/B-5 (a
// genuine mid-stream failure after headers succeed) are not covered here:
// wiremock has no supported way to simulate a connection drop or a body
// shorter than its own Content-Length, and a flaky/synthetic approximation
// would be worse than an honest gap -- see the completion report.

use futures_util::StreamExt as _;
use gemini_genai::files::FileSource;
use gemini_genai::types::{File, GeneratedVideo, Video};

/// A payload well past typical loopback TCP segment/window sizes, so
/// `bytes_stream()` reliably yields more than one item -- the same
/// technique SSE's own streaming tests would need if they had a large
/// payload; here the point of the test *is* the chunk count, so the
/// payload is sized for it deliberately rather than incidentally.
fn multi_chunk_payload() -> Vec<u8> {
    (0..5_000_000_u32).map(|i| (i % 251) as u8).collect()
}

#[tokio::test]
async fn streamed_content_matches_and_is_delivered_in_more_than_one_item() {
    let server = MockServer::start().await;
    let payload = multi_chunk_payload();
    Mock::given(method("GET"))
        .and(path("/v1beta/files/big123:download"))
        .and(query_param("alt", "media"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.clone()))
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = test_client(server.uri())
        .files()
        .download_stream("files/big123", None)
        .await
        .unwrap();

    let mut collected = Vec::new();
    let mut item_count = 0usize;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.unwrap();
        assert!(
            chunk.len() < payload.len(),
            "each item should be smaller than the whole body, not the whole body at once"
        );
        collected.extend_from_slice(&chunk);
        item_count += 1;
    }

    assert_eq!(
        collected, payload,
        "concatenated stream must equal the server's bytes"
    );
    assert!(
        item_count > 1,
        "expected more than one stream item for a {}-byte body, got {item_count}",
        payload.len()
    );
    server.verify().await;
}

#[tokio::test]
async fn download_to_path_writes_the_same_bytes_the_server_sent() {
    let server = MockServer::start().await;
    let payload = multi_chunk_payload();
    Mock::given(method("GET"))
        .and(path("/v1beta/files/path123:download"))
        .and(query_param("alt", "media"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.clone()))
        .expect(1)
        .mount(&server)
        .await;

    let mut destination = std::env::temp_dir();
    destination.push(format!("gemini-genai-download-{}", uuid::Uuid::new_v4()));

    test_client(server.uri())
        .files()
        .download_to_path("files/path123", &destination, None)
        .await
        .unwrap();

    let written = tokio::fs::read(&destination).await.unwrap();
    assert_eq!(written, payload);
    tokio::fs::remove_file(&destination).await.ok();
    server.verify().await;
}

#[tokio::test]
async fn download_stream_rejects_a_file_with_no_download_uri_before_sending_anything() {
    let server = MockServer::start().await;
    // Deliberately no `Mock::given(...).mount(...)`: if a request were sent,
    // wiremock would respond 404 by default and the test would still fail
    // downstream, but `server.received_requests()` below is what actually
    // proves the crate never dialed out.
    let file = File {
        name: Some("files/uploaded123".to_owned()),
        download_uri: None,
        ..Default::default()
    };

    // `FileDownloadStream` (the `Ok` payload) has no `Debug` impl -- it
    // wraps a boxed `dyn Stream` -- so `.unwrap_err()` can't be used here.
    match test_client(server.uri())
        .files()
        .download_stream(FileSource::from(&file), None)
        .await
    {
        Err(gemini_genai::Error::Validation(msg)) => {
            assert!(
                msg.contains("download_uri"),
                "message should name the field: {msg}"
            );
        }
        Err(other) => panic!("expected Error::Validation, got {other:?}"),
        Ok(_) => panic!("expected an error, got Ok"),
    }
    assert!(
        server
            .received_requests()
            .await
            .expect("wiremock records requests")
            .is_empty(),
        "no HTTP request should have been made"
    );
}

#[tokio::test]
async fn download_stream_accepts_a_downloadable_file_object() {
    let server = MockServer::start().await;
    let payload = b"generated file bytes".to_vec();
    Mock::given(method("GET"))
        .and(path("/v1beta/files/gen456:download"))
        .and(query_param("alt", "media"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.clone()))
        .expect(1)
        .mount(&server)
        .await;

    let file = File {
        name: Some("files/gen456".to_owned()),
        download_uri: Some("https://example.com/files/gen456:download".to_owned()),
        ..Default::default()
    };

    let mut stream = test_client(server.uri())
        .files()
        .download_stream(FileSource::from(&file), None)
        .await
        .unwrap();

    let mut collected = Vec::new();
    while let Some(chunk) = stream.next().await {
        collected.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(collected, payload);
    server.verify().await;
}

#[tokio::test]
async fn download_stream_resolves_a_video_by_its_uri() {
    let server = MockServer::start().await;
    let payload = b"video bytes".to_vec();
    Mock::given(method("GET"))
        .and(path("/v1beta/files/vid789:download"))
        .and(query_param("alt", "media"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.clone()))
        .expect(1)
        .mount(&server)
        .await;

    let video = Video {
        uri: Some("files/vid789".to_owned()),
        ..Default::default()
    };

    let mut stream = test_client(server.uri())
        .files()
        .download_stream(FileSource::from(&video), None)
        .await
        .unwrap();

    let mut collected = Vec::new();
    while let Some(chunk) = stream.next().await {
        collected.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(collected, payload);
    server.verify().await;
}

#[tokio::test]
async fn download_stream_resolves_a_generated_video_by_its_inner_video_uri() {
    let server = MockServer::start().await;
    let payload = b"generated video bytes".to_vec();
    Mock::given(method("GET"))
        .and(path("/v1beta/files/genvid001:download"))
        .and(query_param("alt", "media"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.clone()))
        .expect(1)
        .mount(&server)
        .await;

    let generated = GeneratedVideo {
        video: Some(Video {
            uri: Some("files/genvid001".to_owned()),
            ..Default::default()
        }),
    };

    let mut stream = test_client(server.uri())
        .files()
        .download_stream(FileSource::from(&generated), None)
        .await
        .unwrap();

    let mut collected = Vec::new();
    while let Some(chunk) = stream.next().await {
        collected.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(collected, payload);
    server.verify().await;
}

#[tokio::test]
async fn download_stream_does_not_pre_validate_a_bare_name() {
    // Unlike the `File`/`Video`/`GeneratedVideo` variants, a bare name
    // carries no `download_uri` to check, so the request is sent
    // regardless -- an unsupported file only fails once the server
    // responds (spec FR-022).
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/files/whatever:download"))
        .and(query_param("alt", "media"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"ok".to_vec()))
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = test_client(server.uri())
        .files()
        .download_stream("files/whatever", None)
        .await
        .unwrap();
    while stream.next().await.is_some() {}
    server.verify().await;
}

#[tokio::test]
async fn dropping_the_stream_early_does_not_hang() {
    let server = MockServer::start().await;
    let payload = multi_chunk_payload();
    Mock::given(method("GET"))
        .and(path("/v1beta/files/drop123:download"))
        .and(query_param("alt", "media"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(payload))
        .mount(&server)
        .await;

    let mut stream = test_client(server.uri())
        .files()
        .download_stream("files/drop123", None)
        .await
        .unwrap();
    // Read exactly one item, then drop the stream without finishing it.
    let _ = stream.next().await;
    drop(stream);
    // Reaching this point (rather than the surrounding #[tokio::test]
    // hanging) is the assertion: the connection was released, not leaked.
}

#[tokio::test]
async fn a_body_shorter_than_its_declared_content_length_is_an_error() {
    // wiremock has no first-class "drop the connection mid-body" API. A
    // `Content-Length` that overstates the actual body was tried as a
    // substitute for a genuine mid-stream truncation (B-4), but hyper's
    // *server* role (wiremock's own HTTP stack) rejects a mismatched
    // Content-Length before it will serve anything, so the failure this
    // actually exercises is connection/request-level -- surfacing from
    // `download_stream(...).await` itself, not from an item partway
    // through an otherwise-successful stream. It is still a genuine,
    // useful regression test (an HTTP-level failure propagates as `Err`
    // and is not swallowed), just not proof of the specific mid-stream
    // scenario B-4 describes; see the completion report for what remains
    // unverified.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/files/truncated:download"))
        .and(query_param("alt", "media"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "1000000")
                .set_body_bytes(vec![1u8; 10]),
        )
        .expect(1)
        .mount(&server)
        .await;

    let result = test_client(server.uri())
        .files()
        .download_stream("files/truncated", None)
        .await;
    assert!(
        result.is_err(),
        "a mismatched Content-Length must surface as an error, not succeed silently"
    );
    server.verify().await;
}

#[tokio::test]
async fn download_to_path_creates_no_file_when_the_connection_fails_up_front() {
    // Same caveat as the test above: this proves `download_to_path`
    // doesn't leave a file behind when the *initial* connection fails
    // (before any bytes would have been written), not the stronger claim
    // that it cleans up a file it had already started writing to.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/files/truncated2:download"))
        .and(query_param("alt", "media"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "1000000")
                .set_body_bytes(vec![1u8; 10]),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut destination = std::env::temp_dir();
    destination.push(format!(
        "gemini-genai-download-truncated-{}",
        uuid::Uuid::new_v4()
    ));

    let result = test_client(server.uri())
        .files()
        .download_to_path("files/truncated2", &destination, None)
        .await;

    assert!(result.is_err(), "a truncated body must surface as an error");
    assert!(
        !destination.exists(),
        "a partial download must not be left behind as if it had succeeded"
    );
    server.verify().await;
}
