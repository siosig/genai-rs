//! Port of `transformers/test_t_contents.py`.

use gemini_genai::__test_support::transformers as t;
use serde_json::{Value, json};

use super::t_part::{file, file_part};

fn user(parts: Value) -> Value {
    let mut content = json!({"role": "user"});
    content["parts"] = parts;
    content
}

fn model(parts: Value) -> Value {
    let mut content = json!({"role": "model"});
    content["parts"] = parts;
    content
}

fn text(value: &str) -> Value {
    json!({"text": value})
}

fn call(name: &str, arg: &str, value: &str) -> Value {
    json!({"function_call": {"name": name, "args": {arg: value}}})
}

fn response(name: &str, answer: &str) -> Value {
    json!({"function_response": {"name": name, "response": {"answer": answer}}})
}

// upstream-test: transformers/test_t_contents.py::test_none
#[test]
fn test_none() {
    assert!(t::t_contents(Value::Null).is_err());
}

// upstream-test: transformers/test_t_contents.py::test_empty_list
#[test]
fn test_empty_list() {
    assert!(t::t_contents(json!([])).is_err());
}

// upstream-test: transformers/test_t_contents.py::test_content
#[test]
fn test_content() {
    let content = json!({"parts": [text("test")]});
    assert_eq!(
        t::t_contents(json!([content.clone()])).unwrap(),
        json!([content])
    );
}

// upstream-test: transformers/test_t_contents.py::test_content_dict
#[test]
fn test_content_dict() {
    let content = json!({"role": "user", "parts": [text("test")]});
    assert_eq!(
        t::t_contents(json!([content.clone()])).unwrap(),
        json!([content])
    );
}

// upstream-test: transformers/test_t_contents.py::test_empty_dict
#[test]
fn test_empty_dict() {
    assert_eq!(t::t_contents(json!({})).unwrap(), json!([{}]));
}

// upstream-test: transformers/test_t_contents.py::test_invalid_dict
#[test]
fn test_invalid_dict() {
    assert!(t::t_contents(json!({"invalid_key": "test"})).is_err());
}

// upstream-test: transformers/test_t_contents.py::test_text_part
#[test]
fn test_text_part() {
    assert_eq!(
        t::t_contents(json!([text("test")])).unwrap(),
        json!([user(json!([text("test")]))])
    );
}

// upstream-test: transformers/test_t_contents.py::test_function_call_part
#[test]
fn test_function_call_part() {
    let part = call("test_func", "arg1", "value1");
    assert_eq!(
        t::t_contents(json!([part.clone()])).unwrap(),
        json!([model(json!([part]))])
    );
}

// upstream-test: transformers/test_t_contents.py::test_text_part_dict
#[test]
fn test_text_part_dict() {
    assert_eq!(
        t::t_contents(json!([{"text": "test"}])).unwrap(),
        json!([user(json!([text("test")]))])
    );
}

// upstream-test: transformers/test_t_contents.py::test_function_call_part_dict
#[test]
fn test_function_call_part_dict() {
    let part = call("test_func", "arg1", "value1");
    assert_eq!(
        t::t_contents(json!([part.clone()])).unwrap(),
        json!([model(json!([part]))])
    );
}

// upstream-test: transformers/test_t_contents.py::test_empty_string
#[test]
fn test_empty_string() {
    assert_eq!(
        t::t_contents(json!("")).unwrap(),
        json!([user(json!([text("")]))])
    );
}

// upstream-test: transformers/test_t_contents.py::test_string
#[test]
fn test_string() {
    assert_eq!(
        t::t_contents(json!("test")).unwrap(),
        json!([user(json!([text("test")]))])
    );
}

// upstream-test: transformers/test_t_contents.py::test_file
#[test]
fn test_file() {
    assert_eq!(
        t::t_contents(file(Some("gs://test"), Some("image/png"))).unwrap(),
        json!([user(json!([file_part()]))])
    );
}

// upstream-test: transformers/test_t_contents.py::test_file_dict
#[test]
fn test_file_dict() {
    assert_eq!(
        t::t_contents(json!({"file_uri": "gs://test", "mime_type": "image/png"})).unwrap(),
        json!([user(json!([file_part()]))])
    );
}

// upstream-test: transformers/test_t_contents.py::test_file_dict_list
#[test]
fn test_file_dict_list() {
    assert_eq!(
        t::t_contents(json!([{"file_uri": "gs://test", "mime_type": "image/png"}])).unwrap(),
        json!([user(json!([file_part()]))])
    );
}

// upstream-test: transformers/test_t_contents.py::test_file_no_uri
#[test]
fn test_file_no_uri() {
    assert!(t::t_contents(file(None, Some("image/png"))).is_err());
}

// upstream-test: transformers/test_t_contents.py::test_file_no_mime_type
#[test]
fn test_file_no_mime_type() {
    assert!(t::t_contents(file(Some("gs://test"), None)).is_err());
}

// upstream-test: transformers/test_t_contents.py::test_string_list
#[test]
fn test_string_list() {
    assert_eq!(
        t::t_contents(json!(["test1", "test2"])).unwrap(),
        json!([user(json!([text("test1"), text("test2")]))])
    );
}

// upstream-test: transformers/test_t_contents.py::test_file_list
#[test]
fn test_file_list() {
    let part = |uri: &str| json!({"file_data": {"file_uri": uri, "mime_type": "image/png"}});
    assert_eq!(
        t::t_contents(json!([
            file(Some("gs://test1"), Some("image/png")),
            file(Some("gs://test2"), Some("image/png")),
        ]))
        .unwrap(),
        json!([user(json!([part("gs://test1"), part("gs://test2")]))])
    );
}

// upstream-test: transformers/test_t_contents.py::test_string_file_list
#[test]
fn test_string_file_list() {
    let file_part = json!({"file_data": {"file_uri": "gs://test2", "mime_type": "image/png"}});
    assert_eq!(
        t::t_contents(json!([
            "test1",
            file(Some("gs://test2"), Some("image/png")),
        ]))
        .unwrap(),
        json!([user(json!([text("test1"), file_part]))])
    );
}

// upstream-test: transformers/test_t_contents.py::test_function_call_list
#[test]
fn test_function_call_list() {
    let first = call("test_func1", "arg1", "value1");
    let second = call("test_func2", "arg2", "value2");
    assert_eq!(
        t::t_contents(json!([first.clone(), second.clone()])).unwrap(),
        json!([model(json!([first, second]))])
    );
}

// upstream-test: transformers/test_t_contents.py::test_function_call_function_response_list
#[test]
fn test_function_call_function_response_list() {
    let call1 = call("test_func1", "arg1", "value1");
    let call2 = call("test_func2", "arg2", "value2");
    let response1 = response("test_func1", "answer1");
    let response2 = response("test_func2", "answer2");
    assert_eq!(
        t::t_contents(json!([
            "question1",
            "question2",
            call1.clone(),
            call2.clone(),
            response1.clone(),
            response2.clone(),
        ]))
        .unwrap(),
        json!([
            user(json!([text("question1"), text("question2")])),
            model(json!([call1, call2])),
            user(json!([response1, response2])),
        ])
    );
}

// upstream-test: transformers/test_t_contents.py::test_content_list
#[test]
fn test_content_list() {
    let contents = json!([{"parts": [text("test1")]}, {"parts": [text("test2")]}]);
    assert_eq!(t::t_contents(contents.clone()).unwrap(), contents);
}

// upstream-test: transformers/test_t_contents.py::test_content_text_part_list
#[test]
fn test_content_text_part_list() {
    assert_eq!(
        t::t_contents(json!([
            text("test1"),
            text("test2"),
            {"parts": [text("test3")]},
            text("test4"),
        ]))
        .unwrap(),
        json!([
            user(json!([text("test1"), text("test2")])),
            {"parts": [text("test3")]},
            user(json!([text("test4")])),
        ])
    );
}

// upstream-test: transformers/test_t_contents.py::test_list_of_text_part_list
#[test]
fn test_list_of_text_part_list() {
    let call1 = call("test_func1", "arg1", "value1");
    let call2 = call("test_func2", "arg2", "value2");
    let response1 = response("test_func1", "answer1");
    let response2 = response("test_func2", "answer2");
    let contents = json!([
        "question1",
        call1,
        response1,
        ["context2_1", "context2_2", text("context2_3")],
        "question2",
        call2,
        response2,
    ]);
    assert_eq!(
        t::t_contents(contents).unwrap(),
        json!([
            user(json!([text("question1")])),
            model(json!([call1])),
            user(json!([response1])),
            user(json!([
                text("context2_1"),
                text("context2_2"),
                text("context2_3")
            ])),
            user(json!([text("question2")])),
            model(json!([call2])),
            user(json!([response2])),
        ])
    );
}
