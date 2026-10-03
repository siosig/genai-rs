//! Ports of `google/genai/tests/afc/test_raise_error_for_afc_incompatible_config.py`.

use gemini_genai::{
    __test_support::extra_utils::raise_error_for_afc_incompatible_config,
    Error,
    types::{
        AutomaticFunctionCallingConfig, FunctionCallingConfig, GenerateContentConfig, ToolConfig,
    },
};

fn config(
    afc: Option<AutomaticFunctionCallingConfig>,
    tool_config: ToolConfig,
) -> GenerateContentConfig {
    GenerateContentConfig {
        automatic_function_calling: afc,
        tool_config: Some(tool_config),
        ..Default::default()
    }
}

fn afc(disable: Option<bool>) -> AutomaticFunctionCallingConfig {
    AutomaticFunctionCallingConfig {
        disable,
        ..Default::default()
    }
}

fn tool_config(stream_function_call_arguments: Option<bool>) -> ToolConfig {
    ToolConfig {
        function_calling_config: Some(FunctionCallingConfig {
            stream_function_call_arguments,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// `ValueError` upstream; the typed error here is [`Error::Validation`].
fn assert_rejected(config: &GenerateContentConfig) {
    let result = raise_error_for_afc_incompatible_config(Some(config));
    assert!(
        matches!(result, Err(Error::Validation(_))),
        "expected Error::Validation, got {result:?}"
    );
}

// upstream-test: afc/test_raise_error_for_afc_incompatible_config.py::test_config_is_none
#[test]
fn test_config_is_none() {
    assert!(raise_error_for_afc_incompatible_config(None).is_ok());
}

// upstream-test: afc/test_raise_error_for_afc_incompatible_config.py::test_tool_config_config_unset
#[test]
fn test_tool_config_config_unset() {
    let config = GenerateContentConfig {
        automatic_function_calling: Some(AutomaticFunctionCallingConfig {
            disable: Some(false),
            maximum_remote_calls: Some(1),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(raise_error_for_afc_incompatible_config(Some(&config)).is_ok());
}

// upstream-test: afc/test_raise_error_for_afc_incompatible_config.py::test_function_calling_config_unset
#[test]
fn test_function_calling_config_unset() {
    let config = GenerateContentConfig {
        automatic_function_calling: Some(AutomaticFunctionCallingConfig {
            disable: Some(false),
            maximum_remote_calls: Some(1),
            ..Default::default()
        }),
        tool_config: Some(ToolConfig::default()),
        ..Default::default()
    };
    assert!(raise_error_for_afc_incompatible_config(Some(&config)).is_ok());
}

// upstream-test: afc/test_raise_error_for_afc_incompatible_config.py::test_compatible_config_afc_disabled
#[test]
fn test_compatible_config_afc_disabled() {
    let config = config(Some(afc(Some(true))), tool_config(Some(false)));
    assert!(raise_error_for_afc_incompatible_config(Some(&config)).is_ok());
}

// upstream-test: afc/test_raise_error_for_afc_incompatible_config.py::test_compatible_config_stream_function_call_arguments_unset_afc_unset
#[test]
fn test_compatible_config_stream_function_call_arguments_unset_afc_unset() {
    let config = config(None, tool_config(None));
    assert!(raise_error_for_afc_incompatible_config(Some(&config)).is_ok());
}

// upstream-test: afc/test_raise_error_for_afc_incompatible_config.py::test_compatible_config_stream_function_call_arguments_unset_no_disable_afc
#[test]
fn test_compatible_config_stream_function_call_arguments_unset_no_disable_afc() {
    let config = config(Some(afc(None)), tool_config(None));
    assert!(raise_error_for_afc_incompatible_config(Some(&config)).is_ok());
}

// upstream-test: afc/test_raise_error_for_afc_incompatible_config.py::test_compatible_config_stream_function_call_arguments_unset_disable_afc_true
#[test]
fn test_compatible_config_stream_function_call_arguments_unset_disable_afc_true() {
    let config = config(Some(afc(Some(true))), tool_config(None));
    assert!(raise_error_for_afc_incompatible_config(Some(&config)).is_ok());
}

// upstream-test: afc/test_raise_error_for_afc_incompatible_config.py::test_incompatible_config_stream_function_call_arguments_set_enable_afc
#[test]
fn test_incompatible_config_stream_function_call_arguments_set_enable_afc() {
    assert_rejected(&config(Some(afc(Some(false))), tool_config(Some(true))));
}

// upstream-test: afc/test_raise_error_for_afc_incompatible_config.py::test_incompatible_config_stream_function_call_arguments_set_no_afc_config
#[test]
fn test_incompatible_config_stream_function_call_arguments_set_no_afc_config() {
    assert_rejected(&config(None, tool_config(Some(true))));
}

// upstream-test: afc/test_raise_error_for_afc_incompatible_config.py::test_incompatible_config_stream_function_call_arguments_set_no_disable_afc
#[test]
fn test_incompatible_config_stream_function_call_arguments_set_no_disable_afc() {
    assert_rejected(&config(Some(afc(None)), tool_config(Some(true))));
}
