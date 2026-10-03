//! Ports of `google/genai/tests/afc/test_convert_number_values_for_function_call_args.py`.

use gemini_genai::__test_support::extra_utils::convert_number_values_for_function_call_args;
use serde_json::{Value, json};

/// `serde_json::Value`'s equality tells `1` from `1.0`, so comparing against
/// an integer literal checks the conversion itself.
fn convert(value: &Value) -> Value {
    convert_number_values_for_function_call_args(value)
}

// upstream-test: afc/test_convert_number_values_for_function_call_args.py::test_integer_value
#[test]
fn test_integer_value() {
    assert_eq!(convert(&json!(1)), json!(1));
}

// upstream-test: afc/test_convert_number_values_for_function_call_args.py::test_float_value
#[test]
fn test_float_value() {
    let converted = convert(&json!(1.0));
    assert_eq!(converted, json!(1));
    assert!(converted.is_i64(), "{converted} should be an integer");
}

// upstream-test: afc/test_convert_number_values_for_function_call_args.py::test_string_value
#[test]
fn test_string_value() {
    assert_eq!(convert(&json!("1.0")), json!("1.0"));
}

// upstream-test: afc/test_convert_number_values_for_function_call_args.py::test_boolean_value
#[test]
fn test_boolean_value() {
    assert_eq!(convert(&json!(true)), json!(true));
}

// upstream-test: afc/test_convert_number_values_for_function_call_args.py::test_none_value
#[test]
fn test_none_value() {
    assert_eq!(convert(&Value::Null), Value::Null);
}

// upstream-test: afc/test_convert_number_values_for_function_call_args.py::test_float_value_with_decimal
#[test]
fn test_float_value_with_decimal() {
    assert_eq!(convert(&json!(1.1)), json!(1.1));
}

// upstream-test: afc/test_convert_number_values_for_function_call_args.py::test_dict_value
#[test]
fn test_dict_value() {
    let converted = convert(&json!({"key1": 1.0, "key2": 1.1}));
    assert_eq!(converted, json!({"key1": 1, "key2": 1.1}));
    assert!(converted["key1"].is_i64());
}

// upstream-test: afc/test_convert_number_values_for_function_call_args.py::test_list_value
#[test]
fn test_list_value() {
    let converted = convert(&json!([1.0, 1.1, 1.2]));
    assert_eq!(converted, json!([1, 1.1, 1.2]));
    assert!(converted[0].is_i64());
}

// upstream-test: afc/test_convert_number_values_for_function_call_args.py::test_nested_value
#[test]
fn test_nested_value() {
    let converted = convert(&json!({"key1": 1.0, "key2": {"key3": 1.0, "key4": [1.2, 2.0]}}));
    assert_eq!(
        converted,
        json!({"key1": 1, "key2": {"key3": 1, "key4": [1.2, 2]}})
    );
    assert!(converted["key2"]["key4"][1].is_i64());
}
