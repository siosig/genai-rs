//! Port of `google/genai/tests/shared/caches/test_create_update_get.py`.

use gemini_genai::{
    files::UploadSource,
    types::{Content, CreateCachedContentConfig, Part, UpdateCachedContentConfig},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path},
};

use crate::common::test_client;

const GEMINI_MODEL: &str = "gemini-2.5-flash";

// The Developer API branch of the upstream helper: upload a file, cache five
// copies of its URI, then update its ttl and get it.
// upstream-test: shared/caches/test_create_update_get.py::test_create_update_get
#[tokio::test]
async fn test_create_update_get() {
    let server = MockServer::start().await;
    let upload_url = format!("{}/upload-session/img", server.uri());
    Mock::given(method("POST"))
        .and(path("/upload/v1beta/files"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", upload_url.as_str()),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/upload-session/img"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-status", "final")
                .set_body_json(serde_json::json!({"file": {
                    "name": "files/img1",
                    "uri": "https://generativelanguage.googleapis.com/v1beta/files/img1",
                    "mimeType": "image/png"
                }})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1beta/cachedContents"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "cachedContents/abc123"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/v1beta/cachedContents/abc123"))
        .and(body_json(serde_json::json!({"ttl": "7200s"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "cachedContents/abc123"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1beta/cachedContents/abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "cachedContents/abc123"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client = test_client(server.uri());
    let file = client
        .files()
        .upload(
            UploadSource::Bytes {
                data: vec![0x89, b'P', b'N', b'G'],
                mime_type: "image/png".to_owned(),
            },
            None,
        )
        .await
        .unwrap();
    let part = Part::from_uri(file.uri.unwrap(), file.mime_type.unwrap());
    let config = CreateCachedContentConfig {
        contents: Some(
            std::iter::repeat_n(
                Content {
                    role: Some("user".to_owned()),
                    parts: Some(vec![part]),
                },
                5,
            )
            .collect(),
        ),
        ..Default::default()
    };
    let cache = client
        .caches()
        .create(GEMINI_MODEL, Some(config))
        .await
        .unwrap();
    let updated_cache = client
        .caches()
        .update(
            &cache.name.unwrap(),
            Some(UpdateCachedContentConfig {
                ttl: Some("7200s".to_owned()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    client
        .caches()
        .get(&updated_cache.name.unwrap(), None)
        .await
        .unwrap();
    server.verify().await;
}
