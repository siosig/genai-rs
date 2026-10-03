//! Ports of `google/genai/tests/afc/test_should_append_afc_history.py`.

use gemini_genai::{
    __test_support::extra_utils::should_append_afc_history,
    types::{AutomaticFunctionCallingConfig, GenerateContentConfig},
};

fn config(ignore_call_history: Option<bool>) -> GenerateContentConfig {
    GenerateContentConfig {
        automatic_function_calling: Some(AutomaticFunctionCallingConfig {
            ignore_call_history,
            ..Default::default()
        }),
        ..Default::default()
    }
}

// upstream-test: afc/test_should_append_afc_history.py::test_should_append_afc_history_with_default_config
#[test]
fn test_should_append_afc_history_with_default_config() {
    assert!(should_append_afc_history(Some(
        &GenerateContentConfig::default()
    )));
}

// upstream-test: afc/test_should_append_afc_history.py::test_should_append_afc_history_with_empty_afc_config
#[test]
fn test_should_append_afc_history_with_empty_afc_config() {
    assert!(should_append_afc_history(Some(&config(None))));
}

// upstream-test: afc/test_should_append_afc_history.py::test_should_append_afc_history_with_ignore_call_history_true
#[test]
fn test_should_append_afc_history_with_ignore_call_history_true() {
    assert!(!should_append_afc_history(Some(&config(Some(true)))));
}

// upstream-test: afc/test_should_append_afc_history.py::test_should_append_afc_history_with_ignore_call_history_false
#[test]
fn test_should_append_afc_history_with_ignore_call_history_false() {
    assert!(should_append_afc_history(Some(&config(Some(false)))));
}
