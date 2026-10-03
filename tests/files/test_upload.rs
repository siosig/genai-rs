//! Ports of `google/genai/tests/files/test_upload.py`.
//!
//! Upstream runs these against recorded replays; here a `wiremock` server
//! plays the resumable-upload protocol and each test asserts what the
//! client actually put on the wire (MIME type, display name, payload).
//!
//! Python argument-type variants map onto the Rust source types:
//! `str`/`pathlib.Path` -> `UploadSource::Path`; `io.BytesIO` and an open
//! file object -> `UploadSource::Bytes` (this crate takes no reader); a
//! `config` dict -> a JSON value deserialised into `UploadFileConfig`.

use gemini_genai::{Error, files::UploadSource, types::UploadFileConfig};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use super::{data_path, test_client};

const START_MIME_HEADER: &str = "x-goog-upload-header-content-type";

/// The Python `config={...}` dict equivalent.
fn config_from_dict(dict: serde_json::Value) -> Result<UploadFileConfig, serde_json::Error> {
    serde_json::from_value(dict)
}

/// Uploads `source` to a mock resumable-upload server and checks the
/// request the client sent: the declared MIME type, the display name, and
/// that the single finalize chunk carries exactly `expected_bytes`.
async fn upload_and_check(
    source: UploadSource,
    config: Option<UploadFileConfig>,
    expected_mime: &str,
    expected_display_name: Option<&str>,
    expected_bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let session = format!("{}/upload-session/x", server.uri());
    Mock::given(method("POST"))
        .and(path("/upload/v1beta/files"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", session.as_str()),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/upload-session/x"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-status", "final")
                .set_body_json(serde_json::json!({"file": {"name": "files/uploaded"}})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let file = test_client(server.uri())
        .files()
        .upload(source, config)
        .await?;
    assert!(
        file.name
            .as_deref()
            .is_some_and(|n| n.starts_with("files/"))
    );

    let requests = server.received_requests().await.unwrap_or_default();
    let start = requests
        .iter()
        .find(|r| r.url.path() == "/upload/v1beta/files")
        .ok_or("no start request")?;
    let sent_mime = start
        .headers
        .get(START_MIME_HEADER)
        .and_then(|v| v.to_str().ok());
    assert_eq!(sent_mime, Some(expected_mime));
    let body: serde_json::Value = serde_json::from_slice(&start.body)?;
    assert_eq!(
        body["file"]["displayName"].as_str(),
        expected_display_name,
        "start body: {body}"
    );
    let chunk = requests
        .iter()
        .find(|r| r.url.path() == "/upload-session/x")
        .ok_or("no chunk request")?;
    assert_eq!(chunk.body, expected_bytes, "uploaded payload differs");
    server.verify().await;
    Ok(())
}

/// Uploads the bundled asset `name` from its path.
async fn upload_asset(
    name: &str,
    config: Option<UploadFileConfig>,
    expected_mime: &str,
    expected_display_name: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read(data_path(name))?;
    upload_and_check(
        UploadSource::from(data_path(name)),
        config,
        expected_mime,
        expected_display_name,
        &bytes,
    )
    .await
}

fn display(name: &str) -> UploadFileConfig {
    UploadFileConfig {
        display_name: Some(name.to_owned()),
        ..Default::default()
    }
}

fn dict_display(name: &str) -> Result<Option<UploadFileConfig>, serde_json::Error> {
    config_from_dict(serde_json::json!({"display_name": name})).map(Some)
}

// upstream-test: files/test_upload.py::test_image_png_upload
#[tokio::test]
async fn test_image_png_upload() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset("google.png", None, "image/png", None).await
}

// upstream-test: files/test_upload.py::test_image_png_upload_with_config
#[tokio::test]
async fn test_image_png_upload_with_config() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset(
        "google.png",
        Some(display("test_image_png")),
        "image/png",
        Some("test_image_png"),
    )
    .await
}

// upstream-test: files/test_upload.py::test_image_png_upload_with_config_dict
#[tokio::test]
async fn test_image_png_upload_with_config_dict() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset(
        "google.png",
        dict_display("test_image_png")?,
        "image/png",
        Some("test_image_png"),
    )
    .await
}

// upstream-test: files/test_upload.py::test_image_jpg_upload
#[tokio::test]
async fn test_image_jpg_upload() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset("google.jpg", None, "image/jpeg", None).await
}

// upstream-test: files/test_upload.py::test_image_jpg_upload_with_config
#[tokio::test]
async fn test_image_jpg_upload_with_config() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset(
        "google.jpg",
        Some(display("test_image_jpg")),
        "image/jpeg",
        Some("test_image_jpg"),
    )
    .await
}

// upstream-test: files/test_upload.py::test_image_jpg_upload_with_config_dict
#[tokio::test]
async fn test_image_jpg_upload_with_config_dict() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset(
        "google.jpg",
        dict_display("test_image_jpg")?,
        "image/jpeg",
        Some("test_image_jpg"),
    )
    .await
}

// upstream-test: files/test_upload.py::test_application_pdf_file_upload
#[tokio::test]
async fn test_application_pdf_file_upload() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset("story.pdf", None, "application/pdf", None).await
}

// upstream-test: files/test_upload.py::test_application_pdf_upload_with_config
#[tokio::test]
async fn test_application_pdf_upload_with_config() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset(
        "story.pdf",
        Some(display("test_application_pdf")),
        "application/pdf",
        Some("test_application_pdf"),
    )
    .await
}

// upstream-test: files/test_upload.py::test_application_pdf_upload_with_config_dict
#[tokio::test]
async fn test_application_pdf_upload_with_config_dict() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset(
        "story.pdf",
        dict_display("test_application_pdf")?,
        "application/pdf",
        Some("test_application_pdf"),
    )
    .await
}

// upstream-test: files/test_upload.py::test_video_mp4_file_upload
#[tokio::test]
async fn test_video_mp4_file_upload() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset("animal.mp4", None, "video/mp4", None).await
}

// upstream-test: files/test_upload.py::test_video_mp4_upload_with_config
#[tokio::test]
async fn test_video_mp4_upload_with_config() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset(
        "animal.mp4",
        Some(display("test_video_mp4")),
        "video/mp4",
        Some("test_video_mp4"),
    )
    .await
}

// upstream-test: files/test_upload.py::test_video_mp4_upload_with_config_dict
#[tokio::test]
async fn test_video_mp4_upload_with_config_dict() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset(
        "animal.mp4",
        dict_display("test_video_mp4")?,
        "video/mp4",
        Some("test_video_mp4"),
    )
    .await
}

// upstream-test: files/test_upload.py::test_image_png_upload_with_path
#[tokio::test]
async fn test_image_png_upload_with_path() -> Result<(), Box<dyn std::error::Error>> {
    // `pathlib.Path` -> `&Path`.
    let bytes = std::fs::read(data_path("google.png"))?;
    let p = data_path("google.png");
    upload_and_check(
        UploadSource::from(p.as_path()),
        Some(display("test_image_png_path")),
        "image/png",
        Some("test_image_png_path"),
        &bytes,
    )
    .await
}

// upstream-test: files/test_upload.py::test_image_png_upload_with_bytesio
#[tokio::test]
async fn test_image_png_upload_with_bytesio() -> Result<(), Box<dyn std::error::Error>> {
    // `io.BytesIO` -> in-memory bytes; the config MIME type wins over the
    // source's own.
    let bytes = std::fs::read(data_path("google.png"))?;
    let source = UploadSource::Bytes {
        data: bytes.clone(),
        mime_type: "application/octet-stream".to_owned(),
    };
    let config = UploadFileConfig {
        mime_type: Some("image/png".to_owned()),
        ..Default::default()
    };
    upload_and_check(source, Some(config), "image/png", None, &bytes).await
}

// upstream-test: files/test_upload.py::test_image_png_upload_with_fd
#[tokio::test]
async fn test_image_png_upload_with_fd() -> Result<(), Box<dyn std::error::Error>> {
    // An open file object has no Rust counterpart (no reader source); its
    // equivalent is the bytes read from the file, as for `BytesIO`.
    let bytes = tokio::fs::read(data_path("google.png")).await?;
    let source = UploadSource::Bytes {
        data: bytes.clone(),
        mime_type: "application/octet-stream".to_owned(),
    };
    let config = UploadFileConfig {
        mime_type: Some("image/png".to_owned()),
        ..Default::default()
    };
    upload_and_check(source, Some(config), "image/png", None, &bytes).await
}

// upstream-test: files/test_upload.py::test_audio_m4a_file_upload
#[tokio::test]
async fn test_audio_m4a_file_upload() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset(
        "pixel.m4a",
        Some(UploadFileConfig {
            mime_type: Some("audio/mp4".to_owned()),
            ..Default::default()
        }),
        "audio/mp4",
        None,
    )
    .await
}

// upstream-test: files/test_upload.py::test_audio_m4a_upload_with_config
#[tokio::test]
async fn test_audio_m4a_upload_with_config() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset(
        "pixel.m4a",
        Some(UploadFileConfig {
            display_name: Some("test_audio_m4a".to_owned()),
            mime_type: Some("audio/mp4".to_owned()),
            ..Default::default()
        }),
        "audio/mp4",
        Some("test_audio_m4a"),
    )
    .await
}

// upstream-test: files/test_upload.py::test_audio_m4a_upload_with_config_dict
#[tokio::test]
async fn test_audio_m4a_upload_with_config_dict() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset(
        "pixel.m4a",
        config_from_dict(
            serde_json::json!({"display_name": "test_audio_m4a", "mime_type": "audio/mp4"}),
        )
        .map(Some)?,
        "audio/mp4",
        Some("test_audio_m4a"),
    )
    .await
}

/// Starts a server that rejects the upload-start request the way the
/// service rejects an unsupported MIME type, then uploads `b"test"` with
/// `bad/mime_type` and returns the error.
async fn upload_with_bad_mime_type() -> Result<Error, Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/upload/v1beta/files"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": {
                "code": 400,
                "message": "Unsupported MIME type: bad/mime_type",
                "status": "INVALID_ARGUMENT",
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let source = UploadSource::Bytes {
        data: b"test".to_vec(),
        mime_type: "text/plain".to_owned(),
    };
    let config = config_from_dict(serde_json::json!({"mime_type": "bad/mime_type"}))?;
    let err = test_client(server.uri())
        .files()
        .upload(source, Some(config))
        .await
        .err()
        .ok_or("upload of an unsupported MIME type unexpectedly succeeded")?;
    server.verify().await;
    Ok(err)
}

// upstream-test: files/test_upload.py::test_bad_mime_type
#[tokio::test]
async fn test_bad_mime_type() -> Result<(), Box<dyn std::error::Error>> {
    let err = upload_with_bad_mime_type().await?;
    assert!(
        matches!(err, Error::Api(_)),
        "expected an API error: {err:?}"
    );
    assert!(err.to_string().contains("Unsupported MIME"), "{err}");
    Ok(())
}

// upstream-test: files/test_upload.py::test_bad_mime_type_async
#[tokio::test]
async fn test_bad_mime_type_async() -> Result<(), Box<dyn std::error::Error>> {
    let err = upload_with_bad_mime_type().await?;
    assert!(
        matches!(err, Error::Api(_)),
        "expected an API error: {err:?}"
    );
    assert!(err.to_string().contains("Unsupported MIME"), "{err}");
    Ok(())
}

// upstream-test: files/test_upload.py::test_image_upload_async
#[tokio::test]
async fn test_image_upload_async() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset("google.png", None, "image/png", None).await
}

// upstream-test: files/test_upload.py::test_image_upload_with_config_async
#[tokio::test]
async fn test_image_upload_with_config_async() -> Result<(), Box<dyn std::error::Error>> {
    upload_asset(
        "google.png",
        Some(display("test_image")),
        "image/png",
        Some("test_image"),
    )
    .await
}

// upstream-test: files/test_upload.py::test_image_upload_with_config_dict_async
#[tokio::test]
async fn test_image_upload_with_config_dict_async() -> Result<(), Box<dyn std::error::Error>> {
    let config = config_from_dict(serde_json::json!({
        "display_name": "test_image",
        "http_options": {"timeout": 8000},
    }))?;
    upload_asset("google.png", Some(config), "image/png", Some("test_image")).await
}

// upstream-test: files/test_upload.py::test_image_upload_with_bytesio_async
#[tokio::test]
async fn test_image_upload_with_bytesio_async() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read(data_path("google.png"))?;
    let source = UploadSource::Bytes {
        data: bytes.clone(),
        mime_type: "application/octet-stream".to_owned(),
    };
    let config = UploadFileConfig {
        mime_type: Some("image/png".to_owned()),
        ..Default::default()
    };
    upload_and_check(source, Some(config), "image/png", None, &bytes).await
}

// upstream-test: files/test_upload.py::test_unknown_path_upload_async
#[tokio::test]
async fn test_unknown_path_upload_async() -> Result<(), Box<dyn std::error::Error>> {
    // Python raises `FileNotFoundError("... is not a valid file path")`; the
    // Rust equivalent is an `Error::Io` with `NotFound`, raised before any
    // request is sent (the server here has no mocks, so a request would 404).
    let server = MockServer::start().await;
    let err = test_client(server.uri())
        .files()
        .upload("unknown_path", None)
        .await
        .err()
        .ok_or("upload of a missing path unexpectedly succeeded")?;
    assert!(
        matches!(&err, Error::Io(e) if e.kind() == std::io::ErrorKind::NotFound),
        "{err:?}"
    );
    assert!(
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty()
    );
    Ok(())
}
