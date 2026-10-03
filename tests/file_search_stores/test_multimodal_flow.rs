//! Port of the custom `multimodal_search_flow` table case in
//! `file_search_stores/test_multimodal_flow.py`.
//!
//! The upstream flow (create store, upload a text and an image file, poll
//! both operations, run a grounded `generate_content`, download the media
//! referenced by the grounding chunk, delete the store) is served step by
//! step from `wiremock`. Deviation: the Rust upload takes bytes plus a MIME
//! type instead of a path or file-like object, so the image is read from
//! `tests/data/google.png` (upstream reads `../data/dog.jpg`).

use gemini_genai::types::{
    CreateFileSearchStoreConfig, DeleteFileSearchStoreConfig, FileSearch, GenerateContentConfig,
    Tool, UploadToFileSearchStoreConfig,
};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, body_partial_json, method, path, query_param},
};

use crate::common::test_client;

const DISPLAY_NAME: &str = "test-multimodal-store";
const EMBEDDING_MODEL: &str = "models/gemini-embedding-2-preview";
const QUERY: &str = "Find the photo of the dog in the park, what is the dog doing?";
const TEXT_CONTENT: &str = "This is a test text file content for file search.";
const STORE: &str = "fileSearchStores/multimodal-1";
const MEDIA_ID: &str = "fileSearchStores/multimodal-1/media/blob-1";
const START_PATH: &str = "/upload/v1beta/fileSearchStores/multimodal-1:uploadToFileSearchStore";

/// Mounts the resumable-upload start and finalize mocks for one MIME type.
async fn mount_upload(server: &MockServer, mime: &str, session: &str, op_name: &str, done: bool) {
    let upload_url = format!("{}{session}", server.uri());
    Mock::given(method("POST"))
        .and(path(START_PATH))
        .and(body_partial_json(json!({"mimeType": mime})))
        .respond_with(
            ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", upload_url.as_str()),
        )
        .expect(1)
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path(session))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-status", "final")
                .set_body_json(json!({"name": op_name, "done": done})),
        )
        .expect(1)
        .mount(server)
        .await;
}

/// Mounts every endpoint the flow touches: store create, both uploads, the
/// operation poll, the grounded search, the media download and the cleanup.
async fn mount_flow_mocks(server: &MockServer) {
    let text_op = format!("{STORE}/operations/op-text");
    let image_op = format!("{STORE}/operations/op-image");

    // 1. Create store.
    Mock::given(method("POST"))
        .and(path("/v1beta/fileSearchStores"))
        .and(body_json(json!({
            "displayName": DISPLAY_NAME,
            "embeddingModel": EMBEDDING_MODEL
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name": STORE})))
        .expect(1)
        .mount(server)
        .await;
    // 2-3. Upload text (still running) and image (already done).
    mount_upload(
        server,
        "text/plain",
        "/upload-session/text",
        &text_op,
        false,
    )
    .await;
    mount_upload(
        server,
        "image/png",
        "/upload-session/image",
        &image_op,
        true,
    )
    .await;
    // 4. Poll the text operation to completion.
    Mock::given(method("GET"))
        .and(path(format!("/v1beta/{text_op}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"name": text_op, "done": true})),
        )
        .expect(1)
        .mount(server)
        .await;
    // 5. Grounded search.
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-flash:generateContent"))
        .and(body_partial_json(json!({
            // Nested models keep snake_case keys on the wire, as in Python.
            "tools": [{"fileSearch": {"file_search_store_names": [STORE]}}]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "The dog is running."}]},
                "groundingMetadata": {"groundingChunks": [
                    {"retrievedContext": {"mediaId": MEDIA_ID}}
                ]}
            }]
        })))
        .expect(1)
        .mount(server)
        .await;
    // 6. Download media.
    Mock::given(method("GET"))
        .and(path(format!("/v1beta/{MEDIA_ID}")))
        .and(query_param("alt", "media"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"image-bytes".to_vec()))
        .expect(1)
        .mount(server)
        .await;
    // Cleanup: force delete.
    Mock::given(method("DELETE"))
        .and(path(format!("/v1beta/{STORE}")))
        .and(query_param("force", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(server)
        .await;
}

// upstream-test: file_search_stores/test_multimodal_flow.py::test_multimodal_search_flow
#[tokio::test]
async fn test_multimodal_search_flow() {
    let server = MockServer::start().await;
    mount_flow_mocks(&server).await;

    let client = test_client(server.uri());
    let stores = client.file_search_stores();

    let store = stores
        .create(Some(CreateFileSearchStoreConfig {
            display_name: Some(DISPLAY_NAME.to_owned()),
            embedding_model: Some(EMBEDDING_MODEL.to_owned()),
            ..Default::default()
        }))
        .await
        .unwrap();
    let store_name = store.name.unwrap();

    let mut op_text = stores
        .upload_to_file_search_store(
            &store_name,
            TEXT_CONTENT.as_bytes(),
            "text/plain",
            Some(UploadToFileSearchStoreConfig {
                mime_type: Some("text/plain".to_owned()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let image = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/google.png"
    ))
    .unwrap();
    let op_image = stores
        .upload_to_file_search_store(
            &store_name,
            &image,
            "image/png",
            Some(UploadToFileSearchStoreConfig {
                mime_type: Some("image/png".to_owned()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    assert_eq!(op_image.done, Some(true));

    assert_ne!(op_text.done, Some(true));
    op_text = client.operations().get(&op_text).await.unwrap();
    assert_eq!(op_text.done, Some(true));

    let response = client
        .models()
        .generate_content(
            "gemini-2.5-flash",
            QUERY,
            Some(GenerateContentConfig {
                tools: Some(vec![Tool {
                    file_search: Some(FileSearch {
                        file_search_store_names: Some(vec![store_name.clone()]),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let metadata = response
        .candidates
        .unwrap()
        .remove(0)
        .grounding_metadata
        .unwrap();
    let media_id = metadata
        .grounding_chunks
        .unwrap()
        .into_iter()
        .find_map(|chunk| chunk.retrieved_context.and_then(|ctx| ctx.media_id))
        .unwrap();
    assert_eq!(media_id, MEDIA_ID);

    let content = stores.download_media(&media_id, None).await.unwrap();
    assert_eq!(&content[..], b"image-bytes");

    stores
        .delete(
            &store_name,
            Some(DeleteFileSearchStoreConfig {
                force: Some(true),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    server.verify().await;
}
