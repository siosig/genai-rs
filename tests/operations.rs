//! Integration tests for `client.operations()` (long-running operation
//! polling), exercised together with `client.models().generate_videos`
//! since that's the only operation-returning method currently
//! implemented. Runs against the public API only, via `wiremock`.

use google_genai::Client;
use google_genai::types::{GenerateVideosSource, HttpOptions};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[expect(
    clippy::unwrap_used,
    reason = "test helper: a broken Client::builder() here is a test-setup bug, not a runtime condition"
)]
fn test_client(base_url: String) -> Client {
    Client::builder()
        .api_key("test-key")
        .http_options(HttpOptions {
            base_url: Some(base_url),
            ..Default::default()
        })
        .build()
        .unwrap()
}

#[tokio::test]
async fn generate_videos_then_operations_get_returns_the_completed_operation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/veo-2.0-generate-001:predictLongRunning",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "operations/abc123",
            "done": false
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1beta/operations/abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "operations/abc123",
            "done": true,
            "response": {
                "generateVideoResponse": {
                    "generatedSamples": [
                        { "video": { "uri": "https://example.test/videos/abc123.mp4" } }
                    ]
                }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let source = GenerateVideosSource {
        prompt: Some("a neon hologram of a cat".to_owned()),
        ..Default::default()
    };
    let operation = client
        .models()
        .generate_videos("veo-2.0-generate-001", source, None)
        .await
        .unwrap();
    assert_eq!(operation.name.as_deref(), Some("operations/abc123"));
    assert_eq!(operation.done, Some(false));

    let updated = client.operations().get(&operation).await.unwrap();
    assert_eq!(updated.done, Some(true));
    let videos = updated.response.unwrap().generated_videos.unwrap();
    assert_eq!(videos.len(), 1);
    assert_eq!(
        videos[0].video.as_ref().unwrap().uri.as_deref(),
        Some("https://example.test/videos/abc123.mp4")
    );

    server.verify().await;
}

#[tokio::test]
async fn operations_get_polls_the_operations_resource_name_as_the_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/operations/xyz789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "operations/xyz789",
            "done": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let operation = google_genai::types::GenerateVideosOperation {
        name: Some("operations/xyz789".to_owned()),
        done: Some(false),
        ..Default::default()
    };
    let updated = client.operations().get(&operation).await.unwrap();
    assert_eq!(updated.done, Some(true));

    server.verify().await;
}

#[tokio::test]
async fn operations_get_surfaces_an_error_bearing_operation() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/operations/failed1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "operations/failed1",
            "done": true,
            "error": { "code": 3, "message": "invalid prompt" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let operation = google_genai::types::GenerateVideosOperation {
        name: Some("operations/failed1".to_owned()),
        done: Some(false),
        ..Default::default()
    };
    let updated = client.operations().get(&operation).await.unwrap();
    assert_eq!(updated.done, Some(true));
    assert!(updated.response.is_none());
    let error = updated.error.unwrap();
    assert_eq!(
        error.get("message").and_then(serde_json::Value::as_str),
        Some("invalid prompt")
    );

    server.verify().await;
}

#[tokio::test]
async fn operations_get_polls_a_file_search_import_operation() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/fileSearchStores/s1/operations/imp1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "fileSearchStores/s1/operations/imp1",
            "done": true,
            "response": {}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    // Proves `operations().get()` is usable with operations other than
    // video generation -- `file_search_stores().import_file()` returns this
    // type, and Python's `operations.get` accepts any `types.Operation`.
    let operation = google_genai::types::ImportFileOperation {
        name: Some("fileSearchStores/s1/operations/imp1".to_owned()),
        done: Some(false),
        ..Default::default()
    };
    let updated = client.operations().get(&operation).await.unwrap();
    assert_eq!(updated.done, Some(true));

    server.verify().await;
}

#[tokio::test]
async fn operations_get_polls_an_upload_to_file_search_store_operation() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/fileSearchStores/s1/operations/up1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "fileSearchStores/s1/operations/up1",
            "done": true,
            "response": {}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let operation = google_genai::types::UploadToFileSearchStoreOperation {
        name: Some("fileSearchStores/s1/operations/up1".to_owned()),
        done: Some(false),
        ..Default::default()
    };
    let updated = client.operations().get(&operation).await.unwrap();
    assert_eq!(updated.done, Some(true));

    server.verify().await;
}
