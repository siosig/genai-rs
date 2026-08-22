//! Integration tests for `client.tunings()` (fine-tuning job
//! create/get/list/cancel). Runs against the public API only, via
//! `wiremock`, mirroring `src/tunings.rs`'s own unit tests but exercised
//! from outside the crate.

mod common;

use common::test_client;
use gemini_genai::{
    Error,
    types::{CancelTuningJobConfig, CreateTuningJobConfig, JobState, TuningDataset, TuningExample},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path},
};

fn dataset() -> TuningDataset {
    TuningDataset {
        examples: Some(vec![
            TuningExample {
                text_input: Some("1 + 1".to_owned()),
                output: Some("2".to_owned()),
            },
            TuningExample {
                text_input: Some("2 + 3".to_owned()),
                output: Some("5".to_owned()),
            },
        ]),
        ..Default::default()
    }
}

#[tokio::test]
async fn tune_creates_a_job_and_returns_a_queued_stub() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/tunedModels"))
        .and(body_json(serde_json::json!({
            "baseModel": "models/gemini-1.5-flash-001",
            "tuningTask": {"hyperparameters": {"epochCount": 3}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "tunedModels/my-tuned-model/operations/1",
            "metadata": {"tunedModel": "tunedModels/my-tuned-model"},
            "done": false
        })))
        .expect(1)
        .mount(&server)
        .await;

    let config = CreateTuningJobConfig {
        epoch_count: Some(3),
        ..Default::default()
    };
    let job = test_client(server.uri())
        .tunings()
        .tune("models/gemini-1.5-flash-001", dataset(), Some(config))
        .await
        .unwrap();

    assert_eq!(job.name.as_deref(), Some("tunedModels/my-tuned-model"));
    assert_eq!(job.state, Some(JobState::JobStateQueued));
    server.verify().await;
}

#[tokio::test]
async fn tune_rejects_a_config_field_only_supported_by_vertex_ai() {
    let server = MockServer::start().await;
    let config = CreateTuningJobConfig {
        labels: Some(std::collections::HashMap::from([(
            "env".to_owned(),
            "prod".to_owned(),
        )])),
        ..Default::default()
    };

    let err = test_client(server.uri())
        .tunings()
        .tune("models/gemini-1.5-flash-001", dataset(), Some(config))
        .await
        .unwrap_err();

    match err {
        Error::UnsupportedByBackend { field, .. } => assert_eq!(field, "labels"),
        other => panic!("expected Error::UnsupportedByBackend, got {other:?}"),
    }
}

#[tokio::test]
async fn get_fetches_a_tuning_job_by_resource_name() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/tunedModels/my-tuned-model"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "tunedModels/my-tuned-model",
            "state": "JOB_STATE_SUCCEEDED",
            "baseModel": "models/gemini-1.5-flash-001",
            "createTime": "2026-01-01T00:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let job = test_client(server.uri())
        .tunings()
        .get("tunedModels/my-tuned-model", None)
        .await
        .unwrap();

    assert_eq!(job.name.as_deref(), Some("tunedModels/my-tuned-model"));
    assert_eq!(job.state, Some(JobState::JobStateSucceeded));
    assert_eq!(job.create_time.as_deref(), Some("2026-01-01T00:00:00Z"));
    server.verify().await;
}

#[tokio::test]
async fn cancel_posts_to_the_cancel_suffix_and_succeeds_on_an_empty_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/tunedModels/my-tuned-model:cancel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(1)
        .mount(&server)
        .await;

    test_client(server.uri())
        .tunings()
        .cancel(
            "tunedModels/my-tuned-model",
            Some(CancelTuningJobConfig::default()),
        )
        .await
        .unwrap();
    server.verify().await;
}

#[tokio::test]
async fn cancel_maps_a_not_found_response_to_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": {"code": 404, "message": "no such job", "status": "NOT_FOUND"}
        })))
        .mount(&server)
        .await;

    let err = test_client(server.uri())
        .tunings()
        .cancel("tunedModels/missing", None)
        .await
        .unwrap_err();

    match err {
        Error::Api(api_err) => assert_eq!(api_err.code, 404),
        other => panic!("expected Error::Api, got {other:?}"),
    }
}

/// `Tunings::list` mirrors the upstream Python SDK's `Tunings._list`,
/// which raises `ValueError` unless the client is configured for Vertex
/// AI, before ever building a request. This crate implements only the
/// Gemini Developer API backend, so `list` always fails the same way —
/// no HTTP call is made (no mock is mounted, and `MockServer` has no
/// expectations to verify).
#[tokio::test]
async fn list_is_unsupported_by_the_gemini_developer_api_backend() {
    let server = MockServer::start().await;

    let err = test_client(server.uri())
        .tunings()
        .list(None)
        .await
        .unwrap_err();

    match err {
        Error::UnsupportedByBackend { .. } => {}
        other => panic!("expected Error::UnsupportedByBackend, got {other:?}"),
    }
}
