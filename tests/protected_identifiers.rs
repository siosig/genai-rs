//! Pins the identifiers that belong to the upstream API, not to this crate.
//!
//! This crate's own name contains `google`, and so do a great many things it
//! must never rename: `GOOGLE_API_KEY`, `x-goog-api-key`,
//! `generativelanguage.googleapis.com`, the `google.ai.generativelanguage.*`
//! WebSocket service path, and wire fields like `googleSearch`. Worse, the
//! crate's former package name `google-genai-rs` was a strict prefix extension of
//! `google-genai`, the upstream *package* name that appears 87 times in
//! generated headers, the codegen pin and the attribution.
//!
//! A search-and-replace that misses that distinction does not fail loudly. It
//! produces a crate that compiles, passes most tests, and silently sends the
//! wrong JSON keys or reads the wrong environment variables. These tests are
//! the tripwire.
//!
//! Deliberately written against **observable behaviour** rather than the
//! constants themselves: asserting `GOOGLE_API_KEY_VAR == "GOOGLE_API_KEY"`
//! passes just fine if a rename rewrote the constant and its value together.
//! Setting the real environment variable and checking that a request carries
//! the key does not.

#![expect(
    unsafe_code,
    reason = "std::env::set_var/remove_var are unsafe in a multi-threaded process; these tests serialize on ENV_LOCK"
)]
#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed mock or malformed captured request here is a test-setup bug, not a runtime condition"
)]
#![expect(
    clippy::large_futures,
    reason = "Live::connect's future is inherently large (WebSocket handshake + setup-message state held across await points); harmless in test code that isn't stack-constrained"
)]

mod common;

use std::sync::{Mutex, PoisonError};

use common::ws_server::start_mock_ws_server;
use gemini_genai::{
    Client, Error,
    types::{GenerateContentConfig, GoogleSearch, HttpOptions, Tool},
};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

// --- the identifiers under guard ------------------------------------------
//
// Spelled out as literals on purpose. If a rename rewrites both a constant in
// `src/` and its use site, a test that referenced the constant would follow it
// silently; a literal cannot.

/// Primary API-key environment variable, matching the upstream Python SDK.
const GOOGLE_API_KEY: &str = "GOOGLE_API_KEY";
/// Fallback API-key environment variable.
const GEMINI_API_KEY: &str = "GEMINI_API_KEY";
/// Opt-in to the Vertex AI backend, which this port rejects up front.
const USE_VERTEXAI: &str = "GOOGLE_GENAI_USE_VERTEXAI";
/// Overrides the API base URL.
const BASE_URL: &str = "GOOGLE_GEMINI_BASE_URL";
/// Header the Gemini Developer API authenticates with.
const API_KEY_HEADER: &str = "x-goog-api-key";
/// Header identifying the calling SDK.
const API_CLIENT_HEADER: &str = "x-goog-api-client";
/// gRPC service path the Live API's WebSocket endpoint is built from.
const LIVE_SERVICE_PATH: &str = "google.ai.generativelanguage";

/// Environment variables are process-global; anything touching them has to
/// run one at a time. Mirrors the `ENV_LOCK` in `src/client.rs`.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Runs `build` with `vars` set in the environment, holding [`ENV_LOCK`] for
/// exactly that long.
///
/// Environment variables are process-global, so the window in which they are
/// set has to be serialised against every other test that touches them. The
/// window is kept synchronous on purpose: `Client::new()` reads the
/// environment eagerly, so the lock never has to be held across an `.await`
/// (which would risk deadlocking the runtime and is why
/// `clippy::await_holding_lock` exists). By the time the caller starts making
/// requests, the client already holds everything it read.
fn with_env<T>(vars: &[(&str, &str)], build: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    clear_env();
    for (var, value) in vars {
        unsafe { std::env::set_var(var, value) };
    }
    let result = build();
    clear_env();
    result
}

/// Clears every variable these tests set, so a test never inherits state from
/// the one before it -- or from the developer's shell, which very plausibly
/// has `GEMINI_API_KEY` exported.
fn clear_env() {
    for var in [GOOGLE_API_KEY, GEMINI_API_KEY, USE_VERTEXAI, BASE_URL] {
        unsafe { std::env::remove_var(var) };
    }
}

fn ok_response() -> serde_json::Value {
    serde_json::json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{"text": "ok"}]},
            "finishReason": "STOP"
        }]
    })
}

async fn mock_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response()))
        .mount(&server)
        .await;
    server
}

/// Drives one `generate_content` call against `server` and returns the
/// headers of the request it received.
async fn captured_headers(server: &MockServer, client: &Client) -> Vec<(String, String)> {
    client
        .models()
        .generate_content("gemini-2.5-flash", "hi", None)
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1, "expected exactly one captured request");
    requests[0]
        .headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

// --- P1..P3: the API-key environment variables ----------------------------

#[tokio::test]
async fn google_api_key_env_var_supplies_the_key() {
    // P1. If `GOOGLE_API_KEY` were renamed, `Client::new()` would fall through
    // to "no API key" and every downstream user's setup would break.
    let server = mock_server().await;
    let client = with_env(
        &[
            (GOOGLE_API_KEY, "from-google-var"),
            (BASE_URL, &server.uri()),
        ],
        || Client::new().unwrap(),
    );
    let headers = captured_headers(&server, &client).await;

    assert_eq!(
        header_value(&headers, API_KEY_HEADER),
        Some("from-google-var"),
        "{GOOGLE_API_KEY} must still be the primary API-key variable"
    );
}

#[tokio::test]
async fn gemini_api_key_env_var_is_the_fallback() {
    // P2. The upstream SDK accepts either name; dropping the fallback would
    // silently break anyone who only exports `GEMINI_API_KEY`.
    let server = mock_server().await;
    let client = with_env(
        &[
            (GEMINI_API_KEY, "from-gemini-var"),
            (BASE_URL, &server.uri()),
        ],
        || Client::new().unwrap(),
    );
    let headers = captured_headers(&server, &client).await;

    assert_eq!(
        header_value(&headers, API_KEY_HEADER),
        Some("from-gemini-var"),
        "{GEMINI_API_KEY} must still work when {GOOGLE_API_KEY} is unset"
    );
}

#[tokio::test]
async fn google_api_key_wins_when_both_are_set() {
    // P3. The precedence itself is upstream-compatible behaviour, and it is
    // only observable if both variable names resolve correctly.
    let server = mock_server().await;
    let client = with_env(
        &[
            (GOOGLE_API_KEY, "google-wins"),
            (GEMINI_API_KEY, "gemini-loses"),
            (BASE_URL, &server.uri()),
        ],
        || Client::new().unwrap(),
    );
    let headers = captured_headers(&server, &client).await;

    assert_eq!(
        header_value(&headers, API_KEY_HEADER),
        Some("google-wins"),
        "{GOOGLE_API_KEY} must take precedence over {GEMINI_API_KEY}"
    );
}

// --- P4: the Vertex AI opt-out --------------------------------------------

#[test]
fn use_vertexai_env_var_still_fails_fast() {
    // P4. `GOOGLE_GENAI_USE_VERTEXAI` contains `google_genai` case-insensitively,
    // so a careless rename swallows it -- and the failure mode is the worst
    // kind: the client silently builds a Gemini Developer API client for
    // someone who asked for Vertex AI.
    let result = with_env(
        &[(GOOGLE_API_KEY, "irrelevant"), (USE_VERTEXAI, "1")],
        Client::new,
    );

    assert!(
        matches!(result, Err(Error::UnsupportedBackend(_))),
        "{USE_VERTEXAI}=1 must still be rejected with UnsupportedBackend, got {result:?}"
    );
}

// --- P5: the base-URL override --------------------------------------------

#[tokio::test]
async fn base_url_env_var_redirects_requests() {
    // P5. Implicitly exercised by every test above, but asserted on its own so
    // a failure names the right variable.
    let server = mock_server().await;
    let client = with_env(&[(GOOGLE_API_KEY, "k"), (BASE_URL, &server.uri())], || {
        Client::new().unwrap()
    });
    client
        .models()
        .generate_content("gemini-2.5-flash", "hi", None)
        .await
        .unwrap();

    assert_eq!(
        server.received_requests().await.unwrap().len(),
        1,
        "{BASE_URL} must still redirect the API base URL"
    );
}

// --- P6, P7: the HTTP header names ----------------------------------------

#[tokio::test]
async fn requests_carry_the_upstream_header_names() {
    // P6/P7. `x-goog-` is a Google-wide protocol prefix, not this crate's
    // branding. Renaming it produces 401s at runtime and nothing at build time.
    let server = mock_server().await;
    let client = with_env(&[(GOOGLE_API_KEY, "k"), (BASE_URL, &server.uri())], || {
        Client::new().unwrap()
    });
    let headers = captured_headers(&server, &client).await;

    assert!(
        header_value(&headers, API_KEY_HEADER).is_some(),
        "requests must still authenticate with the `{API_KEY_HEADER}` header"
    );
    assert!(
        header_value(&headers, API_CLIENT_HEADER).is_some(),
        "requests must still identify the SDK via `{API_CLIENT_HEADER}`"
    );
}

// --- P9: the Live API service path ----------------------------------------

#[tokio::test]
async fn live_websocket_url_keeps_the_upstream_service_path() {
    // P9. `google.ai.generativelanguage.<version>.GenerativeService.<method>`
    // is the gRPC service name the server routes on. A rename here fails the
    // handshake, which only shows up against the real API.
    let (base_url, join) = start_mock_ws_server(|mut ws, req| async move {
        assert!(
            req.uri.contains(LIVE_SERVICE_PATH),
            "Live API URL must keep the `{LIVE_SERVICE_PATH}` service path, got: {}",
            req.uri
        );
        // Let the client finish connecting, then close.
        let _ = ws.close(None).await;
    })
    .await;

    let client = Client::builder()
        .api_key("test-key")
        .http_options(HttpOptions {
            base_url: Some(base_url),
            ..Default::default()
        })
        .build()
        .unwrap();

    // The connection is expected to end immediately; only the handshake URL
    // matters here, and the assertion above runs inside the server task.
    let _ = client
        .live()
        .connect("gemini-2.0-flash-live-001", None)
        .await;
    join.await.unwrap();
}

// --- P10: the upstream wire field names -----------------------------------

#[tokio::test]
async fn google_prefixed_wire_fields_survive_serialisation() {
    // P10. `googleSearch` / `googleMaps` are field names in the upstream API
    // schema. Renaming them makes the API reject the request body -- and no
    // unit test on the Rust types would notice, because the Rust field name
    // and the wire name are converted independently.
    let server = mock_server().await;
    let client = with_env(&[(GOOGLE_API_KEY, "k"), (BASE_URL, &server.uri())], || {
        Client::new().unwrap()
    });
    let config = GenerateContentConfig {
        tools: Some(vec![Tool {
            google_search: Some(GoogleSearch::default()),
            ..Default::default()
        }]),
        ..Default::default()
    };
    client
        .models()
        .generate_content("gemini-2.5-flash", "hi", Some(config))
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    let body = String::from_utf8_lossy(&requests[0].body).into_owned();

    assert!(
        body.contains("\"googleSearch\""),
        "the request body must still use the upstream `googleSearch` wire \
         field, got: {body}"
    );
}

// --- P11, P12: the upstream references the codegen depends on -------------

#[test]
fn codegen_still_pins_the_upstream_package_by_its_real_name() {
    // P11. `google-genai` is the upstream *package* name and a strict prefix
    // of this crate's old name. Rewriting it here breaks `pip install` and
    // therefore every regeneration.
    let requirements = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tools/codegen/requirements.txt"
    ))
    .unwrap();
    assert!(
        requirements
            .lines()
            .any(|line| line.trim_start().starts_with("google-genai==")),
        "tools/codegen/requirements.txt must pin the upstream package as \
         `google-genai==<version>`"
    );
}

#[test]
fn generated_headers_still_name_the_upstream_package() {
    // P12. The `from google-genai <version>` marker is what ties a generated
    // file to its provenance -- and it is exactly the substring a careless
    // rename of `google-genai-rs` would corrupt.
    let generated = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/types/generated/structs.rs"
    ))
    .unwrap();
    assert!(
        generated.contains("from google-genai "),
        "generated files must still name the upstream package they were \
         generated from"
    );
}
