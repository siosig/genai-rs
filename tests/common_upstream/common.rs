use gemini_genai::__test_support::api_client::recursive_body_update;
use serde_json::{Value, json};

fn merged(target: Value, update: &Value) -> Value {
    let (Value::Object(mut target), Value::Object(update)) = (target, update.clone()) else {
        panic!("test inputs must be JSON objects");
    };
    recursive_body_update(&mut target, &update);
    Value::Object(target)
}

// upstream-test: common/test_common.py::test_recursive_dict_update
#[test]
fn test_recursive_dict_update() {
    let cases = [
        (
            "simple_update",
            json!({"a": 1, "b": 2}),
            json!({"b": 3, "c": 4}),
            json!({"a": 1, "b": 3, "c": 4}),
        ),
        (
            "nested_update",
            json!({"a": 1, "b": {"x": 10, "y": 20}}),
            json!({"b": {"y": 30, "z": 40}, "c": 3}),
            json!({"a": 1, "b": {"x": 10, "y": 30, "z": 40}, "c": 3}),
        ),
        (
            "add_new_nested_dict",
            json!({"a": 1}),
            json!({"b": {"x": 10, "y": 20}}),
            json!({"a": 1, "b": {"x": 10, "y": 20}}),
        ),
        (
            "empty_target",
            json!({}),
            json!({"a": 1, "b": {"x": 10}}),
            json!({"a": 1, "b": {"x": 10}}),
        ),
        (
            "empty_update",
            json!({"a": 1, "b": {"x": 10}}),
            json!({}),
            json!({"a": 1, "b": {"x": 10}}),
        ),
        (
            "overwrite_non_dict_with_dict",
            json!({"a": 1, "b": 2}),
            json!({"b": {"x": 10}}),
            json!({"a": 1, "b": {"x": 10}}),
        ),
        (
            "overwrite_dict_with_non_dict",
            json!({"a": 1, "b": {"x": 10}}),
            json!({"b": 2}),
            json!({"a": 1, "b": 2}),
        ),
        (
            "deeper_nesting",
            json!({"a": {"b": {"c": 1, "d": 2}, "e": 3}}),
            json!({"a": {"b": {"d": 4, "f": 5}, "g": 6}, "h": 7}),
            json!({"a": {"b": {"c": 1, "d": 4, "f": 5}, "e": 3, "g": 6}, "h": 7}),
        ),
        (
            "different_value_types",
            json!({"key1": "string_val", "key2": {"nested_int": 100}}),
            json!({"key1": 123, "key2": {"nested_list": [1, 2, 3]}, "key3": true}),
            json!({"key1": 123, "key2": {"nested_int": 100, "nested_list": [1, 2, 3]}, "key3": true}),
        ),
        (
            "update_with_empty_nested_dict",
            json!({"a": {"b": 1}}),
            json!({"a": {}}),
            json!({"a": {"b": 1}}),
        ),
        (
            "target_with_empty_nested_dict",
            json!({"a": {}}),
            json!({"a": {"b": 1}}),
            json!({"a": {"b": 1}}),
        ),
        (
            "key_case_alignment_check",
            json!({"first_name": "John", "contact_info": {"email_address": "john@example.com"}}),
            json!({"firstName": "Jane", "contact_info": {"email_address": "jane@example.com", "phone_number": "123"}}),
            json!({"first_name": "Jane", "contact_info": {"email_address": "jane@example.com", "phone_number": "123"}}),
        ),
    ];
    for (id, target, update, expected) in cases {
        assert_eq!(merged(target, &update), expected, "case {id}");
    }
}
