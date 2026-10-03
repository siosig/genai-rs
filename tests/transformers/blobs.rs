//! Port of `transformers/test_blobs.py`. The PIL-based tests are excluded
//! (see `tools/codegen/upstream_tests.toml`).

use gemini_genai::{__test_support::transformers as t, types::Blob};
use serde_json::json;

// upstream-test: transformers/test_blobs.py::test_blob_dict
#[test]
fn test_blob_dict() {
    let blob = t::t_blob(json!({"data": "AAAAAAAA", "mime_type": "audio/pcm"})).unwrap();
    let blob: Blob = serde_json::from_value(blob).unwrap();
    assert_eq!(blob.data, Some(vec![0; 6]));
    assert_eq!(blob.mime_type.as_deref(), Some("audio/pcm"));
}

// upstream-test: transformers/test_blobs.py::test_blob
#[test]
fn test_blob() {
    let input = serde_json::to_value(Blob {
        data: Some(vec![0; 6]),
        mime_type: Some("audio/pcm".to_owned()),
        ..Default::default()
    })
    .unwrap();
    let blob: Blob = serde_json::from_value(t::t_blob(input).unwrap()).unwrap();
    assert_eq!(blob.data, Some(vec![0; 6]));
    assert_eq!(blob.mime_type.as_deref(), Some("audio/pcm"));
}
