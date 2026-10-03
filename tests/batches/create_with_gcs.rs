//! Ports of `batches/test_create_with_gcs.py`.

use gemini_genai::{Error, types::BatchJobSource};

use crate::common::test_client;

const GEMINI_MODEL: &str = "gemini-2.5-flash";
const GCS_INPUT: &str = "gs://unified-genai-tests/batches/input/generate_content_requests.jsonl";

// Python asserts the Vertex result and, on the Gemini Developer API, that
// `create` raises `ValueError` ("not supported in Gemini API"). Only the
// Developer-API half is reproducible here.
// upstream-test: batches/test_create_with_gcs.py::test_async_create
#[tokio::test]
async fn test_async_create() {
    let client = test_client("http://127.0.0.1:1".to_owned());
    let src = BatchJobSource {
        gcs_uri: Some(vec![GCS_INPUT.to_owned()]),
        format: Some("jsonl".to_owned()),
        ..Default::default()
    };
    let err = client
        .batches()
        .create(GEMINI_MODEL, src, None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::UnsupportedByBackend { .. } | Error::Validation(_)
        ),
        "expected the GCS source to be rejected on the Developer API, got {err:?}"
    );
}
