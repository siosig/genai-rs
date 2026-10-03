//! Replays the oracle corpus generated from upstream's table-driven tests.
//!
//! `tools/codegen/gen_upstream_cases.py` runs every in-scope upstream table
//! case against the real Python SDK with a capturing transport and writes the
//! requests it produced to `tests/fixtures/upstream/**.json`. Each case here
//! calls the same method through this crate against a mock server and
//! compares the requests, so a divergence fails with the upstream origin
//! (`<test file>::<case name>`) in the message.

mod dispatch;

#[path = "../common/mod.rs"]
mod common;

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde_json::{Value, json};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};

use common::test_client;
use dispatch::Outcome;

const FIXTURES_DIR: &str = "tests/fixtures/upstream";
const CASES_KEY: &str = "cases";
const STREAM_SUFFIX: &str = "_stream";
const SSE_BODY: &str = "data: {}\n\n";
const SSE_CONTENT_TYPE: &str = "text/event-stream";

/// One request the Python SDK sent for a case.
#[derive(Debug, PartialEq)]
struct ExpectedRequest {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    body: Value,
}

struct Case {
    source: String,
    test_method: String,
    name: String,
    parameters: Value,
    response_body: Value,
    requests: Vec<ExpectedRequest>,
    error_contains: Option<String>,
}

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURES_DIR)
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();
    paths
        .into_iter()
        .flat_map(|path| {
            if path.is_dir() {
                json_files(&path)
            } else if path.extension().is_some_and(|ext| ext == "json") {
                vec![path]
            } else {
                Vec::new()
            }
        })
        .collect()
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

fn expected_request(value: &Value) -> ExpectedRequest {
    ExpectedRequest {
        method: text(value, "method"),
        path: text(value, "path"),
        query: pairs_from_array(value),
        body: value["body"].clone(),
    }
}

fn pairs_from_array(value: &Value) -> Vec<(String, String)> {
    value["query"]
        .as_array()
        .map(|pairs| {
            pairs
                .iter()
                .map(|pair| {
                    (
                        pair[0].as_str().unwrap_or_default().to_owned(),
                        pair[1].as_str().unwrap_or_default().to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn load_cases(dir: &str) -> Vec<Case> {
    json_files(&fixtures_root().join(dir))
        .iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .filter_map(|raw| serde_json::from_str::<Value>(&raw).ok())
        .filter(|file| file.get(CASES_KEY).is_some())
        .flat_map(|file| {
            let source = text(&file, "source");
            let test_method = text(&file, "test_method");
            file[CASES_KEY]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|case| Case {
                    source: source.clone(),
                    test_method: test_method.clone(),
                    name: text(&case, "name"),
                    parameters: case["parameters"].clone(),
                    response_body: case["response_body"].clone(),
                    requests: case["expect"]["requests"]
                        .as_array()
                        .map(|requests| requests.iter().map(expected_request).collect())
                        .unwrap_or_default(),
                    error_contains: case["expect"]["error_contains"].as_str().map(str::to_owned),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn mock_response(case: &Case) -> ResponseTemplate {
    if case.test_method.ends_with(STREAM_SUFFIX) {
        ResponseTemplate::new(200)
            .insert_header("content-type", SSE_CONTENT_TYPE)
            .set_body_string(SSE_BODY)
    } else {
        ResponseTemplate::new(200).set_body_json(&case.response_body)
    }
}

async fn received(server: &MockServer) -> Vec<ExpectedRequest> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .map(|request| {
            let mut query: Vec<(String, String)> = request
                .url
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            query.sort();
            ExpectedRequest {
                method: request.method.to_string(),
                path: request.url.path().to_owned(),
                query,
                body: serde_json::from_slice(&request.body).unwrap_or(Value::Null),
            }
        })
        .collect()
}

async fn run_case(case: &Case) -> Result<(), String> {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(mock_response(case))
        .mount(&server)
        .await;
    let client = test_client(server.uri());

    let outcome = dispatch::call(&client, &case.test_method, case.parameters.clone()).await;
    match (&case.error_contains, outcome) {
        (_, Outcome::Harness(message)) => return Err(message),
        (Some(expected), Outcome::Failed(message)) => {
            if !message.contains(expected.as_str()) {
                return Err(format!(
                    "error `{message}` does not contain expected `{expected}`"
                ));
            }
            return Ok(());
        }
        (Some(expected), Outcome::Succeeded) => {
            return Err(format!("expected an error containing `{expected}`"));
        }
        (None, Outcome::Failed(message)) => return Err(format!("unexpected error: {message}")),
        (None, Outcome::Succeeded) => {}
    }

    let actual = received(&server).await;
    if actual != case.requests {
        return Err(format!(
            "request mismatch\n  python: {}\n  rust:   {}",
            describe(&case.requests),
            describe(&actual)
        ));
    }
    Ok(())
}

fn describe(requests: &[ExpectedRequest]) -> String {
    let rendered: Vec<Value> = requests
        .iter()
        .map(|r| json!({"method": r.method, "path": r.path, "query": r.query, "body": r.body}))
        .collect();
    Value::Array(rendered).to_string()
}

async fn run_dir(dir: &str) {
    let cases = load_cases(dir);
    assert!(
        !cases.is_empty(),
        "no corpus cases under {FIXTURES_DIR}/{dir}"
    );
    let mut failures = Vec::new();
    for case in &cases {
        if let Err(message) = run_case(case).await {
            failures.push(format!("{}::{}: {message}", case.source, case.name));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} corpus cases failed:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

#[tokio::test]
async fn corpus_models() {
    run_dir("models").await;
}

#[tokio::test]
async fn corpus_batches() {
    run_dir("batches").await;
}

#[tokio::test]
async fn corpus_caches() {
    run_dir("caches").await;
}

#[tokio::test]
async fn corpus_files() {
    run_dir("files").await;
}

#[tokio::test]
async fn corpus_file_search_stores() {
    run_dir("file_search_stores").await;
}

#[tokio::test]
async fn corpus_documents() {
    run_dir("documents").await;
}

#[tokio::test]
async fn corpus_tunings() {
    run_dir("tunings").await;
}

#[tokio::test]
async fn corpus_tokens() {
    run_dir("tokens").await;
}

#[tokio::test]
async fn corpus_shared() {
    run_dir("shared").await;
}

#[test]
fn every_corpus_method_has_a_dispatch_arm() {
    let mut methods: Vec<String> = [
        "models",
        "batches",
        "caches",
        "files",
        "file_search_stores",
        "documents",
        "tunings",
        "tokens",
        "shared",
    ]
    .iter()
    .flat_map(|dir| load_cases(dir))
    .map(|case| case.test_method)
    .collect();
    methods.sort();
    methods.dedup();
    let missing: Vec<&String> = methods
        .iter()
        .filter(|method| !dispatch::HANDLED_METHODS.contains(&method.as_str()))
        .collect();
    assert!(missing.is_empty(), "no dispatch arm for: {missing:?}");
}
