//! Port of `google/genai/tests/models/test_generate_content_media_resolution.py`
//! (plain test; the table case lives in the oracle corpus).

use gemini_genai::types::{GenerateContentConfig, HttpOptions, MediaResolution, Part};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

// upstream-test: models/test_generate_content_media_resolution.py::test_low_media_resolution
#[tokio::test]
async fn test_low_media_resolution() {
    let server = MockServer::start().await;
    let upload_url = format!("{}/upload-session/image", server.uri());
    Mock::given(method("POST"))
        .and(path("/upload/v1beta/files"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", upload_url.as_str()),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/upload-session/image"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-status", "final")
                .set_body_json(serde_json::json!({"file": {
                    "name": "files/image", "mimeType": "image/png",
                    "uri": "https://generativelanguage.googleapis.com/v1beta/files/image"
                }})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1alpha/models/gemini-2.5-flash:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "candidates": [{"content": {"role": "model", "parts": [{"text": "A logo."}]}, "finishReason": "STOP"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let source = gemini_genai::files::UploadSource::Bytes {
        data: vec![0x89, b'P', b'N', b'G'],
        mime_type: "image/png".to_owned(),
    };
    let file = client.files().upload(source, None).await.unwrap();
    let contents = vec![
        Part::from_uri(file.uri.clone().unwrap_or_default(), "image/png"),
        Part::from_text("Describe the image."),
    ];
    let config = GenerateContentConfig {
        media_resolution: Some(MediaResolution::MediaResolutionLow),
        http_options: Some(HttpOptions {
            api_version: Some("v1alpha".to_owned()),
            base_url: Some(server.uri()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let response = client
        .models()
        .generate_content("gemini-2.5-flash", contents, Some(config))
        .await
        .unwrap();
    assert_eq!(response.text().as_deref(), Some("A logo."));

    let requests = server.received_requests().await.unwrap();
    let generate = requests
        .iter()
        .find(|r| r.url.path().ends_with(":generateContent"))
        .unwrap();
    let body: serde_json::Value = generate.body_json().unwrap();
    assert_eq!(
        body["generationConfig"]["mediaResolution"],
        "MEDIA_RESOLUTION_LOW"
    );
}
