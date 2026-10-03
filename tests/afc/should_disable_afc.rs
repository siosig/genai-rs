//! Ports of `google/genai/tests/afc/test_should_disable_afc.py`.
//!
//! Python's dict-config cases (`{'automatic_function_calling':
//! {'maximum_remote_calls': 0.0}}`) have no Rust counterpart: `maximum_remote_calls`
//! is an `i64`, so those cases are ported with the equivalent integer.

use gemini_genai::{
    __test_support::extra_utils::should_disable_afc,
    types::{AutomaticFunctionCallingConfig, GenerateContentConfig},
};

fn config(disable: Option<bool>, maximum_remote_calls: Option<i64>) -> GenerateContentConfig {
    GenerateContentConfig {
        automatic_function_calling: Some(AutomaticFunctionCallingConfig {
            disable,
            maximum_remote_calls,
            ..Default::default()
        }),
        ..Default::default()
    }
}

// upstream-test: afc/test_should_disable_afc.py::test_config_is_none
#[test]
fn test_config_is_none() {
    assert!(!should_disable_afc(None));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_config_unset
#[test]
fn test_afc_config_unset() {
    assert!(!should_disable_afc(Some(&GenerateContentConfig::default())));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_unset_max_0
#[test]
fn test_afc_enable_unset_max_0() {
    assert!(should_disable_afc(Some(&config(None, Some(0)))));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_unset_max_negative
#[test]
fn test_afc_enable_unset_max_negative() {
    assert!(should_disable_afc(Some(&config(None, Some(-1)))));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_unset_max_0_0
#[test]
fn test_afc_enable_unset_max_0_0() {
    assert!(should_disable_afc(Some(&config(None, Some(0)))));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_unset_max_1
#[test]
fn test_afc_enable_unset_max_1() {
    assert!(!should_disable_afc(Some(&config(None, Some(1)))));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_unset_max_1_0
#[test]
fn test_afc_enable_unset_max_1_0() {
    assert!(!should_disable_afc(Some(&config(None, Some(1)))));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_false_max_unset
#[test]
fn test_afc_enable_false_max_unset() {
    assert!(should_disable_afc(Some(&config(Some(true), None))));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_false_max_0
#[test]
fn test_afc_enable_false_max_0() {
    assert!(should_disable_afc(Some(&config(Some(true), Some(0)))));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_false_max_negative
#[test]
fn test_afc_enable_false_max_negative() {
    assert!(should_disable_afc(Some(&config(Some(true), Some(-1)))));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_false_max_0_0
#[test]
fn test_afc_enable_false_max_0_0() {
    assert!(should_disable_afc(Some(&config(None, Some(0)))));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_false_max_1
#[test]
fn test_afc_enable_false_max_1() {
    assert!(should_disable_afc(Some(&config(Some(true), Some(1)))));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_true_max_unset
#[test]
fn test_afc_enable_true_max_unset() {
    assert!(!should_disable_afc(Some(&config(Some(false), None))));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_true_max_0
#[test]
fn test_afc_enable_true_max_0() {
    assert!(should_disable_afc(Some(&config(Some(false), Some(0)))));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_true_max_negative
#[test]
fn test_afc_enable_true_max_negative() {
    assert!(should_disable_afc(Some(&config(Some(false), Some(-1)))));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_true_max_0_0
#[test]
fn test_afc_enable_true_max_0_0() {
    assert!(should_disable_afc(Some(&config(None, Some(0)))));
}

// upstream-test: afc/test_should_disable_afc.py::test_afc_enable_true_max_1
#[test]
fn test_afc_enable_true_max_1() {
    assert!(!should_disable_afc(Some(&config(Some(false), Some(1)))));
}
