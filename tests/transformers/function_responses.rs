//! Port of `transformers/test_function_responses.py`.

use gemini_genai::{__test_support::transformers as t, types::FunctionResponse};
use serde_json::{Value, json};

fn weather(temperature: f64, id: &str) -> Value {
    json!({
        "name": "get_current_weather",
        "response": {"temperature": temperature, "unit": "C"},
        "id": id,
    })
}

// upstream-test: transformers/test_function_responses.py::test_function_response_dict
#[test]
fn test_function_response_dict() {
    let input = weather(14.5, "some-id");
    let responses = t::t_function_responses(input).unwrap();
    let responses = responses.as_array().unwrap();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["name"], "get_current_weather");
    assert_eq!(responses[0]["response"]["temperature"], 14.5);
    assert_eq!(responses[0]["response"]["unit"], "C");
}

// upstream-test: transformers/test_function_responses.py::test_send_function_response
#[test]
fn test_send_function_response() {
    let input = serde_json::to_value(FunctionResponse {
        name: Some("get_current_weather".to_owned()),
        response: Some(std::collections::HashMap::from([
            ("temperature".to_owned(), json!(14.5)),
            ("unit".to_owned(), json!("C")),
        ])),
        id: Some("some-id".to_owned()),
        ..Default::default()
    })
    .unwrap();
    let responses = t::t_function_responses(input).unwrap();
    let responses = responses.as_array().unwrap();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["name"], "get_current_weather");
    assert_eq!(responses[0]["response"]["temperature"], 14.5);
    assert_eq!(responses[0]["response"]["unit"], "C");
}

// upstream-test: transformers/test_function_responses.py::test_send_function_response_list
#[test]
fn test_send_function_response_list() {
    let responses =
        t::t_function_responses(json!([weather(14.5, "1"), weather(99.9, "2")])).unwrap();
    let responses = responses.as_array().unwrap();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["response"]["temperature"], 14.5);
    assert_eq!(responses[1]["response"]["temperature"], 99.9);
    assert_eq!(responses[1]["response"]["unit"], "C");
}
