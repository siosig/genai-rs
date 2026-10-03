//! Ports of `google/genai/tests/files/test_download.py`.
//!
//! Upstream drives `Files.download(file, destination=...)`, where
//! `destination` is a path, a `pathlib.Path`, or a writable object. The Rust
//! API splits that union: `download` returns the bytes, `download_to_path`
//! writes to a path, and `download_stream` hands back the chunks, which
//! callers write wherever they like (the `io.BytesIO` equivalent is
//! collecting the stream into a `Vec<u8>`). Replays and `mock.patch` calls
//! upstream become `wiremock` servers here.
//!
//! Documented deviation: Python also stores the downloaded bytes in
//! `Video.video_bytes`; this crate does not (see `Files::download_stream`).

use bytes::Bytes;
use futures_util::TryStreamExt as _;
use gemini_genai::{
    __test_support::transformers::t_file_name,
    files::FileSource,
    types::{File, GeneratedVideo, Video},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

use super::test_client;

/// A minimal MP4 header: bytes 4..8 are the `ftyp` box tag.
const MP4_HEADER: &[u8] = b"\0\0\0\x18ftypmp42\0\0\0\0mp42isom";

/// Mounts `GET /v1beta/files/<id>:download?alt=media` answering `body`.
async fn mount_download(server: &MockServer, id: &str, body: &[u8], expected_calls: u64) {
    Mock::given(method("GET"))
        .and(path(format!("/v1beta/files/{id}:download")))
        .and(query_param("alt", "media"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
        .expect(expected_calls)
        .mount(server)
        .await;
}

/// A generated (hence downloadable) file, the stand-in for upstream's
/// `_get_downloadable_file`.
fn downloadable_file(base: &str, id: &str) -> File {
    File {
        name: Some(format!("files/{id}")),
        uri: Some(format!("{base}/v1beta/files/{id}")),
        download_uri: Some(format!("{base}/v1beta/files/{id}:download?alt=media")),
        ..Default::default()
    }
}

fn video(uri: &str) -> Video {
    Video {
        uri: Some(uri.to_owned()),
        ..Default::default()
    }
}

/// Drains a download stream into one buffer (the `io.BytesIO` destination).
async fn collect(
    client: &gemini_genai::Client,
    file: impl Into<FileSource>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let chunks: Vec<Bytes> = client
        .files()
        .download_stream(file, None)
        .await?
        .try_collect()
        .await?;
    Ok(chunks.concat())
}

fn temp_destination(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "gemini-genai-download-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}

// upstream-test: files/test_download.py::test_name_transform_name
#[tokio::test]
async fn test_name_transform_name() -> Result<(), Box<dyn std::error::Error>> {
    // Every accepted spelling of the same file must resolve to one id.
    // `https://` URIs are checked directly on `t_file_name` (a mock server
    // is `http://`, which Python also passes through unchanged); the
    // object forms go through the real request path.
    let https_uri = "https://generativelanguage.googleapis.com/v1beta/files/abc123";
    let download_uri = format!("{https_uri}:download?alt=media");
    for spelling in [https_uri, download_uri.as_str(), "files/abc123", "abc123"] {
        let id = t_file_name(serde_json::json!(spelling))?;
        assert_eq!(id, serde_json::json!("abc123"), "spelling {spelling}");
    }

    let server = MockServer::start().await;
    let base = server.uri();
    let sources: Vec<FileSource> = vec![
        downloadable_file(&base, "abc123").into(),
        "abc123".into(),
        "files/abc123".into(),
        video("files/abc123").into(),
        GeneratedVideo {
            video: Some(video("files/abc123")),
        }
        .into(),
    ];
    mount_download(&server, "abc123", MP4_HEADER, sources.len() as u64).await;

    let client = test_client(base);
    for source in sources {
        assert_eq!(collect(&client, source).await?, MP4_HEADER);
    }
    server.verify().await;
    Ok(())
}

// upstream-test: files/test_download.py::test_basic_download
#[tokio::test]
async fn test_basic_download() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_download(&server, "abc123", MP4_HEADER, 1).await;

    let content = test_client(server.uri())
        .files()
        .download("files/abc123", None)
        .await?;
    assert_eq!(&content[4..8], b"ftyp");
    server.verify().await;
    Ok(())
}

// upstream-test: files/test_download.py::test_basic_download_async
#[tokio::test]
async fn test_basic_download_async() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_download(&server, "abc123", MP4_HEADER, 1).await;

    let client = test_client(server.uri());
    let content = collect(&client, downloadable_file(&server.uri(), "abc123")).await?;
    assert_eq!(&content[4..8], b"ftyp");
    server.verify().await;
    Ok(())
}

/// Downloads a generated file to a fresh path and checks its contents.
async fn download_file_to_destination(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_download(&server, "abc123", MP4_HEADER, 1).await;

    let destination = temp_destination(name);
    test_client(server.uri())
        .files()
        .download_to_path(
            downloadable_file(&server.uri(), "abc123"),
            &destination,
            None,
        )
        .await?;
    let written = tokio::fs::read(&destination).await?;
    tokio::fs::remove_file(&destination).await.ok();
    assert_eq!(&written[4..8], b"ftyp");
    server.verify().await;
    Ok(())
}

// upstream-test: files/test_download.py::test_destination_download
#[tokio::test]
async fn test_destination_download() -> Result<(), Box<dyn std::error::Error>> {
    download_file_to_destination("downloaded.mp4").await
}

// upstream-test: files/test_download.py::test_async_destination_download
#[tokio::test]
async fn test_async_destination_download() -> Result<(), Box<dyn std::error::Error>> {
    download_file_to_destination("downloaded_async.mp4").await
}

// upstream-test: files/test_download.py::test_destination_filepath
#[tokio::test]
async fn test_destination_filepath() -> Result<(), Box<dyn std::error::Error>> {
    // A `str` destination.
    let server = MockServer::start().await;
    mount_download(&server, "test_123", b"payload", 1).await;

    let destination = temp_destination("out.mp4").to_string_lossy().into_owned();
    test_client(server.uri())
        .files()
        .download_to_path("files/test_123", destination.as_str(), None)
        .await?;
    let written = tokio::fs::read(&destination).await?;
    tokio::fs::remove_file(&destination).await.ok();
    assert_eq!(written, b"payload");
    server.verify().await;
    Ok(())
}

// upstream-test: files/test_download.py::test_destination_pathlib
#[tokio::test]
async fn test_destination_pathlib() -> Result<(), Box<dyn std::error::Error>> {
    // A `pathlib.Path` destination.
    let server = MockServer::start().await;
    mount_download(&server, "test_123", b"payload", 1).await;

    let destination = temp_destination("out.mp4");
    test_client(server.uri())
        .files()
        .download_to_path("files/test_123", destination.as_path(), None)
        .await?;
    let written = tokio::fs::read(&destination).await?;
    tokio::fs::remove_file(&destination).await.ok();
    assert_eq!(written, b"payload");
    server.verify().await;
    Ok(())
}

// upstream-test: files/test_download.py::test_destination_bytesio
#[tokio::test]
async fn test_destination_bytesio() -> Result<(), Box<dyn std::error::Error>> {
    // An `io.BytesIO` destination: the stream collected into one buffer.
    let server = MockServer::start().await;
    mount_download(&server, "test_123", b"payload", 1).await;

    let buffer = collect(&test_client(server.uri()), "files/test_123").await?;
    assert_eq!(buffer, b"payload");
    server.verify().await;
    Ok(())
}

/// The three `Video`/`GeneratedVideo` cases of upstream's
/// `test_video_destination_behavior`, minus the `video_bytes` side effect.
async fn video_destination_behavior() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_download(&server, "testvideo", b"video_data", 3).await;
    let client = test_client(server.uri());
    let uri = "https://generativelanguage.googleapis.com/v1beta/files/testvideo";

    // No destination: the bytes come back.
    assert_eq!(collect(&client, video(uri)).await?, b"video_data");

    // A destination: the bytes go there.
    let destination = temp_destination("video.mp4");
    client
        .files()
        .download_to_path(video(uri), &destination, None)
        .await?;
    let written = tokio::fs::read(&destination).await?;
    tokio::fs::remove_file(&destination).await.ok();
    assert_eq!(written, b"video_data");

    // A `GeneratedVideo` resolves through its inner video.
    let generated = GeneratedVideo {
        video: Some(video(uri)),
    };
    assert_eq!(collect(&client, generated).await?, b"video_data");
    server.verify().await;
    Ok(())
}

// upstream-test: files/test_download.py::test_video_destination_behavior
#[tokio::test]
async fn test_video_destination_behavior() -> Result<(), Box<dyn std::error::Error>> {
    video_destination_behavior().await
}

// upstream-test: files/test_download.py::test_async_video_destination_behavior
#[tokio::test]
async fn test_async_video_destination_behavior() -> Result<(), Box<dyn std::error::Error>> {
    video_destination_behavior().await
}

// upstream-test: files/test_download.py::test_async_destination
#[tokio::test]
async fn test_async_destination() -> Result<(), Box<dyn std::error::Error>> {
    download_file_to_destination("out_async.mp4").await
}

// upstream-test: files/test_download.py::test_async_destination_bytesio
#[tokio::test]
async fn test_async_destination_bytesio() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_download(&server, "test_123", b"payload", 1).await;

    let buffer = collect(&test_client(server.uri()), "files/test_123").await?;
    assert_eq!(buffer, b"payload");
    server.verify().await;
    Ok(())
}

/// The body `b"chunk1chunk2"` must arrive, in order, in the buffer.
async fn chunks_are_written_in_order() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    mount_download(&server, "test_123", b"chunk1chunk2", 1).await;

    let buffer = collect(&test_client(server.uri()), "files/test_123").await?;
    assert_eq!(buffer, b"chunk1chunk2");
    server.verify().await;
    Ok(())
}

// upstream-test: files/test_download.py::test_async_destination_bytesio_writes_chunks
#[tokio::test]
async fn test_async_destination_bytesio_writes_chunks() -> Result<(), Box<dyn std::error::Error>> {
    chunks_are_written_in_order().await
}

// upstream-test: files/test_download.py::test_async_httpx_destination_bytesio_writes_chunks
#[tokio::test]
async fn test_async_httpx_destination_bytesio_writes_chunks()
-> Result<(), Box<dyn std::error::Error>> {
    chunks_are_written_in_order().await
}
