//! Ports of `client/test_retries.py`: retry policy resolution and retry
//! behavior for unary and streamed requests, at client and request level.
//!
//! Python mocks the httpx transport and reads tenacity's `retry_args`; the
//! crate's `RetryPolicy` is private, so the same behavior is checked on the
//! requests a mock server receives. Python's sync tests map to the
//! `blocking` client and its async tests to the async client; the aiohttp
//! variants (a Python-only transport) are recorded as exclusions, and the
//! Vertex AI client the Python tests construct is irrelevant to retries.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use gemini_genai::{
    Client, Error,
    types::{GenerateContentConfig, GetModelConfig, HttpOptions, HttpRetryOptions},
};
use serde_json::json;
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{method, path},
};

use crate::support::{MODEL, received};

/// Status codes retried by default.
const RETRIED_CODES: [u16; 6] = [408, 429, 500, 502, 503, 504];

const UNARY_PATH: &str = "/v1beta/models/gemini-2.5-flash";
const STREAM_PATH: &str = "/v1beta/models/gemini-2.5-flash:streamGenerateContent";

#[derive(Clone, Copy)]
enum Api {
    Async,
    #[cfg(feature = "blocking")]
    Blocking,
}

#[derive(Clone, Copy)]
enum Kind {
    Unary,
    Streamed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Client,
    Request,
}

/// Python's `_RETRY_OPTIONS`: two attempts, no real delay, 429 and 504 only.
fn retry_options() -> HttpRetryOptions {
    HttpRetryOptions {
        attempts: Some(2),
        initial_delay: Some(0.0),
        max_delay: Some(1.0),
        exp_base: Some(0.1),
        jitter: Some(0.1),
        http_status_codes: Some(vec![429, 504]),
    }
}

fn with_retry(retry: HttpRetryOptions) -> HttpOptions {
    HttpOptions {
        retry_options: Some(retry),
        ..Default::default()
    }
}

/// Answers the unary or streamed route with the next status in `statuses`;
/// the last status repeats, so a client that retries too often is caught by
/// the request count rather than by running out of responses.
async fn mount_statuses(server: &MockServer, kind: Kind, statuses: &[u16]) {
    let (verb, route) = match kind {
        Kind::Unary => ("GET", UNARY_PATH),
        Kind::Streamed => ("POST", STREAM_PATH),
    };
    for (index, status) in statuses.iter().enumerate() {
        let template = if *status == 200 {
            match kind {
                Kind::Unary => ResponseTemplate::new(200)
                    .set_body_json(json!({"name": format!("models/{MODEL}")})),
                Kind::Streamed => ResponseTemplate::new(200)
                    .set_body_string(format!(
                        "data: {}\n\n",
                        json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "hi"}]}}]})
                    ))
                    .insert_header("content-type", "text/event-stream"),
            }
        } else {
            ResponseTemplate::new(*status)
        };
        let mut mock = Mock::given(method(verb))
            .and(path(route))
            .respond_with(template);
        if index + 1 < statuses.len() {
            mock = mock.up_to_n_times(1);
        }
        mock.mount(server).await;
    }
}

async fn call_async(
    client_options: HttpOptions,
    per_request: Option<HttpOptions>,
    base_url: String,
    kind: Kind,
) -> Result<(), Error> {
    let client = Client::builder()
        .api_key("test-key")
        .http_options(HttpOptions {
            base_url: Some(base_url),
            ..client_options
        })
        .build()?;
    match kind {
        Kind::Unary => {
            let config = per_request.map(|options| GetModelConfig {
                http_options: Some(options),
            });
            client.models().get(MODEL, config).await.map(|_| ())
        }
        Kind::Streamed => {
            let config = per_request.map(|options| GenerateContentConfig {
                http_options: Some(options),
                ..Default::default()
            });
            let mut stream = client
                .models()
                .generate_content_stream(MODEL, "hi", config)
                .await?;
            while let Some(item) = stream.next().await {
                item?;
            }
            Ok(())
        }
    }
}

#[cfg(feature = "blocking")]
#[expect(
    clippy::unwrap_used,
    reason = "test helper: a panic on the worker thread is a test failure"
)]
fn call_blocking(
    client_options: HttpOptions,
    per_request: Option<HttpOptions>,
    base_url: String,
    kind: Kind,
) -> Result<(), Error> {
    // The blocking client must run outside any Tokio runtime context.
    std::thread::spawn(move || {
        let client = gemini_genai::blocking::Client::builder()
            .api_key("test-key")
            .http_options(HttpOptions {
                base_url: Some(base_url),
                ..client_options
            })
            .build()?;
        match kind {
            Kind::Unary => {
                let config = per_request.map(|options| GetModelConfig {
                    http_options: Some(options),
                });
                client.models().get(MODEL, config).map(|_| ())
            }
            Kind::Streamed => {
                let config = per_request.map(|options| GenerateContentConfig {
                    http_options: Some(options),
                    ..Default::default()
                });
                for item in client
                    .models()
                    .generate_content_stream(MODEL, "hi", config)?
                {
                    item?;
                }
                Ok(())
            }
        }
    })
    .join()
    .unwrap()
}

/// Runs one request against a server answering with `statuses`, with
/// `retry` configured at `level`; returns the outcome and how many requests
/// the server received.
async fn scenario(
    api: Api,
    kind: Kind,
    statuses: &[u16],
    retry: Option<HttpRetryOptions>,
    level: Level,
) -> (Result<(), Error>, usize) {
    let server = MockServer::start().await;
    mount_statuses(&server, kind, statuses).await;
    let (client_options, per_request) = match (retry, level) {
        (None, _) => (HttpOptions::default(), None),
        (Some(retry), Level::Client) => (with_retry(retry), None),
        (Some(retry), Level::Request) => (HttpOptions::default(), Some(with_retry(retry))),
    };
    let result = match api {
        Api::Async => call_async(client_options, per_request, server.uri(), kind).await,
        #[cfg(feature = "blocking")]
        Api::Blocking => call_blocking(client_options, per_request, server.uri(), kind),
    };
    let count = received(&server).await.len();
    (result, count)
}

/// The API error code a failed request ended with.
fn failed_with(result: Result<(), Error>) -> u16 {
    match result {
        Err(Error::Api(error)) => error.code,
        other => panic!("expected an API error, got {other:?}"),
    }
}

async fn assert_disabled_success(api: Api, kind: Kind) {
    let (result, count) = scenario(api, kind, &[200], None, Level::Client).await;
    result.unwrap();
    assert_eq!(count, 1);
}

async fn assert_disabled_failure(api: Api, kind: Kind) {
    let (result, count) = scenario(api, kind, &[429], None, Level::Client).await;
    assert_eq!(failed_with(result), 429);
    assert_eq!(count, 1, "without retry options a request is sent once");
}

async fn assert_enabled_success(api: Api, kind: Kind) {
    let (result, count) = scenario(api, kind, &[200], Some(retry_options()), Level::Client).await;
    result.unwrap();
    assert_eq!(count, 1);
}

async fn assert_retries_successfully(api: Api, kind: Kind, level: Level) {
    let (result, count) = scenario(api, kind, &[429, 200], Some(retry_options()), level).await;
    result.unwrap();
    assert_eq!(count, 2, "one retry after the 429");
}

async fn assert_retries_unsuccessfully(api: Api, kind: Kind, level: Level) {
    let (result, count) = scenario(api, kind, &[429, 504], Some(retry_options()), level).await;
    assert_eq!(failed_with(result), 504, "the last error is reported");
    assert_eq!(count, 2, "attempts = 2");
}

// -- Policy resolution ----------------------------------------------------

/// Sends one request per status in 400..=599 (the route encodes the status)
/// under `retry` and returns how many requests each status produced.
async fn attempts_by_status(retry: HttpRetryOptions) -> BTreeMap<u16, usize> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(|request: &Request| {
            let status = request
                .url
                .path()
                .rsplit_once("/models/m")
                .and_then(|(_, code)| code.parse::<u16>().ok())
                .unwrap_or(500);
            ResponseTemplate::new(status)
        })
        .mount(&server)
        .await;
    let client = Client::builder()
        .api_key("test-key")
        .http_options(HttpOptions {
            base_url: Some(server.uri()),
            retry_options: Some(retry),
            ..Default::default()
        })
        .build()
        .unwrap();

    let mut attempts = BTreeMap::new();
    for status in 400..=599_u16 {
        let before = received(&server).await.len();
        let result = client.models().get(&format!("m{status}"), None).await;
        assert!(result.is_err(), "status {status} must be an error");
        attempts.insert(status, received(&server).await.len() - before);
    }
    attempts
}

// upstream-test: client/test_retries.py::test_retry_args_disabled
#[tokio::test]
async fn test_retry_args_disabled() {
    // No options at all means a single attempt (`stop_after_attempt(1)`).
    for status in [429_u16, 503] {
        let (result, count) =
            scenario(Api::Async, Kind::Unary, &[status], None, Level::Client).await;
        assert_eq!(failed_with(result), status);
        assert_eq!(count, 1);
    }
}

// upstream-test: client/test_retries.py::test_retry_args_enabled_with_defaults
#[tokio::test]
async fn test_retry_args_enabled_with_defaults() {
    // Empty options mean "retry with the defaults" (5 attempts, the six
    // standard status codes); only the delays are shortened to keep the test
    // fast. Every other error status must not be retried.
    let attempts = attempts_by_status(HttpRetryOptions {
        initial_delay: Some(0.0),
        max_delay: Some(0.001),
        jitter: Some(0.0),
        ..Default::default()
    })
    .await;
    for (status, count) in attempts {
        let expected = if RETRIED_CODES.contains(&status) {
            5
        } else {
            1
        };
        assert_eq!(count, expected, "attempts for status {status}");
    }
}

// upstream-test: client/test_retries.py::test_retry_wait
#[tokio::test]
async fn test_retry_wait() {
    // Delays double each retry (initial * exp_base^n, here scaled from the
    // Python 1/2/4/8 s to 50/100/200/400 ms to keep the test fast).
    let timestamps = Arc::new(Mutex::new(Vec::<Instant>::new()));
    let recorded = Arc::clone(&timestamps);
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(move |_: &Request| {
            recorded
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Instant::now());
            ResponseTemplate::new(429)
        })
        .mount(&server)
        .await;
    let client = Client::builder()
        .api_key("test-key")
        .http_options(HttpOptions {
            base_url: Some(server.uri()),
            retry_options: Some(HttpRetryOptions {
                initial_delay: Some(0.05),
                max_delay: Some(60.0),
                exp_base: Some(2.0),
                jitter: Some(0.0),
                ..Default::default()
            }),
            ..Default::default()
        })
        .build()
        .unwrap();

    let result = client.models().get(MODEL, None).await;

    assert_eq!(failed_with(result.map(|_| ())), 429);
    let timestamps = timestamps.lock().unwrap_or_else(PoisonError::into_inner);
    assert_eq!(timestamps.len(), 5, "default attempts");
    for (index, minimum_ms) in [50_u64, 100, 200, 400].into_iter().enumerate() {
        assert!(
            timestamps[index + 1] - timestamps[index] >= Duration::from_millis(minimum_ms),
            "gap {index} was shorter than {minimum_ms} ms"
        );
    }
}

// upstream-test: client/test_retries.py::test_retry_args_enabled_with_custom_values_are_not_overridden
#[tokio::test]
async fn test_retry_args_enabled_with_custom_values_are_not_overridden() {
    let attempts = attempts_by_status(HttpRetryOptions {
        attempts: Some(3),
        initial_delay: Some(0.0),
        max_delay: Some(0.001),
        exp_base: Some(1.5),
        jitter: Some(0.0),
        http_status_codes: Some(vec![408, 429]),
    })
    .await;
    for (status, count) in attempts {
        let expected = if [408, 429].contains(&status) { 3 } else { 1 };
        assert_eq!(count, expected, "attempts for status {status}");
    }
}

// upstream-test: client/test_retries.py::test_retry_args_retries_httpx_transport_errors
#[tokio::test]
async fn test_retry_args_retries_httpx_transport_errors() {
    let retry = HttpRetryOptions {
        attempts: Some(3),
        initial_delay: Some(0.1),
        max_delay: Some(1.0),
        exp_base: Some(1.0),
        jitter: Some(0.0),
        ..Default::default()
    };

    // Timeouts are retried: the server stalls past the client timeout.
    let slow = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(500)))
        .mount(&slow)
        .await;
    let result = call_async(
        HttpOptions {
            timeout: Some(50),
            retry_options: Some(retry.clone()),
            ..Default::default()
        },
        None,
        slow.uri(),
        Kind::Unary,
    )
    .await;
    assert!(
        matches!(&result, Err(Error::Http(error)) if error.is_timeout()),
        "got {result:?}"
    );
    assert_eq!(received(&slow).await.len(), 3, "a timeout is retried");

    // Connection errors are retried: nothing listens on the port, so the
    // two backoff delays (2 x 100 ms) are the only way to take this long.
    let closed_port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let started = Instant::now();
    let result = call_async(
        with_retry(retry.clone()),
        None,
        format!("http://127.0.0.1:{closed_port}"),
        Kind::Unary,
    )
    .await;
    assert!(
        matches!(&result, Err(Error::Http(error)) if error.is_connect()),
        "got {result:?}"
    );
    assert!(
        started.elapsed() >= Duration::from_millis(200),
        "a connection error is retried with backoff"
    );

    // An invalid URL is not a transient failure: it fails immediately even
    // though a retry would wait a full second.
    let started = Instant::now();
    let result = call_async(
        with_retry(HttpRetryOptions {
            initial_delay: Some(1.0),
            ..retry
        }),
        None,
        "not a url".to_owned(),
        Kind::Unary,
    )
    .await;
    assert!(matches!(result, Err(Error::Http(_))), "got {result:?}");
    assert!(
        started.elapsed() < Duration::from_millis(900),
        "an invalid URL must not be retried"
    );
}

// -- Sync (blocking client) -------------------------------------------------

// upstream-test: client/test_retries.py::test_disabled_retries_successful_request_executes_once
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_disabled_retries_successful_request_executes_once() {
    assert_disabled_success(Api::Blocking, Kind::Unary).await;
}

// upstream-test: client/test_retries.py::test_disabled_retries_failed_request_executes_once
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_disabled_retries_failed_request_executes_once() {
    assert_disabled_failure(Api::Blocking, Kind::Unary).await;
}

// upstream-test: client/test_retries.py::test_retries_successful_request_executes_once
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_retries_successful_request_executes_once() {
    assert_enabled_success(Api::Blocking, Kind::Unary).await;
}

// upstream-test: client/test_retries.py::test_retries_failed_request_retries_successfully
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_retries_failed_request_retries_successfully() {
    assert_retries_successfully(Api::Blocking, Kind::Unary, Level::Client).await;
}

// upstream-test: client/test_retries.py::test_retries_failed_request_retries_successfully_at_request_level
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_retries_failed_request_retries_successfully_at_request_level() {
    assert_retries_successfully(Api::Blocking, Kind::Unary, Level::Request).await;
}

// upstream-test: client/test_retries.py::test_retries_failed_request_retries_unsuccessfully
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_retries_failed_request_retries_unsuccessfully() {
    assert_retries_unsuccessfully(Api::Blocking, Kind::Unary, Level::Client).await;
}

// upstream-test: client/test_retries.py::test_retries_failed_request_no_retries_unsuccessfully
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_retries_failed_request_no_retries_unsuccessfully() {
    // `attempts = 0` still sends the request once and reports its error.
    let options = HttpRetryOptions {
        attempts: Some(0),
        ..Default::default()
    };
    let (result, count) = scenario(
        Api::Blocking,
        Kind::Unary,
        &[429],
        Some(options),
        Level::Client,
    )
    .await;
    assert_eq!(failed_with(result), 429);
    assert_eq!(count, 1);
}

// upstream-test: client/test_retries.py::test_retries_failed_request_retries_unsuccessfully_at_request_level
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_retries_failed_request_retries_unsuccessfully_at_request_level() {
    assert_retries_unsuccessfully(Api::Blocking, Kind::Unary, Level::Request).await;
}

// -- Async ------------------------------------------------------------------

// upstream-test: client/test_retries.py::test_async_disabled_retries_successful_request_executes_once
#[tokio::test]
async fn test_async_disabled_retries_successful_request_executes_once() {
    assert_disabled_success(Api::Async, Kind::Unary).await;
}

// upstream-test: client/test_retries.py::test_async_disabled_retries_failed_request_executes_once
#[tokio::test]
async fn test_async_disabled_retries_failed_request_executes_once() {
    assert_disabled_failure(Api::Async, Kind::Unary).await;
}

// upstream-test: client/test_retries.py::test_async_retries_successful_request_executes_once
#[tokio::test]
async fn test_async_retries_successful_request_executes_once() {
    assert_enabled_success(Api::Async, Kind::Unary).await;
}

// upstream-test: client/test_retries.py::test_async_retries_failed_request_retries_successfully
#[tokio::test]
async fn test_async_retries_failed_request_retries_successfully() {
    assert_retries_successfully(Api::Async, Kind::Unary, Level::Client).await;
}

// upstream-test: client/test_retries.py::test_async_retries_failed_request_retries_successfully_at_request_level
#[tokio::test]
async fn test_async_retries_failed_request_retries_successfully_at_request_level() {
    assert_retries_successfully(Api::Async, Kind::Unary, Level::Request).await;
}

// upstream-test: client/test_retries.py::test_async_retries_failed_request_retries_unsuccessfully
#[tokio::test]
async fn test_async_retries_failed_request_retries_unsuccessfully() {
    assert_retries_unsuccessfully(Api::Async, Kind::Unary, Level::Client).await;
}

// upstream-test: client/test_retries.py::test_async_retries_failed_request_retries_unsuccessfully_at_request_level
#[tokio::test]
async fn test_async_retries_failed_request_retries_unsuccessfully_at_request_level() {
    assert_retries_unsuccessfully(Api::Async, Kind::Unary, Level::Request).await;
}

// -- Streamed (sync = blocking client) ----------------------------------------

// upstream-test: client/test_retries.py::test_retries_streamed_failed_request_retries_successfully
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_retries_streamed_failed_request_retries_successfully() {
    assert_retries_successfully(Api::Blocking, Kind::Streamed, Level::Client).await;
}

// upstream-test: client/test_retries.py::test_retries_streamed_failed_request_retries_successfully_at_request_level
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_retries_streamed_failed_request_retries_successfully_at_request_level() {
    assert_retries_successfully(Api::Blocking, Kind::Streamed, Level::Request).await;
}

// upstream-test: client/test_retries.py::test_retries_streamed_failed_request_retries_unsuccessfully
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_retries_streamed_failed_request_retries_unsuccessfully() {
    assert_retries_unsuccessfully(Api::Blocking, Kind::Streamed, Level::Client).await;
}

// upstream-test: client/test_retries.py::test_retries_streamed_failed_request_retries_unsuccessfully_at_request_level
#[cfg(feature = "blocking")]
#[tokio::test]
async fn test_retries_streamed_failed_request_retries_unsuccessfully_at_request_level() {
    assert_retries_unsuccessfully(Api::Blocking, Kind::Streamed, Level::Request).await;
}

// upstream-test: client/test_retries.py::test_async_retries_streamed_failed_request_retries_successfully
#[tokio::test]
async fn test_async_retries_streamed_failed_request_retries_successfully() {
    assert_retries_successfully(Api::Async, Kind::Streamed, Level::Client).await;
}

// upstream-test: client/test_retries.py::test_async_retries_streamed_failed_request_retries_successfully_at_request_level
#[tokio::test]
async fn test_async_retries_streamed_failed_request_retries_successfully_at_request_level() {
    assert_retries_successfully(Api::Async, Kind::Streamed, Level::Request).await;
}

// upstream-test: client/test_retries.py::test_async_retries_streamed_failed_request_retries_unsuccessfully
#[tokio::test]
async fn test_async_retries_streamed_failed_request_retries_unsuccessfully() {
    assert_retries_unsuccessfully(Api::Async, Kind::Streamed, Level::Client).await;
}

// upstream-test: client/test_retries.py::test_async_retries_streamed_failed_request_retries_unsuccessfully_at_request_level
#[tokio::test]
async fn test_async_retries_streamed_failed_request_retries_unsuccessfully_at_request_level() {
    assert_retries_unsuccessfully(Api::Async, Kind::Streamed, Level::Request).await;
}
