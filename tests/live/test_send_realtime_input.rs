//! Ports of `google/genai/tests/live/test_send_realtime_input.py`.
//!
//! Developer-API behaviour only. Deviation: the top-level wire key is
//! camelCase `realtimeInput` (Python writes `realtime_input`; both are
//! accepted by the server).

use gemini_genai::{
    Error,
    live::RealtimeInput,
    types::{ActivityEnd, ActivityStart, Blob},
};
use serde_json::json;

use super::{get_value_ignore_key_case, run_send};

const BASE64_SIX_ZERO_BYTES: &str = "AAAAAAAA";

fn blob(mime_type: &str) -> Blob {
    Blob {
        data: Some(vec![0; 6]),
        mime_type: Some(mime_type.to_owned()),
        ..Default::default()
    }
}

/// A tiny stand-in for `tests/data/google.jpg` (only the MIME type matters).
fn image_jpeg() -> Blob {
    Blob {
        data: Some(vec![0xFF, 0xD8, 0xFF, 0xE0]),
        mime_type: Some("image/jpeg".to_owned()),
        ..Default::default()
    }
}

// upstream-test: live/test_send_realtime_input.py::test_send_media_blob_dict
#[tokio::test]
async fn test_send_media_blob_dict() {
    // Python builds the blob from a dict; Rust has only the typed `Blob`.
    let input = RealtimeInput {
        media: Some(blob("audio/pcm")),
        ..Default::default()
    };
    let (result, sent) = run_send(async |s| s.send_realtime_input(input).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("realtimeInput").is_some(), "{sent}");
    let chunk = &sent["realtimeInput"]["mediaChunks"][0];
    assert_eq!(chunk["data"], BASE64_SIX_ZERO_BYTES);
    assert_eq!(get_value_ignore_key_case(chunk, "mime_type"), "audio/pcm");
}

// upstream-test: live/test_send_realtime_input.py::test_send_media_blob
#[tokio::test]
async fn test_send_media_blob() {
    let input = RealtimeInput {
        media: Some(blob("audio/pcm")),
        ..Default::default()
    };
    let (result, sent) = run_send(async |s| s.send_realtime_input(input).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("realtimeInput").is_some(), "{sent}");
    let chunk = &sent["realtimeInput"]["mediaChunks"][0];
    assert_eq!(chunk["data"], BASE64_SIX_ZERO_BYTES);
    assert_eq!(get_value_ignore_key_case(chunk, "mime_type"), "audio/pcm");
}

// upstream-test: live/test_send_realtime_input.py::test_send_media_image
#[tokio::test]
async fn test_send_media_image() {
    let input = RealtimeInput {
        media: Some(image_jpeg()),
        ..Default::default()
    };
    let (result, sent) = run_send(async |s| s.send_realtime_input(input).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("realtimeInput").is_some(), "{sent}");
    assert_eq!(
        get_value_ignore_key_case(&sent["realtimeInput"]["mediaChunks"][0], "mime_type"),
        "image/jpeg"
    );
}

// upstream-test: live/test_send_realtime_input.py::test_send_audio
#[tokio::test]
async fn test_send_audio() {
    // Upstream runs a dict and a `Blob`; Rust has only the typed `Blob`.
    let input = RealtimeInput {
        audio: Some(blob("audio/pcm")),
        ..Default::default()
    };
    let (result, sent) = run_send(async |s| s.send_realtime_input(input).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("realtimeInput").is_some(), "{sent}");
    assert_eq!(
        sent["realtimeInput"]["audio"]["data"],
        BASE64_SIX_ZERO_BYTES
    );
    assert_eq!(
        get_value_ignore_key_case(&sent["realtimeInput"]["audio"], "mime_type"),
        "audio/pcm"
    );
}

// upstream-test: live/test_send_realtime_input.py::test_send_bad_audio_blob
#[tokio::test]
async fn test_send_bad_audio_blob() {
    let input = RealtimeInput {
        audio: Some(blob("image/png")),
        ..Default::default()
    };
    let (result, sent) = run_send(async |s| s.send_realtime_input(input).await).await;
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("nsupported mime type"),
        "unexpected error: {err}"
    );
    assert!(sent.is_none());
}

// upstream-test: live/test_send_realtime_input.py::test_send_bad_video_blob
#[tokio::test]
async fn test_send_bad_video_blob() {
    let input = RealtimeInput {
        video: Some(blob("audio/pcm")),
        ..Default::default()
    };
    let (result, sent) = run_send(async |s| s.send_realtime_input(input).await).await;
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("nsupported mime type"),
        "unexpected error: {err}"
    );
    assert!(sent.is_none());
}

// upstream-test: live/test_send_realtime_input.py::test_send_audio_stream_end
#[tokio::test]
async fn test_send_audio_stream_end() {
    let input = RealtimeInput {
        audio_stream_end: Some(true),
        ..Default::default()
    };
    let (result, sent) = run_send(async |s| s.send_realtime_input(input).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("realtimeInput").is_some(), "{sent}");
    assert_eq!(sent["realtimeInput"]["audioStreamEnd"], true);
}

// upstream-test: live/test_send_realtime_input.py::test_send_video
#[tokio::test]
async fn test_send_video() {
    let input = RealtimeInput {
        video: Some(blob("image/png")),
        ..Default::default()
    };
    let (result, sent) = run_send(async |s| s.send_realtime_input(input).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("realtimeInput").is_some(), "{sent}");
    assert_eq!(
        sent["realtimeInput"]["video"]["data"],
        BASE64_SIX_ZERO_BYTES
    );
    assert_eq!(
        get_value_ignore_key_case(&sent["realtimeInput"]["video"], "mime_type"),
        "image/png"
    );
}

// upstream-test: live/test_send_realtime_input.py::test_send_video_image
#[tokio::test]
async fn test_send_video_image() {
    let input = RealtimeInput {
        video: Some(image_jpeg()),
        ..Default::default()
    };
    let (result, sent) = run_send(async |s| s.send_realtime_input(input).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("realtimeInput").is_some(), "{sent}");
    assert_eq!(
        get_value_ignore_key_case(&sent["realtimeInput"]["video"], "mime_type"),
        "image/jpeg"
    );
}

// upstream-test: live/test_send_realtime_input.py::test_send_text
#[tokio::test]
async fn test_send_text() {
    let input = RealtimeInput {
        text: Some("Hello?".to_owned()),
        ..Default::default()
    };
    let (result, sent) = run_send(async |s| s.send_realtime_input(input).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("realtimeInput").is_some(), "{sent}");
    assert_eq!(sent["realtimeInput"]["text"], "Hello?");
}

// upstream-test: live/test_send_realtime_input.py::test_send_activity_start
#[tokio::test]
async fn test_send_activity_start() {
    let input = RealtimeInput {
        activity_start: Some(ActivityStart::default()),
        ..Default::default()
    };
    let (result, sent) = run_send(async |s| s.send_realtime_input(input).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("realtimeInput").is_some(), "{sent}");
    assert_eq!(sent["realtimeInput"]["activityStart"], json!({}));
}

// upstream-test: live/test_send_realtime_input.py::test_send_activity_end
#[tokio::test]
async fn test_send_activity_end() {
    let input = RealtimeInput {
        activity_end: Some(ActivityEnd::default()),
        ..Default::default()
    };
    let (result, sent) = run_send(async |s| s.send_realtime_input(input).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("realtimeInput").is_some(), "{sent}");
    assert_eq!(sent["realtimeInput"]["activityEnd"], json!({}));
}

// upstream-test: live/test_send_realtime_input.py::test_send_multiple_args
#[tokio::test]
async fn test_send_multiple_args() {
    let input = RealtimeInput {
        text: Some("Hello?".to_owned()),
        activity_start: Some(ActivityStart::default()),
        ..Default::default()
    };
    let (result, sent) = run_send(async |s| s.send_realtime_input(input).await).await;
    let err = result.unwrap_err();
    assert!(
        matches!(&err, Error::Validation(message) if message.contains("one argument")),
        "unexpected error: {err:?}"
    );
    assert!(sent.is_none());
}
