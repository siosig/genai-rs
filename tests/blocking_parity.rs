//! Integration tests for `google_genai::blocking` (`feature = "blocking"`):
//! `Client` construction, one `wiremock`-backed call of each generated
//! wrapper shape (unary/stream/pager -- see `tools/codegen/methods.toml`'s
//! `kind` field), and the `Error::BlockingInsideRuntime` guard.
//!
//! Supersedes the never-actually-implemented blocking-wrapper portions of
//! tasks T037/T052/T061/T067/T072/T081 (each marked done in a prior
//! session without a `blocking` counterpart ever landing); this file
//! covers that gap for every story via a representative sample of each
//! wrapper `kind` rather than one test per module (`src/blocking/mod.rs`'s
//! doc comments explain why every module's wrapper shares the same
//! `Runtime::block_on` plumbing, so one exercised example per `kind` is
//! sufficient signal that the generator/hand-written split works).

#![cfg(feature = "blocking")]

use std::future::Future;

use google_genai::Error;
use google_genai::blocking::Client;
use google_genai::types::HttpOptions;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[expect(
    clippy::unwrap_used,
    reason = "test helper: a broken Client::builder() here is a test-setup bug, not a runtime condition"
)]
fn test_client(base_url: String) -> Client {
    Client::builder()
        .api_key("test-key")
        .http_options(HttpOptions {
            base_url: Some(base_url),
            ..Default::default()
        })
        .build()
        .unwrap()
}

/// Runs `f` on a plain OS thread with no Tokio runtime context at all, so
/// `blocking::Runtime::block_on` doesn't see itself as nested (which is
/// exactly what the "happy path" tests below need: they run inside
/// `#[tokio::test]` only to get an async `MockServer::start()`, and the
/// blocking calls under test must not observe that outer runtime).
#[expect(
    clippy::unwrap_used,
    reason = "test helper: a broken std::thread::spawn/join here is a test-infrastructure bug, not a runtime condition"
)]
fn run_off_runtime<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(f).join().unwrap()
}

#[test]
fn client_new_and_builder_construct_without_a_running_call() {
    // `Client::new()` reads GOOGLE_API_KEY/GEMINI_API_KEY from the
    // environment; exercise the always-available builder path instead so
    // this test doesn't depend on the process environment. A plain
    // `#[test]` (no Tokio runtime at all) confirms building a
    // `blocking::Client` -- which builds its own dedicated runtime --
    // needs no ambient async context.
    let client = Client::builder().api_key("test-key").build();
    assert!(client.is_ok());
}

#[tokio::test]
async fn generate_content_blocks_on_a_unary_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-flash:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "hello"}]},
                "finishReason": "STOP"
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let base_url = server.uri();
    let text = run_off_runtime(move || {
        let response = test_client(base_url)
            .models()
            .generate_content("gemini-2.5-flash", "hi", None)
            .unwrap();
        response.text().unwrap_or_default()
    });
    assert_eq!(text, "hello");
    server.verify().await;
}

#[tokio::test]
async fn generate_content_stream_yields_chunks_via_iterator() {
    let server = MockServer::start().await;
    let sse_body = concat!(
        "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hel\"}]}}]}\n\n",
        "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"lo\"}]},\"finishReason\":\"STOP\"}]}\n\n",
    );
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_body)
                .insert_header("content-type", "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let base_url = server.uri();
    let texts = run_off_runtime(move || {
        let stream = test_client(base_url)
            .models()
            .generate_content_stream("gemini-2.5-flash", "hi", None)
            .unwrap();
        stream
            .map(|item| item.unwrap().text().unwrap_or_default())
            .collect::<Vec<_>>()
    });
    assert_eq!(texts, vec!["Hel".to_owned(), "lo".to_owned()]);
    server.verify().await;
}

#[tokio::test]
async fn list_paginates_via_the_blocking_pager() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "models": [{"name": "models/gemini-2.5-flash"}],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let base_url = server.uri();
    let names = run_off_runtime(move || {
        let pager = test_client(base_url).models().list(None).unwrap();
        pager
            .page()
            .iter()
            .filter_map(|m| m.name.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(names, vec!["models/gemini-2.5-flash".to_owned()]);
    server.verify().await;
}

#[tokio::test]
async fn chat_send_message_and_send_message_stream_record_history() {
    // `blocking::Chat` (returned by `blocking::Chats::create`) is entirely
    // hand-written, not generated -- see `src/blocking/mod.rs`'s module
    // docs -- and `send_message_stream` in particular eagerly drains the
    // async `ChatStream` inside `block_on` rather than exposing a
    // lifetime-borrowing stream, so it gets its own coverage beyond the
    // `Models` unary/stream/pager sample above.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-flash:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "hi there"}]},
                "finishReason": "STOP"
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let sse_body = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"go\"}]},\"finishReason\":\"STOP\"}]}\n\n";
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_body)
                .insert_header("content-type", "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let base_url = server.uri();
    let (first_text, stream_texts, curated_len) = run_off_runtime(move || {
        let mut chat = test_client(base_url)
            .chats()
            .create("gemini-2.5-flash", None, None);
        let first = chat.send_message("hello", None).unwrap();
        let first_text = first.text().unwrap_or_default();

        let stream_texts: Vec<String> = chat
            .send_message_stream("again", None)
            .unwrap()
            .map(|item| item.unwrap().text().unwrap_or_default())
            .collect();

        (first_text, stream_texts, chat.get_history(true).len())
    });

    assert_eq!(first_text, "hi there");
    assert_eq!(stream_texts, vec!["go".to_owned()]);
    // Two exchanges (user + model each), fully valid: 4 curated turns.
    assert_eq!(curated_len, 4);
    server.verify().await;
}

/// Drives `f` (which calls into a `blocking::Client`) as if from inside a
/// `#[tokio::test]`-style async context: `f` runs inside
/// `outer_rt.block_on`, so `Handle::try_current()` sees `outer_rt` as
/// already running on this thread -- exactly the reentrant situation
/// `blocking::Runtime::block_on` must reject.
///
/// This builds `outer_rt` (and expects `client` to have been built)
/// *outside* the `block_on` call, and both are dropped only after it
/// returns: a `tokio::runtime::Runtime` may not be dropped from within an
/// async context (a plain worker thread disallows the blocking shutdown
/// that requires -- unrelated to, and stricter than, the
/// `BlockingInsideRuntime` guard this crate adds), so `blocking::Client`
/// itself (which owns one) must equally never be constructed *or* dropped
/// from inside a running runtime; only *calling into* an already-built one
/// is a supported (if rejected) scenario.
#[expect(
    clippy::unwrap_used,
    reason = "test helper: a broken tokio::runtime::Runtime::new() here is a test-infrastructure bug, not a runtime condition"
)]
fn call_from_inside_a_running_runtime<T>(f: impl Future<Output = T>) -> T {
    tokio::runtime::Runtime::new().unwrap().block_on(f)
}

#[test]
fn calling_a_blocking_method_from_inside_a_running_runtime_errors_instead_of_panicking() {
    let client = Client::builder().api_key("test-key").build().unwrap();
    let err = call_from_inside_a_running_runtime(async {
        client
            .models()
            .generate_content("gemini-2.5-flash", "hi", None)
    })
    .unwrap_err();
    assert!(matches!(err, Error::BlockingInsideRuntime));
}

#[test]
fn building_a_client_from_inside_a_running_runtime_errors_instead_of_panicking_later() {
    // `ClientBuilder::build` (and therefore `Client::new`) applies the
    // same `Handle::try_current()` guard as `Runtime::block_on`, but for
    // a different reason: it's not that anything here reenters
    // `block_on` -- it's that a `tokio::runtime::Runtime` built inside a
    // running runtime could never be *dropped* later without panicking
    // (see `Runtime::new`'s doc comment), so construction fails fast
    // instead of deferring that panic to some arbitrary later point.
    let err =
        call_from_inside_a_running_runtime(async { Client::builder().api_key("test-key").build() })
            .unwrap_err();
    assert!(matches!(err, Error::BlockingInsideRuntime));
}

#[test]
fn a_pager_kind_wrapper_also_errors_instead_of_panicking_when_nested() {
    // Same guard, exercised through a `kind = "pager"` generated wrapper
    // (not just the `kind = "unary"` one above): `Models::list` hits the
    // same `Runtime::block_on` -> `Handle::try_current()` check before it
    // ever constructs a `Pager` to return, so this also never reaches
    // `blocking::Pager::next_page`'s own (identically guarded) call.
    let client = Client::builder().api_key("test-key").build().unwrap();
    let err = call_from_inside_a_running_runtime(async { client.models().list(None) }).unwrap_err();
    assert!(matches!(err, Error::BlockingInsideRuntime));
}
