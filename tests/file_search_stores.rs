//! Integration tests for `client.file_search_stores()` and
//! `client.file_search_stores().documents()`. Runs against the public API
//! only, via `wiremock`, mirroring `src/file_search_stores.rs`'s and
//! `src/documents.rs`'s own unit tests but exercised from outside the crate.

mod common;

use common::test_client;
use google_genai::types::{
    CreateFileSearchStoreConfig, DeleteDocumentConfig, DeleteFileSearchStoreConfig,
    ImportFileConfig, ListDocumentsConfig, UploadToFileSearchStoreConfig,
};
use wiremock::matchers::{
    body_json, header, headers, method, path, query_param, query_param_is_missing,
};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn create_get_list_delete_a_file_search_store() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1beta/fileSearchStores"))
        .and(body_json(serde_json::json!({"displayName": "my store"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "fileSearchStores/abc123",
            "displayName": "my store"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1beta/fileSearchStores/abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "fileSearchStores/abc123",
            "displayName": "my store",
            "activeDocumentsCount": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1beta/fileSearchStores"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "fileSearchStores": [{"name": "fileSearchStores/abc123"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1beta/fileSearchStores/abc123"))
        .and(query_param_is_missing("force"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let stores = client.file_search_stores();

    let created = stores
        .create(Some(CreateFileSearchStoreConfig {
            display_name: Some("my store".to_owned()),
            ..Default::default()
        }))
        .await
        .unwrap();
    assert_eq!(created.name.as_deref(), Some("fileSearchStores/abc123"));

    let fetched = stores.get("fileSearchStores/abc123", None).await.unwrap();
    assert_eq!(fetched.active_documents_count, Some(2));

    let pager = stores.list(None).await.unwrap();
    assert_eq!(pager.page().len(), 1);
    assert_eq!(
        pager.page()[0].name.as_deref(),
        Some("fileSearchStores/abc123")
    );

    stores
        .delete(
            "fileSearchStores/abc123",
            Some(DeleteFileSearchStoreConfig::default()),
        )
        .await
        .unwrap();

    server.verify().await;
}

#[tokio::test]
async fn import_file_returns_a_long_running_operation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/fileSearchStores/abc123:importFile"))
        .and(body_json(serde_json::json!({
            "fileName": "files/xyz",
            // Note: the Gemini API's mldev wire format for CustomMetadata
            // items themselves uses snake_case (matching the Python SDK's
            // `_common.convert_to_dict`, which only camelCases the
            // *object's own* keys, not fields of Pydantic sub-models
            // nested inside a list).
            "customMetadata": [{"key": "topic", "string_value": "cats"}]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "fileSearchStores/abc123/operations/op1",
            "done": true,
            "response": {
                "parent": "fileSearchStores/abc123",
                "documentName": "fileSearchStores/abc123/documents/doc1"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let op = client
        .file_search_stores()
        .import_file(
            "fileSearchStores/abc123",
            "files/xyz",
            Some(ImportFileConfig {
                custom_metadata: Some(vec![google_genai::types::CustomMetadata {
                    key: Some("topic".to_owned()),
                    string_value: Some("cats".to_owned()),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    assert_eq!(op.done, Some(true));
    assert_eq!(
        op.response.unwrap().document_name.as_deref(),
        Some("fileSearchStores/abc123/documents/doc1")
    );
    server.verify().await;
}

#[tokio::test]
async fn upload_to_file_search_store_runs_the_resumable_upload_protocol() {
    let server = MockServer::start().await;
    let upload_url = format!("{}/upload-session/abc", server.uri());

    Mock::given(method("POST"))
        .and(path(
            "/upload/v1beta/fileSearchStores/abc123:uploadToFileSearchStore",
        ))
        .and(header("X-Goog-Upload-Protocol", "resumable"))
        .and(header("X-Goog-Upload-Command", "start"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", upload_url.as_str()),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/upload-session/abc"))
        // wiremock's `header()` matcher splits the *actual* request header
        // value on commas, so a literal `"upload, finalize"` value must be
        // matched via `headers()` with the pre-split parts.
        .and(headers("X-Goog-Upload-Command", vec!["upload", "finalize"]))
        .and(header("X-Goog-Upload-Offset", "0"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-status", "final")
                .set_body_json(serde_json::json!({
                    "name": "fileSearchStores/abc123/operations/op2",
                    "done": true,
                    "response": {
                        "parent": "fileSearchStores/abc123",
                        "documentName": "fileSearchStores/abc123/documents/doc2"
                    }
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let op = client
        .file_search_stores()
        .upload_to_file_search_store(
            "fileSearchStores/abc123",
            b"hello world",
            "text/plain",
            Some(UploadToFileSearchStoreConfig {
                display_name: Some("doc2".to_owned()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    assert_eq!(op.done, Some(true));
    assert_eq!(
        op.response.unwrap().document_name.as_deref(),
        Some("fileSearchStores/abc123/documents/doc2")
    );
    server.verify().await;
}

#[tokio::test]
async fn download_media_returns_raw_bytes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/fileSearchStores/abc123/media/blob1"))
        .and(query_param("alt", "media"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"raw-media-bytes".to_vec()))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let data = client
        .file_search_stores()
        .download_media("fileSearchStores/abc123/media/blob1", None)
        .await
        .unwrap();
    assert_eq!(&data[..], b"raw-media-bytes");
    server.verify().await;
}

#[tokio::test]
async fn documents_get_list_and_delete() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1beta/fileSearchStores/abc123/documents/doc1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "fileSearchStores/abc123/documents/doc1",
            "displayName": "doc one",
            "state": "STATE_ACTIVE"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1beta/fileSearchStores/abc123/documents"))
        .and(query_param("pageSize", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "documents": [
                {"name": "fileSearchStores/abc123/documents/doc1"},
                {"name": "fileSearchStores/abc123/documents/doc2"}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1beta/fileSearchStores/abc123/documents/doc1"))
        .and(query_param("force", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let documents = client.file_search_stores().documents();

    let doc = documents
        .get("fileSearchStores/abc123/documents/doc1", None)
        .await
        .unwrap();
    assert_eq!(doc.display_name.as_deref(), Some("doc one"));
    assert_eq!(
        doc.state,
        Some(google_genai::types::DocumentState::StateActive)
    );

    let pager = documents
        .list(
            "fileSearchStores/abc123",
            Some(ListDocumentsConfig {
                page_size: Some(10),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    assert_eq!(pager.page().len(), 2);

    documents
        .delete(
            "fileSearchStores/abc123/documents/doc1",
            Some(DeleteDocumentConfig {
                force: Some(true),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    server.verify().await;
}
