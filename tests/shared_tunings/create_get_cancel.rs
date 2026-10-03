use gemini_genai::{
    Error,
    types::{CreateTuningJobConfig, TuningDataset},
};
use wiremock::MockServer;

use super::common::test_client;

// upstream-test: shared/tunings/test_create_get_cancel.py::test_create_get_cancel
#[tokio::test]
async fn test_create_get_cancel() {
    // Python: on the Gemini Developer API a `gcs_uri` training dataset (and
    // so the whole create/get/cancel flow) fails with "only supported in
    // Gemini Enterprise Agent Platform mode". No request is made.
    let server = MockServer::start().await;
    let dataset = TuningDataset {
        gcs_uri: Some(
            "gs://cloud-samples-data/ai-platform/generative_ai/gemini-2_0/text/sft_train_data.jsonl"
                .to_owned(),
        ),
        ..Default::default()
    };

    let error = test_client(server.uri())
        .tunings()
        .tune(
            "gemini-2.5-flash",
            dataset,
            Some(CreateTuningJobConfig {
                epoch_count: Some(1),
                ..Default::default()
            }),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(error, Error::UnsupportedByBackend { .. }),
        "expected UnsupportedByBackend, got {error:?}"
    );
    assert!(
        error
            .to_string()
            .contains("only supported in Gemini Enterprise Agent Platform mode"),
        "{error}"
    );
}
