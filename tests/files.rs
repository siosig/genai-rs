//! Integration tests for `client.files()`: upload (resumable protocol,
//! multi-chunk), get, list (Pager), delete, and download.

mod common;

use common::test_client;
use wiremock::matchers::{
    body_json, header, headers, method, path, query_param, query_param_is_missing,
};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
