//! Ported from `google/genai/tests/interactions/test_integration.py`.
//!
//! The Python timeout tests assert that the parent client's `timeout` reaches the
//! generated client constructor (`timeout_ms=5000`) or its `httpx.Timeout`. The
//! observable behavior is ported instead: a client timeout aborts a slow
//! interactions call, and an unset timeout does not.

use std::{collections::HashMap, time::Duration};

use gemini_genai::{
    Client, Error,
    interactions::{
        AllowlistEntry, CreateInteractionRequestBody, CreateModelInteraction, InteractionsInput,
        Model, Transform,
    },
    types::HttpOptions,
};
use serde_json::{Value, json};
use wiremock::ResponseTemplate;

use crate::recording::{completed, create_hello, server_answering};

/// Longer than the 5 s timeout the default-timeout upstream tests rule out.
const SLOWER_THAN_FIVE_SECONDS: Duration = Duration::from_millis(5_500);

fn client_with_timeout(base_url: String, timeout_ms: Option<i64>) -> Client {
    Client::builder()
        .api_key("placeholder")
        .http_options(HttpOptions {
            base_url: Some(base_url),
            timeout: timeout_ms,
            ..Default::default()
        })
        .build()
        .unwrap()
}

async fn check_client_timeout() {
    let server = server_answering(
        ResponseTemplate::new(200)
            .set_body_json(completed())
            .set_delay(Duration::from_secs(2)),
    )
    .await;
    let client = client_with_timeout(server.uri(), Some(100));

    let err = create_hello(&client).await.unwrap_err();

    assert!(
        matches!(&err, Error::Http(http) if http.is_timeout()),
        "expected a timeout, got {err:?}"
    );
}

async fn check_default_timeout() {
    let server = server_answering(
        ResponseTemplate::new(200)
            .set_body_json(completed())
            .set_delay(SLOWER_THAN_FIVE_SECONDS),
    )
    .await;
    let client = client_with_timeout(server.uri(), None);

    // Default on the Developer API is no timeout (httpx.Timeout(None)), not 5 s.
    create_hello(&client).await.unwrap();
}

// upstream-test: interactions/test_integration.py::test_client_timeout
#[tokio::test]
async fn test_client_timeout() {
    check_client_timeout().await;
}

// upstream-test: interactions/test_integration.py::test_async_client_timeout
#[tokio::test]
async fn test_async_client_timeout() {
    check_client_timeout().await;
}

// upstream-test: interactions/test_integration.py::test_client_genai_default_timeout
#[tokio::test]
async fn test_client_genai_default_timeout() {
    check_default_timeout().await;
}

// upstream-test: interactions/test_integration.py::test_async_client_genai_default_timeout
#[tokio::test]
async fn test_async_client_genai_default_timeout() {
    check_default_timeout().await;
}

fn unrecognized_model_body() -> CreateModelInteraction {
    CreateModelInteraction {
        model: Some(Model::from("gemini-3.5-flash".to_owned())),
        input: Some(InteractionsInput::Text("hello".to_owned())),
        ..Default::default()
    }
}

// upstream-test: interactions/test_integration.py::test_unrecognized_model_serialization
#[test]
fn test_unrecognized_model_serialization() {
    // A model name the SDK does not know serializes as the plain string.
    let dumped = serde_json::to_value(unrecognized_model_body()).unwrap();

    assert_eq!(dumped["model"], "gemini-3.5-flash");
}

// upstream-test: interactions/test_integration.py::test_unrecognized_model_request_serialization
#[test]
fn test_unrecognized_model_request_serialization() {
    let body = CreateInteractionRequestBody::CreateModelInteraction(unrecognized_model_body());

    let dumped = serde_json::to_value(&body).unwrap();

    assert_eq!(dumped["model"], "gemini-3.5-flash");
}

// upstream-test: interactions/test_integration.py::test_allowlist_entry_with_dict_transform
#[test]
fn test_allowlist_entry_with_dict_transform() {
    let transform = HashMap::from([("Authorization".to_owned(), "Bearer TOKEN".to_owned())]);
    let entry = AllowlistEntry {
        domain: Some("github.com".to_owned()),
        transform: Some(Transform::Map(transform.clone())),
        ..Default::default()
    };

    assert_eq!(entry.domain.as_deref(), Some("github.com"));
    assert_eq!(entry.transform, Some(Transform::Map(transform)));
    // Serialization preserves it as a dict
    let dumped: Value = serde_json::to_value(&entry).unwrap();
    assert_eq!(
        dumped["transform"],
        json!({"Authorization": "Bearer TOKEN"})
    );
}

// upstream-test: interactions/test_integration.py::test_allowlist_entry_with_list_transform
#[test]
fn test_allowlist_entry_with_list_transform() {
    let transform = vec![HashMap::from([(
        "Authorization".to_owned(),
        "Bearer TOKEN".to_owned(),
    )])];
    let entry = AllowlistEntry {
        domain: Some("github.com".to_owned()),
        transform: Some(Transform::List(transform.clone())),
        ..Default::default()
    };

    assert_eq!(entry.domain.as_deref(), Some("github.com"));
    assert_eq!(entry.transform, Some(Transform::List(transform)));
    // Serialization preserves it as a list
    let dumped: Value = serde_json::to_value(&entry).unwrap();
    assert_eq!(
        dumped["transform"],
        json!([{"Authorization": "Bearer TOKEN"}])
    );
}
