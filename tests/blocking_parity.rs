//! Integration tests for `google_genai::blocking` (`feature = "blocking"`):
//! `Client` construction, one `wiremock`-backed call of each generated
//! wrapper shape (unary/stream/pager -- see `tools/codegen/methods.toml`'s
//! `kind` field), and the `Error::BlockingInsideRuntime` guard.
//!
//! Supersedes the never-actually-implemented blocking-wrapper portions of
//! tasks T037/T052/T061/T067/T072/T081 (each marked done in a prior
//! session without a `blocking` counterpart ever landing).
//!
//! Two layers:
//!
//! - **Behavioural** (the first eight tests): one `wiremock`-backed call
//!   per wrapper shape rather than one test per module, since
//!   `src/blocking/mod.rs`'s doc comments explain that every module's
//!   wrapper shares the same `Runtime::block_on` plumbing --- one
//!   exercised example per `kind` is sufficient signal that the
//!   generator/hand-written split works at runtime. Sampling cannot,
//!   however, notice a method that is simply *absent*, which is what the
//!   next layer is for.
//! - **Ledger-driven symmetry** (spec task T088, SC-008): the last two
//!   tests read `tools/codegen/methods.toml` --- the same inventory
//!   `gen_blocking.py` generates from --- and assert in both directions
//!   that every public async method has a synchronous counterpart. Adding
//!   an async method without a blocking one turns one of them red.

#![cfg(feature = "blocking")]

mod common;

use std::collections::HashSet;
use std::future::Future;

use common::blocking_test_client;
use google_genai::Error;
use google_genai::blocking::Client;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
        let response = blocking_test_client(base_url)
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
        let stream = blocking_test_client(base_url)
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
        let pager = blocking_test_client(base_url).models().list(None).unwrap();
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
        let mut chat =
            blocking_test_client(base_url)
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

// ---------------------------------------------------------------------------
// SC-008: async/blocking symmetry, driven by the method ledger
// ---------------------------------------------------------------------------

/// The hand-maintained inventory of every ported public method --- the same
/// file `tools/codegen/gen_blocking.py` generates `src/blocking/generated.rs`
/// from, and `gen_parity.py` generates `docs/parity.md` from.
const METHODS_TOML: &str = include_str!("../tools/codegen/methods.toml");

/// The generated blocking wrappers.
const BLOCKING_GENERATED: &str = include_str!("../src/blocking/generated.rs");

/// The hand-written blocking wrappers (`Chats::create`/`Chat`,
/// `Operations::get<T>`, `FileSearchStores::documents`).
const BLOCKING_MANUAL: &str = include_str!("../src/blocking/mod.rs");

/// Every async module whose handles carry ledger entries, so the reverse
/// check below can prove nothing was added to the async API without also
/// being written down in `methods.toml` (and therefore without getting a
/// blocking wrapper). The Live modules are excluded for the same reason
/// they are excluded above.
const ASYNC_SOURCES: &[(&str, &str)] = &[
    ("models", include_str!("../src/models.rs")),
    ("chats", include_str!("../src/chats.rs")),
    ("files", include_str!("../src/files.rs")),
    ("caches", include_str!("../src/caches.rs")),
    ("tunings", include_str!("../src/tunings.rs")),
    ("batches", include_str!("../src/batches.rs")),
    ("operations", include_str!("../src/operations.rs")),
    (
        "file_search_stores",
        include_str!("../src/file_search_stores.rs"),
    ),
    ("documents", include_str!("../src/documents.rs")),
    ("auth_tokens", include_str!("../src/auth_tokens.rs")),
];

/// One `[[method]]` row of `methods.toml`. Only the fields this check
/// needs are modelled; `serde` ignores the rest.
#[derive(serde::Deserialize)]
struct LedgerMethod {
    module: String,
    owner: String,
    name: String,
    kind: String,
    #[serde(default)]
    visibility: Option<String>,
}

/// `methods.toml` as a whole.
#[derive(serde::Deserialize)]
struct Ledger {
    #[serde(default)]
    method: Vec<LedgerMethod>,
}

impl LedgerMethod {
    /// The blocking wrapper struct this method's counterpart lives on.
    /// `src/blocking/` mirrors the async type names exactly, so this is
    /// just the last segment of `owner` (`crate::chats::Chat` -> `Chat`).
    fn blocking_type(&self) -> &str {
        last_path_segment(&self.owner)
    }

    /// Whether a blocking counterpart is expected at all. Only the Live
    /// surface is exempt: Python's own Live API is `client.aio.live`
    /// (async) only, so there is nothing to mirror --- see
    /// `specs/001-port-genai-rust/contracts/public-api.md` and
    /// `methods.toml`'s `live` section header. `visibility = "private"`
    /// entries exist purely so `gen_parity.py` can account for a
    /// parity-matrix row and are not public API.
    fn needs_blocking_counterpart(&self) -> bool {
        self.module != "live"
            && self.module != "live_music"
            && self.visibility.as_deref() != Some("private")
    }
}

/// `crate::models::Models` -> `Models`.
fn last_path_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

/// Collects `(type, method)` for every inherent `fn` in `source` whose
/// declaration starts with `prefix` (`"pub fn "` or `"pub async fn "`).
///
/// Deliberately a line-oriented scan rather than a real parse: this crate
/// has no syn/proc-macro dev-dependency, every `impl` block in the files
/// it reads opens at column 0 and closes with a bare `}` at column 0, and
/// a scan that silently matched nothing is caught by the population
/// assertions in the tests below.
fn inherent_fns(source: &str, prefix: &str) -> HashSet<(String, String)> {
    let mut found = HashSet::new();
    let mut current: Option<String> = None;
    for line in source.lines() {
        if line.starts_with("impl") {
            // Trait impls (`impl Trait for Type`) never carry the
            // inherent `pub fn`s this check is about.
            current = if line.contains(" for ") {
                None
            } else {
                let mut rest = line.trim_start_matches("impl").trim_start();
                // Skip an `impl<T>`-style generic parameter list.
                if rest.starts_with('<') {
                    rest = rest
                        .split_once('>')
                        .map_or("", |(_, tail)| tail)
                        .trim_start();
                }
                let name = rest
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('{')
                    .rsplit("::")
                    .next()
                    .unwrap_or("")
                    .split('<')
                    .next()
                    .unwrap_or("");
                (!name.is_empty()).then(|| name.to_owned())
            };
        } else if line == "}" {
            current = None;
        }
        if let (Some(ty), Some(rest)) = (current.as_ref(), line.trim_start().strip_prefix(prefix)) {
            let method: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !method.is_empty() {
                found.insert((ty.clone(), method));
            }
        }
    }
    found
}

/// SC-008, forward direction: every ledger method that should have a
/// synchronous counterpart has one, whether the generator emitted it
/// (`kind = "unary"/"stream"/"pager"/"upload"`) or it was hand-written in
/// `src/blocking/mod.rs` (`kind = "session"/"manual"`).
///
/// This is the ledger-driven check spec task T088 asked for: the eight
/// behavioural tests above prove the `Runtime::block_on` plumbing works
/// for one example of each wrapper shape, but they can't notice a
/// *missing* method. This can.
#[test]
fn every_ledger_method_has_a_blocking_counterpart() {
    let ledger: Ledger = toml::from_str(METHODS_TOML).expect("methods.toml is not valid TOML");

    let mut blocking = inherent_fns(BLOCKING_GENERATED, "pub fn ");
    blocking.extend(inherent_fns(BLOCKING_MANUAL, "pub fn "));
    assert!(
        blocking.len() > 40,
        "the src/blocking scan found only {} methods, so it is broken rather than the API",
        blocking.len()
    );

    let mut checked = 0_usize;
    let mut missing = Vec::new();
    for method in &ledger.method {
        if !method.needs_blocking_counterpart() {
            continue;
        }
        checked += 1;
        let expected = (method.blocking_type().to_owned(), method.name.clone());
        if !blocking.contains(&expected) {
            missing.push(format!(
                "{}::{} (module = {}, kind = {})",
                expected.0, expected.1, method.module, method.kind
            ));
        }
    }

    assert!(
        checked > 40,
        "only {checked} ledger methods were checked, so this test is not actually covering the API"
    );
    assert!(
        missing.is_empty(),
        "{} async method(s) in tools/codegen/methods.toml have no blocking counterpart in \
         src/blocking/ (SC-008). Either add the wrapper (re-run \
         `python tools/codegen/generate.py --only blocking` for a generated `kind`, or \
         hand-write it in src/blocking/mod.rs) or, if it genuinely cannot have one, document \
         why in methods.toml alongside the Live surface:\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
}

/// SC-008, reverse direction: no `pub async fn` may exist on a ported
/// handle without a `methods.toml` entry --- otherwise the forward check
/// above would happily pass over a brand-new async method that never got
/// a blocking wrapper.
#[test]
fn every_async_public_method_is_listed_in_the_ledger() {
    let ledger: Ledger = toml::from_str(METHODS_TOML).expect("methods.toml is not valid TOML");
    let listed: HashSet<(String, String)> = ledger
        .method
        .iter()
        .map(|m| (m.blocking_type().to_owned(), m.name.clone()))
        .collect();

    let mut found = 0_usize;
    let mut unlisted = Vec::new();
    for (module, source) in ASYNC_SOURCES {
        for (ty, name) in inherent_fns(source, "pub async fn ") {
            found += 1;
            if !listed.contains(&(ty.clone(), name.clone())) {
                unlisted.push(format!("{ty}::{name} (src/{module}.rs)"));
            }
        }
    }

    assert!(
        found > 40,
        "only {found} async methods were discovered, so this test is not actually covering the API"
    );
    unlisted.sort();
    assert!(
        unlisted.is_empty(),
        "{} public async method(s) are missing from tools/codegen/methods.toml, so neither \
         the blocking generator nor the SC-008 symmetry check above can see them:\n  {}",
        unlisted.len(),
        unlisted.join("\n  ")
    );
}
