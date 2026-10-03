//! Ported from `google/genai/tests/gaos/test_environments_lifecycle.py`.
//!
//! The Rust client is async-only, so the upstream sync and async variants both
//! drive the one async API. The upload tests need a fake resumable-upload
//! service, built here with wiremock (`mount_scotty`), like upstream's
//! `_ScottyFileHandler`.

use gemini_genai::{
    environments::{
        CreateEnvironmentRequest, EnvironmentFile, EnvironmentFileSource,
        EnvironmentFileUploadConfig, EnvironmentListParams, GetEnvironmentFilesRequest,
        GetEnvironmentFilesResponse,
    },
    types::HttpOptions,
};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{method, path, path_regex},
};

use crate::{
    common::test_client_with_api_key,
    recording::{captured, captured_bodies, path_and_query, recording_server},
};

/// Path the fake upload service hands out for the data chunks.
const SCOTTY_UPLOAD_PATH: &str = "/scotty/upload/resumable_123";

fn environment_body() -> Value {
    json!({
        "id": "env_abc_1234",
        "status": "active",
        "created": "2026-07-22T15:18:38Z",
        "updated": "2026-07-22T15:18:38Z",
        "sources": [{"type": "INLINE", "content": "print('hello')", "target": "main.py"}],
    })
}

fn environment_files_payload() -> Value {
    json!({
        "files": [{
            "name": "main.py",
            "path": "workspace/src/main.py",
            "type": "file",
            "size_bytes": "128",
            "mime_type": "text/x-python",
            "created": "2026-07-22T15:18:38Z",
            "modified": "2026-07-22T15:18:38Z",
        }],
        "next_page_token": "token_next_123",
    })
}

// upstream-test: gaos/test_environments_lifecycle.py::test_python_environments_lifecycle_routes_through_google_genai_client
#[tokio::test]
async fn test_python_environments_lifecycle_routes_through_google_genai_client() {
    let server = recording_server(|_, _| environment_body()).await;
    let client = test_client_with_api_key(server.uri(), "test-api-key");
    let environments = client.environments();

    let environment = environments
        .create_environment(
            &serde_json::from_value(json!({
                "sources": [{"type": "inline", "content": "print('hello')", "target": "main.py"}],
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let forked_environment = environments
        .create_environment(&CreateEnvironmentRequest {
            from_environment: Some("environments/env_abc_1234".to_owned()),
            ..Default::default()
        })
        .await
        .unwrap();
    environments
        .list_environments(&EnvironmentListParams::default())
        .await
        .unwrap();
    let fetched = environments.get_environment("env_abc_1234").await.unwrap();
    environments
        .delete_environment("env_abc_1234")
        .await
        .unwrap();

    assert_eq!(environment.id.as_deref(), Some("env_abc_1234"));
    assert_eq!(forked_environment.id.as_deref(), Some("env_abc_1234"));
    assert_eq!(fetched.id.as_deref(), Some("env_abc_1234"));
    assert_eq!(
        captured(&server).await,
        [
            "POST /v1beta/environments",
            "POST /v1beta/environments",
            "GET /v1beta/environments",
            "GET /v1beta/environments/env_abc_1234",
            "DELETE /v1beta/environments/env_abc_1234",
        ]
    );
    let bodies = captured_bodies(&server).await;
    assert_eq!(bodies[0]["sources"][0]["content"], "print('hello')");
    assert_eq!(bodies[1]["from_environment"], "environments/env_abc_1234");
}

// upstream-test: gaos/test_environments_lifecycle.py::test_python_environments_async_create_with_from_environment
#[tokio::test]
async fn test_python_environments_async_create_with_from_environment() {
    let server = recording_server(|_, _| environment_body()).await;
    let client = test_client_with_api_key(server.uri(), "test-api-key");

    let forked_environment = client
        .environments()
        .create_environment(&CreateEnvironmentRequest {
            from_environment: Some("environments/env_abc_1234".to_owned()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(forked_environment.id.as_deref(), Some("env_abc_1234"));
    assert_eq!(captured(&server).await, ["POST /v1beta/environments"]);
    assert_eq!(
        captured_bodies(&server).await[0]["from_environment"],
        "environments/env_abc_1234"
    );
}

/// Mounts the fake service of upstream's `_ScottyFileHandler`: the `PUT
/// /upload/.../environments/{env}/files/{path}` handshake answers with an upload
/// URL, the chunk `POST` echoes a file, and `GET ...?alt=media` returns the
/// download bytes while any other `GET .../files...` returns the file listing.
async fn mount_scotty(server: &MockServer) {
    let upload_url = format!("{}{SCOTTY_UPLOAD_PATH}", server.uri());
    Mock::given(method("PUT"))
        .and(path_regex(r"^/upload/.*/environments/.*/files/.*"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-url", upload_url.as_str())
                .insert_header("x-goog-upload-status", "active"),
        )
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path(SCOTTY_UPLOAD_PATH))
        .respond_with(|request: &Request| {
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-status", "final")
                .set_body_json(json!({
                    "file": {
                        "name": "main.py",
                        "sizeBytes": request.body.len().to_string(),
                        "mimeType": "text/x-python",
                    }
                }))
        })
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .respond_with(|request: &Request| {
            if path_and_query(request).contains("alt=media") {
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/octet-stream")
                    .set_body_bytes(b"print('downloaded content')\n".to_vec())
            } else {
                ResponseTemplate::new(200).set_body_json(environment_files_payload())
            }
        })
        .mount(server)
        .await;
}

/// The request the server received first whose `METHOD` and path satisfy `pred`.
async fn first_request(
    server: &MockServer,
    http_method: &str,
    pred: impl Fn(&str) -> bool,
) -> Request {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.method.as_str() == http_method && pred(r.url.path()))
        .unwrap_or_else(|| panic!("no {http_method} request matched"))
}

fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request.headers.get(name).and_then(|v| v.to_str().ok())
}

/// Every chunk body the upload endpoint received, in order.
async fn uploaded_bytes(server: &MockServer) -> Vec<Vec<u8>> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == "POST" && r.url.path() == SCOTTY_UPLOAD_PATH)
        .map(|r| r.body)
        .collect()
}

async fn check_file_upload_download(contents: &'static [u8]) {
    let server = MockServer::start().await;
    mount_scotty(&server).await;
    let client = test_client_with_api_key(server.uri(), "test-api-key");
    let files = client.environments().files();

    // list basic
    let files_res = files
        .list("env_123", "src/main.py", &Default::default())
        .await
        .unwrap();
    let listed = files_res.files.as_ref().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name.as_deref(), Some("main.py"));
    assert_eq!(listed[0].path.as_deref(), Some("workspace/src/main.py"));
    assert_eq!(listed[0].r#type.as_ref().map(|t| t.as_str()), Some("file"));
    assert_eq!(listed[0].size_bytes, Some(128));
    assert_eq!(files_res.next_page_token.as_deref(), Some("token_next_123"));

    // list with pagination and recursive options
    let paginated = files
        .list(
            "env_123",
            "src",
            &GetEnvironmentFilesRequest {
                page_size: Some(10),
                page_token: Some("token_start".to_owned()),
                recursive: Some(true),
            },
        )
        .await
        .unwrap();
    assert_eq!(paginated.files.as_ref().map(Vec::len), Some(1));

    // upload
    let upload_res = files
        .upload(
            "env_123",
            "src/main.py",
            EnvironmentFileSource::Bytes(contents.to_vec()),
            &EnvironmentFileUploadConfig {
                mime_type: Some("text/x-python".to_owned()),
                overwrite: Some(true),
                extract: None,
            },
        )
        .await
        .unwrap();
    let uploaded = upload_res.files.as_ref().unwrap();
    assert_eq!(uploaded.len(), 1);
    assert_eq!(uploaded[0].name.as_deref(), Some("main.py"));
    assert_eq!(uploaded_bytes(&server).await[0], contents);

    // Verify handshake and chunk headers
    let handshake = first_request(&server, "PUT", |p| p.contains("/files/")).await;
    assert!(
        handshake
            .url
            .path()
            .starts_with("/upload/v1beta/environments/env_123/files/src/main.py"),
        "{}",
        handshake.url
    );
    assert!(handshake.url.query().unwrap().contains("overwrite=true"));
    assert_eq!(
        header(&handshake, "x-goog-upload-protocol"),
        Some("resumable")
    );
    assert_eq!(header(&handshake, "x-goog-upload-command"), Some("start"));
    assert_eq!(
        header(&handshake, "x-goog-upload-header-content-length"),
        Some(contents.len().to_string().as_str())
    );
    assert_eq!(
        header(&handshake, "x-goog-upload-header-content-type"),
        Some("text/x-python")
    );
    let chunk = first_request(&server, "POST", |p| p == SCOTTY_UPLOAD_PATH).await;
    assert_eq!(
        header(&chunk, "x-goog-upload-command"),
        Some("upload, finalize")
    );
    assert_eq!(header(&chunk, "x-goog-upload-offset"), Some("0"));
    assert_eq!(chunk.body, contents);

    // download
    let downloaded = files.download("env_123", "src/main.py").await.unwrap();
    assert_eq!(downloaded.as_ref(), b"print('downloaded content')\n");
    // download with full resource name and leading slash
    let downloaded_full = files
        .download("environments/env_123", "/src/main.py")
        .await
        .unwrap();
    assert_eq!(downloaded_full.as_ref(), b"print('downloaded content')\n");

    let calls = captured(&server).await;
    assert!(calls.iter().any(|c| c.contains("page_size=10")));
    assert!(calls.iter().any(|c| c.contains("page_token=token_start")));
    assert!(calls.iter().any(|c| c.contains("recursive=true")));
    assert!(
        calls.contains(&"GET /v1beta/environments/env_123/files/src/main.py?alt=media".to_owned()),
        "{calls:?}"
    );
}

// upstream-test: gaos/test_environments_lifecycle.py::test_python_environments_file_upload_download
#[tokio::test]
async fn test_python_environments_file_upload_download() {
    // The upstream `with_raw_response.files.list(...).parse()` tail has no Rust
    // counterpart (no raw-response wrapper); `files().list` already returns the parsed value.
    check_file_upload_download(b"print('hello world')").await;
}

// upstream-test: gaos/test_environments_lifecycle.py::test_python_environments_async_file_upload_download
#[tokio::test]
async fn test_python_environments_async_file_upload_download() {
    check_file_upload_download(b"print('async hello world')").await;
}

/// A scratch directory under the OS temp dir, removed on drop.
struct ScratchDir(std::path::PathBuf);

impl ScratchDir {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("genai-rs-env-files-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn check_upload_sources(prefix: &str, with_extract_false: bool) {
    let server = MockServer::start().await;
    mount_scotty(&server).await;
    let client = test_client_with_api_key(server.uri(), "test-api-key");
    let files = client.environments().files();
    let dir = ScratchDir::new();

    // 1. Upload from in-memory bytes (Python: io.BytesIO)
    let stream_data = format!("{prefix}content from BytesIO").into_bytes();
    let upload_res = files
        .upload(
            "env_123",
            "stream.txt",
            EnvironmentFileSource::Bytes(stream_data.clone()),
            &EnvironmentFileUploadConfig {
                mime_type: Some("text/plain".to_owned()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(upload_res.files.as_ref().map(Vec::len), Some(1));
    assert_eq!(uploaded_bytes(&server).await.last().unwrap(), &stream_data);

    // 2. Upload from a file path (Python: str path); the MIME type is guessed.
    let tmp_file = dir.0.join("hello.py");
    let tmp_contents = format!("print('{prefix}from file path str')");
    std::fs::write(&tmp_file, &tmp_contents).unwrap();
    let upload_res_str = files
        .upload(
            "env_123",
            "hello.py",
            EnvironmentFileSource::Path(tmp_file.clone()),
            &EnvironmentFileUploadConfig::default(),
        )
        .await
        .unwrap();
    assert_eq!(upload_res_str.files.as_ref().map(Vec::len), Some(1));
    assert_eq!(
        uploaded_bytes(&server).await.last().unwrap(),
        tmp_contents.as_bytes()
    );
    let handshake = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .rfind(|r| r.method.as_str() == "PUT")
        .unwrap();
    // Python's `mimetypes` answers `text/x-python` for `.py`; the `mime_guess` table
    // behind `Files::upload` answers `text/plain`. Either is a guess from the name.
    assert_eq!(
        header(&handshake, "x-goog-upload-header-content-type"),
        Some("text/plain"),
        "guessed from `hello.py`"
    );

    // 3. Upload from a path, with explicit extract / overwrite flags (Python: pathlib.Path)
    let tmp_file_path = dir.0.join("hello_path.py");
    let path_contents = format!("print('{prefix}from pathlib.Path')");
    std::fs::write(&tmp_file_path, &path_contents).unwrap();
    let upload_res_path = files
        .upload(
            "env_123",
            "hello_path.py",
            EnvironmentFileSource::Path(tmp_file_path),
            &EnvironmentFileUploadConfig {
                extract: with_extract_false.then_some(false),
                overwrite: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(upload_res_path.files.as_ref().map(Vec::len), Some(1));
    assert_eq!(
        uploaded_bytes(&server).await.last().unwrap(),
        path_contents.as_bytes()
    );
    let handshake = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .rfind(|r| r.method.as_str() == "PUT")
        .unwrap();
    let query = handshake.url.query().unwrap();
    assert!(query.contains("overwrite=true"), "{query}");
    assert_eq!(
        query.contains("extract=false"),
        with_extract_false,
        "{query}"
    );

    // 4. Upload from an open file handle (Python: io.IOBase). A Rust caller reads
    // the handle and passes the bytes; the wire exchange is identical to case 1.
    let handle_bytes = std::fs::read(&tmp_file).unwrap();
    let upload_res_fh = files
        .upload(
            "env_123",
            "hello_fh.py",
            EnvironmentFileSource::Bytes(handle_bytes),
            &EnvironmentFileUploadConfig::default(),
        )
        .await
        .unwrap();
    assert_eq!(upload_res_fh.files.as_ref().map(Vec::len), Some(1));
    assert_eq!(
        uploaded_bytes(&server).await.last().unwrap(),
        tmp_contents.as_bytes()
    );
}

// upstream-test: gaos/test_environments_lifecycle.py::test_python_environments_upload_io_and_file_paths
#[tokio::test]
async fn test_python_environments_upload_io_and_file_paths() {
    check_upload_sources("", true).await;
}

// upstream-test: gaos/test_environments_lifecycle.py::test_python_environments_async_upload_io_and_file_paths
#[tokio::test]
async fn test_python_environments_async_upload_io_and_file_paths() {
    check_upload_sources("async ", false).await;
}

// upstream-test: gaos/test_environments_lifecycle.py::test_python_environments_types_and_models
#[test]
fn test_python_environments_types_and_models() {
    let file_obj: EnvironmentFile = serde_json::from_value(json!({
        "created": "2026-07-22T15:18:38Z",
        "mime_type": "text/x-python",
        "modified": "2026-07-22T15:18:38Z",
        "name": "main.py",
        "path": "workspace/src/main.py",
        "size_bytes": 128,
        "type": "file",
    }))
    .unwrap();
    assert_eq!(file_obj.name.as_deref(), Some("main.py"));
    assert_eq!(file_obj.path.as_deref(), Some("workspace/src/main.py"));
    assert_eq!(file_obj.r#type.as_ref().map(|t| t.as_str()), Some("file"));
    assert_eq!(file_obj.size_bytes, Some(128));
    assert_eq!(file_obj.mime_type.as_deref(), Some("text/x-python"));
    assert!(file_obj.created.is_some());
    assert!(file_obj.modified.is_some());

    let response = GetEnvironmentFilesResponse {
        files: Some(vec![file_obj]),
        next_page_token: Some("next_tok".to_owned()),
    };
    assert_eq!(response.files.as_ref().map(Vec::len), Some(1));
    assert_eq!(response.next_page_token.as_deref(), Some("next_tok"));

    // Request models: the Rust files request carries the query parameters only;
    // `environment` and `path` are path arguments and `api_version` a per-call `HttpOptions`.
    let req = GetEnvironmentFilesRequest {
        page_size: Some(20),
        page_token: Some("tok".to_owned()),
        recursive: Some(true),
    };
    assert_eq!(req.page_size, Some(20));
    assert_eq!(req.page_token.as_deref(), Some("tok"));
    assert_eq!(req.recursive, Some(true));
    let _ = HttpOptions {
        api_version: Some("v1beta".to_owned()),
        ..Default::default()
    };

    let create_req = CreateEnvironmentRequest {
        from_environment: Some("environments/env_abc_1234".to_owned()),
        ..Default::default()
    };
    assert_eq!(
        create_req.from_environment.as_deref(),
        Some("environments/env_abc_1234")
    );
}

// upstream-test: gaos/test_environments_lifecycle.py::test_python_environments_dedicated_module_and_from_environment
#[test]
fn test_python_environments_dedicated_module_and_from_environment() {
    // Python's `hasattr` checks become compile-time name checks on the dedicated
    // `environments` module, and on the re-exports in the `interactions` module.
    use gemini_genai::{
        environments::{
            CreateEnvironmentRequest, Environment, EnvironmentDeleteResponse, EnvironmentFile,
            EnvironmentListParams, EnvironmentListResponse, EnvironmentStatus,
            GetEnvironmentFilesResponse,
        },
        interactions,
    };
    let _ = (
        std::any::type_name::<CreateEnvironmentRequest>(),
        std::any::type_name::<Environment>(),
        std::any::type_name::<EnvironmentListResponse>(),
        std::any::type_name::<EnvironmentFile>(),
        std::any::type_name::<GetEnvironmentFilesResponse>(),
        std::any::type_name::<EnvironmentStatus>(),
        std::any::type_name::<EnvironmentListParams>(),
        std::any::type_name::<EnvironmentDeleteResponse>(),
        // backward-compatible re-exports in the interactions module
        std::any::type_name::<interactions::Environment>(),
        std::any::type_name::<interactions::CreateEnvironmentRequest>(),
        std::any::type_name::<interactions::EnvironmentFile>(),
    );

    let req = CreateEnvironmentRequest {
        from_environment: Some("environments/env_abc_1234".to_owned()),
        ..Default::default()
    };
    assert_eq!(
        req.from_environment.as_deref(),
        Some("environments/env_abc_1234")
    );
    // `model_dump(exclude_unset=True, by_alias=True)`: only the set field is serialized.
    assert_eq!(
        serde_json::to_value(&req).unwrap(),
        json!({"from_environment": "environments/env_abc_1234"})
    );
}
