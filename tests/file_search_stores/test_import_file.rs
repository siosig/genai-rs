//! Ports of the plain tests in `file_search_stores/test_import_file.py`.
//!
//! Python's sync and async variants share the one async Rust API, so each
//! pair calls the same helper. The wire format keeps `snake_case` keys inside
//! nested models (Python's `convert_to_dict` only camel-cases the request's
//! own keys), which is verified against the Python SDK.
#![expect(
    clippy::unwrap_used,
    reason = "test helpers: a malformed mock or literal here is a test-setup bug"
)]

use gemini_genai::types::{ChunkingConfig, CustomMetadata, ImportFileConfig, WhiteSpaceConfig};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path},
};

use crate::common::test_client;

const FILE_SEARCH_STORE_NAME: &str = "fileSearchStores/my-store-37cbhu1nw16r";
const FILE_NAME: &str = "files/mk4h34zkv33d";

fn chunking_config() -> ImportFileConfig {
    ImportFileConfig {
        chunking_config: Some(ChunkingConfig {
            white_space_config: Some(WhiteSpaceConfig {
                max_tokens_per_chunk: Some(200),
                max_overlap_tokens: Some(20),
            }),
        }),
        ..Default::default()
    }
}

fn metadata_config() -> ImportFileConfig {
    ImportFileConfig {
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

/// Imports `FILE_NAME` with `config` and checks the request body.
async fn import_and_check(config: Option<ImportFileConfig>, mut expected_body: Value) {
    expected_body["fileName"] = json!(FILE_NAME);
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1beta/{FILE_SEARCH_STORE_NAME}:importFile")))
        .and(body_json(expected_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": format!("{FILE_SEARCH_STORE_NAME}/operations/op1"),
            "done": true
        })))
        .expect(1)
        .mount(&server)
        .await;
    let op = test_client(server.uri())
        .file_search_stores()
        .import_file(FILE_SEARCH_STORE_NAME, FILE_NAME, config)
        .await
        .unwrap();
    assert_eq!(op.done, Some(true));
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

// upstream-test: file_search_stores/test_import_file.py::test_import
#[tokio::test]
async fn test_import() {
    import_and_check(None, json!({})).await;
}

// upstream-test: file_search_stores/test_import_file.py::test_chunking
#[tokio::test]
async fn test_chunking() {
    import_and_check(Some(chunking_config()), chunking_body()).await;
}

// upstream-test: file_search_stores/test_import_file.py::test_metadata
#[tokio::test]
async fn test_metadata() {
    import_and_check(Some(metadata_config()), metadata_body()).await;
}

// upstream-test: file_search_stores/test_import_file.py::test_async_import
#[tokio::test]
async fn test_async_import() {
    import_and_check(None, json!({})).await;
}

// upstream-test: file_search_stores/test_import_file.py::test_async_chunking
#[tokio::test]
async fn test_async_chunking() {
    import_and_check(Some(chunking_config()), chunking_body()).await;
}

// upstream-test: file_search_stores/test_import_file.py::test_async_metadata
#[tokio::test]
async fn test_async_metadata() {
    import_and_check(Some(metadata_config()), metadata_body()).await;
}
