//! Ports of `google/genai/tests/afc/test_get_max_remote_calls_for_afc.py`:
//! the request budget read off the config, and what the AFC loops (unary,
//! streaming and chat) do once it is spent.
//!
//! Python mocks `Models._generate_content` and
//! `_extra_utils.get_function_response_parts` to observe the loop; here a mock
//! HTTP server answers the requests and a counting tool observes whether a
//! function ran. `maximum_remote_calls=5.0` (a Python float) is ported with the
//! integer `5`: the field is an `i64`.

use std::sync::atomic::Ordering;

use futures_util::StreamExt;
use gemini_genai::{
    __test_support::extra_utils::get_max_remote_calls_afc,
    Error,
    types::{AutomaticFunctionCallingConfig, GenerateContentConfig},
};
use serde_json::json;
use wiremock::MockServer;

use super::{
    common::test_client,
    support::{
        config_with_tool, counting_tool, function_call_candidate, mount_next, request_count,
        sse_reply, text_candidate, unary_reply,
    },
};

fn afc(disable: Option<bool>, maximum_remote_calls: Option<i64>) -> GenerateContentConfig {
    GenerateContentConfig {
        automatic_function_calling: Some(AutomaticFunctionCallingConfig {
            disable,
            maximum_remote_calls,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// `ValueError` upstream; the typed error here is [`Error::Validation`].
fn assert_rejected(config: &GenerateContentConfig) {
    let result = get_max_remote_calls_afc(Some(config));
    assert!(
        matches!(result, Err(Error::Validation(_))),
        "expected Error::Validation, got {result:?}"
    );
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_config_is_none
#[test]
fn test_config_is_none() {
    assert_eq!(get_max_remote_calls_afc(None).ok(), Some(10));
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_afc_unset_max_unset
#[test]
fn test_afc_unset_max_unset() {
    assert_eq!(
        get_max_remote_calls_afc(Some(&GenerateContentConfig::default())).ok(),
        Some(10)
    );
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_afc_unset_max_set
#[test]
fn test_afc_unset_max_set() {
    assert_eq!(
        get_max_remote_calls_afc(Some(&afc(None, Some(20)))).ok(),
        Some(20)
    );
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_afc_disabled_max_unset
#[test]
fn test_afc_disabled_max_unset() {
    assert_rejected(&afc(Some(true), None));
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_afc_disabled_max_set
#[test]
fn test_afc_disabled_max_set() {
    assert_rejected(&afc(Some(true), Some(20)));
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_afc_d_max_unset
#[test]
fn test_afc_d_max_unset() {
    assert_eq!(
        get_max_remote_calls_afc(Some(&afc(Some(false), None))).ok(),
        Some(10)
    );
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_afc_d_max_set
#[test]
fn test_afc_d_max_set() {
    assert_eq!(
        get_max_remote_calls_afc(Some(&afc(Some(false), Some(5)))).ok(),
        Some(5)
    );
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_afc_enabled_max_set_to_zero
#[test]
fn test_afc_enabled_max_set_to_zero() {
    assert_rejected(&afc(Some(false), Some(0)));
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_afc_enabled_max_set_to_negative
#[test]
fn test_afc_enabled_max_set_to_negative() {
    assert_rejected(&afc(Some(false), Some(-1)));
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_afc_enabled_max_set_to_float
#[test]
fn test_afc_enabled_max_set_to_float() {
    assert_eq!(
        get_max_remote_calls_afc(Some(&afc(Some(false), Some(5)))).ok(),
        Some(5)
    );
}

const WEATHER_QUESTION: &str = "what is the weather in San Francisco?";

/// The `functionCall` upstream's mocked model asks for.
fn weather_call(name: &str) -> serde_json::Value {
    function_call_candidate(name, &json!({"location": "San Francisco"}))
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_generate_content_spent_budget_does_not_run_functions
#[tokio::test]
async fn test_generate_content_spent_budget_does_not_run_functions() {
    // The one allowed request is spent asking, so nothing is run.
    let name = "afc_budget_generate_content_weather";
    let server = MockServer::start().await;
    mount_next(&server, unary_reply(&weather_call(name))).await;
    let (tool, calls) = counting_tool(name, "sunny");

    let response = test_client(server.uri())
        .models()
        .generate_content(
            "test_model",
            WEATHER_QUESTION,
            Some(config_with_tool(tool, Some(1))),
        )
        .await
        .expect("the request succeeds");

    assert_eq!(request_count(&server).await, 1);
    // The result could not have been delivered, so the function is never called.
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(response.function_calls().len(), 1);
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_generate_content_stream_spent_budget_does_not_run_functions
#[tokio::test]
async fn test_generate_content_stream_spent_budget_does_not_run_functions() {
    // The same over a stream: chunks are yielded, nothing is run.
    let name = "afc_budget_stream_weather";
    let server = MockServer::start().await;
    mount_next(&server, sse_reply(&[weather_call(name)])).await;
    let (tool, calls) = counting_tool(name, "sunny");

    let stream = test_client(server.uri())
        .models()
        .generate_content_stream(
            "test_model",
            WEATHER_QUESTION,
            Some(config_with_tool(tool, Some(1))),
        )
        .await
        .expect("the request succeeds");
    let chunks: Vec<_> = stream.collect().await;

    assert_eq!(request_count(&server).await, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let first = chunks[0].as_ref().expect("the chunk is not an error");
    assert_eq!(first.function_calls().len(), 1);
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_send_message_spent_budget_records_the_turn_once
#[tokio::test]
async fn test_send_message_spent_budget_records_the_turn_once() {
    // The turn is recorded once, as the message and the unanswered call.
    let name = "afc_budget_send_message_weather";
    let server = MockServer::start().await;
    mount_next(&server, unary_reply(&weather_call(name))).await;
    let (tool, calls) = counting_tool(name, "sunny");

    let mut chat = test_client(server.uri()).chats().create(
        "test_model",
        Some(config_with_tool(tool, Some(1))),
        None,
    );
    chat.send_message(WEATHER_QUESTION, None)
        .await
        .expect("the request succeeds");
    let history = chat.get_history(false);

    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // The model's function call is recorded once, not twice, and the history
    // is left ready for the caller to answer that call themselves.
    let roles: Vec<_> = history
        .iter()
        .map(|content| content.role.as_deref())
        .collect();
    assert_eq!(roles, [Some("user"), Some("model")]);
    assert!(
        history[1].parts.as_ref().expect("parts")[0]
            .function_call
            .is_some()
    );
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_send_message_budget_of_two_answers_the_call
#[tokio::test]
async fn test_send_message_budget_of_two_answers_the_call() {
    // A budget of two is the smallest that lets the model answer.
    let name = "afc_budget_two_weather";
    let server = MockServer::start().await;
    mount_next(&server, unary_reply(&weather_call(name))).await;
    mount_next(&server, unary_reply(&text_candidate("It is sunny."))).await;
    let (tool, _calls) = counting_tool(name, "sunny");

    let mut chat = test_client(server.uri()).chats().create(
        "test_model",
        Some(config_with_tool(tool, Some(2))),
        None,
    );
    let response = chat
        .send_message(WEATHER_QUESTION, None)
        .await
        .expect("both requests succeed");
    let history = chat.get_history(false);

    assert_eq!(response.text().as_deref(), Some("It is sunny."));
    let roles: Vec<_> = history
        .iter()
        .map(|content| content.role.as_deref())
        .collect();
    assert_eq!(
        roles,
        [Some("user"), Some("model"), Some("user"), Some("model")]
    );
    assert!(
        history[2].parts.as_ref().expect("parts")[0]
            .function_response
            .is_some()
    );
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_spent_budget_leaves_the_afc_history_empty
#[tokio::test]
async fn test_spent_budget_leaves_the_afc_history_empty() {
    // Nothing ran, so automatic function calling added no turns to report.
    let name = "afc_budget_empty_history_weather";
    let server = MockServer::start().await;
    mount_next(&server, unary_reply(&weather_call(name))).await;
    let (tool, _calls) = counting_tool(name, "sunny");

    let response = test_client(server.uri())
        .models()
        .generate_content(
            "test_model",
            WEATHER_QUESTION,
            Some(config_with_tool(tool, Some(1))),
        )
        .await
        .expect("the request succeeds");

    assert!(
        response
            .automatic_function_calling_history
            .as_deref()
            .is_none_or(<[_]>::is_empty)
    );
}

// upstream-test: afc/test_get_max_remote_calls_for_afc.py::test_afc_history_holds_the_rounds_that_completed
#[tokio::test]
async fn test_afc_history_holds_the_rounds_that_completed() {
    // A round that was delivered is reported; the budget stops after it.
    let name = "afc_budget_rounds_weather";
    let server = MockServer::start().await;
    mount_next(&server, unary_reply(&weather_call(name))).await;
    mount_next(&server, unary_reply(&text_candidate("It is sunny."))).await;
    let (tool, calls) = counting_tool(name, "sunny");

    let response = test_client(server.uri())
        .models()
        .generate_content(
            "test_model",
            WEATHER_QUESTION,
            Some(config_with_tool(tool, Some(2))),
        )
        .await
        .expect("both requests succeed");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let history = response
        .automatic_function_calling_history
        .expect("a delivered round is reported");
    let roles: Vec<_> = history
        .iter()
        .map(|content| content.role.as_deref())
        .collect();
    assert_eq!(roles, [Some("user"), Some("model"), Some("user")]);
    assert!(
        history[1].parts.as_ref().expect("parts")[0]
            .function_call
            .is_some()
    );
    assert!(
        history[2].parts.as_ref().expect("parts")[0]
            .function_response
            .is_some()
    );
}
