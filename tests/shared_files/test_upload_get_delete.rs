//! Ports of `google/genai/tests/shared/files/test_upload_get_delete.py`.

use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use super::common::test_client;

// upstream-test: shared/files/test_upload_get_delete.py::test_upload_get_delete_image
#[tokio::test]
async fn test_upload_get_delete_image() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let image = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join("google.png");

    let session = format!("{}/upload-session/img", server.uri());
    Mock::given(method("POST"))
        .and(path("/upload/v1beta/files"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", session.as_str()),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/upload-session/img"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-status", "final")
                .set_body_json(serde_json::json!({"file": {"name": "files/img123"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1beta/files/img123"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"name": "files/img123"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v1beta/files/img123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let files = client.files();
    let file = files.upload(image.as_path(), None).await?;
    let name = file.name.ok_or("uploaded file has no name")?;
    let got = files.get(&name, None).await?;
    assert_eq!(got.name.as_deref(), Some(name.as_str()));
    files.delete(&name, None).await?;
    server.verify().await;
    Ok(())
}
