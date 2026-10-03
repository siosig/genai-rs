//! Ports of `google/genai/tests/live/test_send_tool_response.py`.
//!
//! Developer-API behaviour only. Deviation: the top-level wire key is
//! camelCase `toolResponse` (Python writes `tool_response`; both are accepted
//! by the server).

use std::collections::HashMap;

use gemini_genai::{
    Error,
    types::{FunctionResponse, FunctionResponseScheduling},
};
use serde_json::{Value, json};

use super::run_send;

fn weather_response(temperature: f64, id: Option<&str>) -> FunctionResponse {
    FunctionResponse {
        id: id.map(str::to_owned),
        name: Some("get_current_weather".to_owned()),
        response: Some(HashMap::from([
            ("temperature".to_owned(), json!(temperature)),
            ("unit".to_owned(), json!("C")),
        ])),
        ..Default::default()
    }
}

// upstream-test: live/test_send_tool_response.py::test_function_response_dict
#[tokio::test]
async fn test_function_response_dict() {
    let (result, sent) = run_send(async |s| {
        s.send_tool_response(vec![weather_response(14.5, Some("some-id"))])
            .await
    })
    .await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("toolResponse").is_some(), "{sent}");
    let first = &sent["toolResponse"]["functionResponses"][0];
    assert_eq!(first["name"], "get_current_weather");
    assert_eq!(first["response"]["temperature"], 14.5);
    assert_eq!(first["response"]["unit"], "C");
}

// upstream-test: live/test_send_tool_response.py::test_function_response
#[tokio::test]
async fn test_function_response() {
    // Keys inside `response` are user data and must reach the wire untouched
    // (no snake_case -> camelCase rewriting).
    let response = HashMap::from([
        ("temperature".to_owned(), json!(14.5)),
        ("unit".to_owned(), json!("C")),
        ("user_name".to_owned(), json!("test_user_name")),
        ("userEmail".to_owned(), json!("test_user_email")),
    ]);
    let input = FunctionResponse {
        id: Some("some-id".to_owned()),
        name: Some("get_current_weather".to_owned()),
        response: Some(response.clone()),
        ..Default::default()
    };
    let (result, sent) = run_send(async |s| s.send_tool_response(vec![input]).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("toolResponse").is_some(), "{sent}");
    let first = &sent["toolResponse"]["functionResponses"][0];
    assert_eq!(first["name"], "get_current_weather");
    assert_eq!(first["response"], json!(response));
}

// upstream-test: live/test_send_tool_response.py::test_function_response_scheduling
#[tokio::test]
async fn test_function_response_scheduling() {
    let input = FunctionResponse {
        will_continue: Some(true),
        scheduling: Some(FunctionResponseScheduling::Silent),
        ..weather_response(14.5, Some("some-id"))
    };
    let (result, sent) = run_send(async |s| s.send_tool_response(vec![input]).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("toolResponse").is_some(), "{sent}");
    let first = &sent["toolResponse"]["functionResponses"][0];
    assert_eq!(first["willContinue"], Value::Bool(true));
    assert_eq!(first["scheduling"], "SILENT");
}

// upstream-test: live/test_send_tool_response.py::test_function_response_list
#[tokio::test]
async fn test_function_response_list() {
    let inputs = vec![
        weather_response(14.5, Some("1")),
        weather_response(99.9, Some("2")),
    ];
    let (result, sent) = run_send(async |s| s.send_tool_response(inputs).await).await;
    result.unwrap();
    let sent = sent.unwrap();
    assert!(sent.get("toolResponse").is_some(), "{sent}");
    let responses = sent["toolResponse"]["functionResponses"]
        .as_array()
        .unwrap();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["response"]["temperature"], 14.5);
    assert_eq!(responses[1]["response"]["temperature"], 99.9);
}

// upstream-test: live/test_send_tool_response.py::test_missing_id
#[tokio::test]
async fn test_missing_id() {
    let inputs = vec![
        weather_response(14.5, Some("1")),
        weather_response(99.9, None),
    ];
    let (result, sent) = run_send(async |s| s.send_tool_response(inputs).await).await;
    assert!(
        matches!(result, Err(Error::Validation(_))),
        "unexpected result: {result:?}"
    );
    assert!(sent.is_none(), "nothing may be sent when validation fails");
}
