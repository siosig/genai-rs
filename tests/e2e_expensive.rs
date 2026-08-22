//! Expensive / long-running end-to-end tests against the **live** Gemini
//! Developer API: video generation, batch jobs, and Live (WebSocket)
//! sessions.
//!
//! These are separated from `tests/e2e.rs` because they either cost
//! meaningfully more quota, take minutes rather than seconds, or hold an
//! open socket. They require **both** an API key and an explicit opt-in:
//!
//! ```sh
//! GEMINI_API_KEY=... GENAI_E2E_EXPENSIVE=1 \
//!   cargo test --all-features --test e2e_expensive -- --ignored --nocapture
//! ```
//!
//! Without `GENAI_E2E_EXPENSIVE=1` each test skips (rather than fails), so
//! running the whole `--ignored` set stays safe by default.
//!
//! Tuning (US8 scenario 1) *is* covered below, but the test skips today:
//! the Gemini Developer API answers `POST /v1beta/tunedModels` with
//! **501 UNIMPLEMENTED** for every base model tried (`gemini-flash-latest`,
//! `gemini-2.5-flash`, and the legacy `gemini-1.5-flash-001-tuning`), and
//! `GET /v1beta/tunedModels` answers 501 as well, so no tuning job can be
//! created against this backend at all. The test therefore treats a 501 as
//! "not served" and skips, and starts exercising the real flow the moment
//! the endpoint comes back.
//!
//! An earlier version of this comment gave a different reason -- that a
//! tuned model could never be cleaned up, because `tunings().list` is
//! Vertex-AI-only here. That reason was wrong. Cleanup is possible:
//! `models().delete(name)` runs `name` through `t_model`
//! (`src/transformers.rs`), which passes a `tunedModels/...` prefix
//! straight through, so it issues `DELETE /v1beta/tunedModels/{id}`; and
//! `models().list(..)` with `query_base: Some(false)` enumerates tuned
//! models via `t_models_url`. The test below does exactly that cleanup.

use futures_util::StreamExt;
use google_genai::types::{
    BatchJobSource, Content, CreateBatchJobConfig, CreateTuningJobConfig, GenerateVideosSource,
    InlinedRequest, JobState, LiveConnectConfig, Modality, Part, TuningDataset, TuningExample,
};
use google_genai::{Client, Error};

/// A currently-served video-generation model.
const VIDEO_MODEL: &str = "veo-3.1-fast-generate-preview";
/// A currently-served Live (bidirectional WebSocket) model.
const LIVE_MODEL: &str = "gemini-3.1-flash-live-preview";
/// A small text model, used for batch requests.
const TEXT_MODEL: &str = "gemini-flash-latest";
/// The base model a tuning job would be built on.
const TUNING_BASE_MODEL: &str = TEXT_MODEL;

/// Builds a live client, or returns `None` when the API key or the
/// expensive-test opt-in is missing (in which case the caller skips).
fn expensive_client() -> Option<Client> {
    if std::env::var("GENAI_E2E_EXPENSIVE").as_deref() != Ok("1") {
        eprintln!("skipping: set GENAI_E2E_EXPENSIVE=1 to run expensive live tests");
        return None;
    }
    if std::env::var("GOOGLE_API_KEY").is_err() && std::env::var("GEMINI_API_KEY").is_err() {
        eprintln!("skipping: neither GOOGLE_API_KEY nor GEMINI_API_KEY is set");
        return None;
    }
    match Client::new() {
        Ok(client) => Some(client),
        Err(error) => panic!("building a live client failed: {error}"),
    }
}

/// Expands to an early `return` when the expensive-test preconditions
/// aren't met.
macro_rules! client_or_skip {
    () => {
        match expensive_client() {
            Some(client) => client,
            None => return,
        }
    };
}

/// US7: `generate_videos` starts a long-running operation, and
/// `operations().get` polls it to completion.
///
/// Video generation routinely takes minutes, so this polls with a cap and
/// reports (rather than fails) if the operation is still running when the
/// cap is hit -- the point of the test is that the request/poll round-trip
/// works, not that Google's queue is fast.
#[tokio::test]
#[ignore = "expensive: generates a video; needs GENAI_E2E_EXPENSIVE=1"]
async fn test_e2e_generate_videos_and_poll_operation() {
    let client = client_or_skip!();
    let source = GenerateVideosSource {
        prompt: Some("A close-up of a single drop of water falling into a still pond.".to_owned()),
        ..Default::default()
    };
    let mut operation = client
        .models()
        .generate_videos(VIDEO_MODEL, source, None)
        .await
        .expect("generate_videos failed");

    assert!(
        operation.name.as_deref().is_some_and(|n| !n.is_empty()),
        "operation has no name to poll"
    );
    eprintln!("started operation {:?}", operation.name);

    for attempt in 1..=20 {
        if operation.done == Some(true) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        operation = client
            .operations()
            .get(&operation)
            .await
            .expect("operations get failed");
        eprintln!("poll {attempt}: done={:?}", operation.done);
    }

    if operation.done != Some(true) {
        eprintln!("operation still running after the poll cap; the round-trip itself succeeded");
        return;
    }
    if let Some(error) = &operation.error {
        panic!("video generation reported an error: {error:?}");
    }
    let videos = operation
        .response
        .and_then(|r| r.generated_videos)
        .expect("completed operation carried no generated_videos");
    assert!(!videos.is_empty(), "no videos in the completed operation");
}

/// US8: a batch job is created, fetched, and cancelled.
///
/// The job is cancelled immediately rather than awaited: batch turnaround
/// is measured in hours, and leaving it running would consume quota long
/// after the test exits.
#[tokio::test]
#[ignore = "expensive: creates a batch job; needs GENAI_E2E_EXPENSIVE=1"]
async fn test_e2e_batch_create_get_cancel() {
    let client = client_or_skip!();
    let src = BatchJobSource {
        inlined_requests: Some(vec![InlinedRequest {
            model: Some(format!("models/{TEXT_MODEL}")),
            contents: Some(vec![Content {
                role: Some("user".to_owned()),
                parts: Some(vec![Part::from_text("Say hello.")]),
            }]),
            ..Default::default()
        }]),
        ..Default::default()
    };

    let job = client
        .batches()
        .create(
            TEXT_MODEL,
            src,
            Some(CreateBatchJobConfig {
                display_name: Some("genai-rs e2e smoke".to_owned()),
                ..Default::default()
            }),
        )
        .await
        .expect("batches create failed");

    let name = job.name.clone().expect("created batch job has no name");
    eprintln!("created batch job {name}");

    let fetched = client
        .batches()
        .get(&name, None)
        .await
        .expect("batches get failed");
    assert_eq!(fetched.name.as_deref(), Some(name.as_str()));

    client
        .batches()
        .cancel(&name, None)
        .await
        .expect("batches cancel failed");
}

/// US9: a Live session completes the WebSocket handshake, accepts a turn,
/// and streams the model's reply back.
///
/// `AUDIO` is requested rather than `TEXT`: the served `*-live-preview`
/// models are audio-native and close the socket outright on
/// `responseModalities: [TEXT]` ("The requested combination of response
/// modalities (TEXT) is not supported by the model"), verified against the
/// live endpoint.
#[cfg(feature = "live")]
#[tokio::test]
#[ignore = "expensive: opens a Live WebSocket session; needs GENAI_E2E_EXPENSIVE=1"]
async fn test_e2e_live_session_audio_turn() {
    let client = client_or_skip!();
    let config = LiveConnectConfig {
        response_modalities: Some(vec![Modality::Audio]),
        ..Default::default()
    };
    let mut session = Box::pin(client.live().connect(LIVE_MODEL, Some(config)))
        .await
        .expect("live connect failed");

    session
        .send_client_content(
            Some(vec![Content {
                role: Some("user".to_owned()),
                parts: Some(vec![Part::from_text("Reply with exactly: OK")]),
            }]),
            true,
        )
        .await
        .expect("send_client_content failed");

    let mut audio_bytes = 0_usize;
    let mut saw_turn_complete = false;
    {
        let mut stream = Box::pin(session.receive());
        // Guard against a server that never sends turnComplete.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        while let Ok(Some(message)) = tokio::time::timeout_at(deadline, stream.next()).await {
            let message = message.expect("live stream yielded an error");
            if let Some(content) = &message.server_content {
                if let Some(turn) = &content.model_turn {
                    for part in turn.parts.iter().flatten() {
                        if let Some(blob) = &part.inline_data {
                            audio_bytes += blob.data.as_ref().map_or(0, Vec::len);
                        }
                    }
                }
                if content.turn_complete == Some(true) {
                    saw_turn_complete = true;
                    break;
                }
            }
        }
    }

    session.close().await.expect("live close failed");
    assert!(saw_turn_complete, "never received turnComplete");
    assert!(
        audio_bytes > 0,
        "live session produced no audio data (received {audio_bytes} bytes)"
    );
}

/// US8 scenario 1: a tuning job is created, its name and state come back,
/// `tunings().get` confirms it, and the tuned model the job produced is
/// deleted again so the test leaves nothing behind.
///
/// Skips (rather than fails) on `501 UNIMPLEMENTED`, which is what the
/// Gemini Developer API currently answers for `POST /v1beta/tunedModels`
/// -- see this file's module doc. `tunings().list` is not used: it is
/// Vertex-AI-only in the upstream SDK and this crate returns
/// `UnsupportedByBackend` for it (`tests/tunings.rs` covers that), so the
/// created job is confirmed with `tunings().get` instead.
#[tokio::test]
#[ignore = "expensive: creates a tuned model; needs GENAI_E2E_EXPENSIVE=1"]
async fn test_e2e_tuning_create_get_and_delete_tuned_model() {
    let client = client_or_skip!();
    let dataset = TuningDataset {
        examples: Some(
            [
                ("1 + 1", "2"),
                ("2 + 2", "4"),
                ("3 + 3", "6"),
                ("4 + 4", "8"),
            ]
            .into_iter()
            .map(|(text_input, output)| TuningExample {
                text_input: Some(text_input.to_owned()),
                output: Some(output.to_owned()),
            })
            .collect(),
        ),
        ..Default::default()
    };

    let job = match client
        .tunings()
        .tune(
            TUNING_BASE_MODEL,
            dataset,
            Some(CreateTuningJobConfig {
                tuned_model_display_name: Some("genai-rs e2e tuning smoke".to_owned()),
                epoch_count: Some(1),
                ..Default::default()
            }),
        )
        .await
    {
        Ok(job) => job,
        Err(Error::Api(error)) if error.code == 501 => {
            eprintln!(
                "skipping: tuning is not served on this backend \
                 (POST /v1beta/tunedModels -> 501 {}): {}",
                error.status.as_deref().unwrap_or("UNIMPLEMENTED"),
                error.message
            );
            return;
        }
        Err(error) => panic!("tunings tune failed: {error}"),
    };

    let name = job.name.clone().expect("created tuning job has no name");
    eprintln!("created tuning job {name}");
    assert!(
        name.starts_with("tunedModels/"),
        "tuning job name is not a tunedModels resource: {name}"
    );
    assert_eq!(
        job.state,
        Some(JobState::JobStateQueued),
        "tune() should synthesize a queued stub job"
    );

    let fetched = client.tunings().get(&name, None).await;

    // Clean up before asserting on the fetch, so a surprising `get`
    // response can't leave a tuned model behind on the project.
    // `models().delete` accepts a `tunedModels/...` name unchanged, so
    // this is `DELETE /v1beta/{name}`.
    let deleted = client.models().delete(&name, None).await;

    let fetched = fetched.expect("tunings get failed");
    assert_eq!(fetched.name.as_deref(), Some(name.as_str()));
    assert!(fetched.state.is_some(), "fetched job carried no state");
    deleted.unwrap_or_else(|error| panic!("deleting tuned model {name} failed: {error}"));
}
