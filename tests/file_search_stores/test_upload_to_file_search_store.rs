//! Ports of the plain tests in
//! `file_search_stores/test_upload_to_file_search_store.py`.
//!
//! Python replays the recorded resumable upload; these serve the start
//! request and the single `upload, finalize` chunk from `wiremock` and assert
//! on both. Python's sync and async variants share the one async Rust API.
//! Deviation: Python accepts a path or file-like object and guesses the MIME
//! type from the path; the Rust API takes the bytes and the MIME type, so
//! the path variants read `tests/data/story.pdf` and pass `application/pdf`
//! (what Python guesses for that path).
#![expect(
    clippy::unwrap_used,
    reason = "test helpers: a malformed mock or literal here is a test-setup bug"
)]

use gemini_genai::types::{
    ChunkingConfig, CustomMetadata, UploadToFileSearchStoreConfig, WhiteSpaceConfig,
};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_bytes, body_json, header, headers, method, path},
};

use crate::common::test_client;

const FILE_SEARCH_STORE_NAME: &str = "fileSearchStores/my-store-37cbhu1nw16r";
const PDF_MIME: &str = "application/pdf";
const SESSION_PATH: &str = "/upload-session/story";

fn story_pdf() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/story.pdf")).unwrap()
}

fn chunking_config() -> UploadToFileSearchStoreConfig {
    UploadToFileSearchStoreConfig {
        chunking_config: Some(ChunkingConfig {
            white_space_config: Some(WhiteSpaceConfig {
                max_tokens_per_chunk: Some(200),
                max_overlap_tokens: Some(20),
            }),
        }),
        ..Default::default()
    }
}

fn metadata_config() -> UploadToFileSearchStoreConfig {
    UploadToFileSearchStoreConfig {
        custom_metadata: Some(vec![
            CustomMetadata {
                key: Some("year".to_owned()),
                numeric_value: Some(2024.0),
                ..Default::default()
            },
            CustomMetadata {
                key: Some("tag".to_owned()),
                string_value: Some("story".to_owned()),
                ..Default::default()
            },
        ]),
        ..Default::default()
    }
}

/// Runs `upload_to_file_search_store` for the story PDF with `config` and
/// checks the start request body (`expected_start_body`, always carrying
/// `mimeType`), its upload headers, and the finalized chunk.
async fn upload_and_check(config: Option<UploadToFileSearchStoreConfig>, extra_body: Value) {
    let data = story_pdf();
    let mut expected_start_body = json!({"mimeType": PDF_MIME});
    if let (Some(target), Some(extra)) =
        (expected_start_body.as_object_mut(), extra_body.as_object())
    {
        target.extend(extra.clone());
    }

    let server = MockServer::start().await;
    let upload_url = format!("{}{SESSION_PATH}", server.uri());
    Mock::given(method("POST"))
        .and(path(format!(
            "/upload/v1beta/{FILE_SEARCH_STORE_NAME}:uploadToFileSearchStore"
        )))
        .and(header("X-Goog-Upload-Protocol", "resumable"))
        .and(header("X-Goog-Upload-Command", "start"))
        .and(header("X-Goog-Upload-Header-Content-Type", PDF_MIME))
        .and(header(
            "X-Goog-Upload-Header-Content-Length",
            data.len().to_string().as_str(),
        ))
        .and(body_json(expected_start_body))
        .respond_with(
            ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", upload_url.as_str()),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SESSION_PATH))
        .and(headers("X-Goog-Upload-Command", vec!["upload", "finalize"]))
        .and(header("X-Goog-Upload-Offset", "0"))
        .and(body_bytes(data.clone()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-status", "final")
                .set_body_json(json!({
                    "name": format!("{FILE_SEARCH_STORE_NAME}/operations/op1"),
                    "done": true,
                    "response": {
                        "parent": FILE_SEARCH_STORE_NAME,
                        "documentName": format!("{FILE_SEARCH_STORE_NAME}/documents/story-1")
                    }
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let op = test_client(server.uri())
        .file_search_stores()
        .upload_to_file_search_store(FILE_SEARCH_STORE_NAME, &data, PDF_MIME, config)
        .await
        .unwrap();
    assert_eq!(op.done, Some(true));
    assert_eq!(
        op.response.unwrap().document_name.as_deref(),
        Some(format!("{FILE_SEARCH_STORE_NAME}/documents/story-1").as_str())
    );
    server.verify().await;
}

fn chunking_body() -> Value {
    json!({"chunkingConfig": {"white_space_config": {
        "max_tokens_per_chunk": 200, "max_overlap_tokens": 20
    }}})
}

fn metadata_body() -> Value {
    json!({"customMetadata": [
        {"key": "year", "numeric_value": 2024.0},
        {"key": "tag", "string_value": "story"}
    ]})
}

// upstream-test: file_search_stores/test_upload_to_file_search_store.py::test_file_path
#[tokio::test]
async fn test_file_path() {
    upload_and_check(None, json!({})).await;
}

// upstream-test: file_search_stores/test_upload_to_file_search_store.py::test_bytesio
#[tokio::test]
async fn test_bytesio() {
    let config = UploadToFileSearchStoreConfig {
        mime_type: Some(PDF_MIME.to_owned()),
        ..Default::default()
    };
    upload_and_check(Some(config), json!({})).await;
}

// upstream-test: file_search_stores/test_upload_to_file_search_store.py::test_chunking
#[tokio::test]
async fn test_chunking() {
    upload_and_check(Some(chunking_config()), chunking_body()).await;
}

// upstream-test: file_search_stores/test_upload_to_file_search_store.py::test_metadata
#[tokio::test]
async fn test_metadata() {
    upload_and_check(Some(metadata_config()), metadata_body()).await;
}

// upstream-test: file_search_stores/test_upload_to_file_search_store.py::test_async_file_path
#[tokio::test]
async fn test_async_file_path() {
    upload_and_check(None, json!({})).await;
}

// upstream-test: file_search_stores/test_upload_to_file_search_store.py::test_async_bytesio
#[tokio::test]
async fn test_async_bytesio() {
    let config = UploadToFileSearchStoreConfig {
        mime_type: Some(PDF_MIME.to_owned()),
        ..Default::default()
    };
    upload_and_check(Some(config), json!({})).await;
}

// upstream-test: file_search_stores/test_upload_to_file_search_store.py::test_async_chunking
#[tokio::test]
async fn test_async_chunking() {
    upload_and_check(Some(chunking_config()), chunking_body()).await;
}

// upstream-test: file_search_stores/test_upload_to_file_search_store.py::test_async_metadata
#[tokio::test]
async fn test_async_metadata() {
    upload_and_check(Some(metadata_config()), metadata_body()).await;
}
