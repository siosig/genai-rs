//! Ports of `google/genai/tests/afc/test_find_afc_incompatible_tool_indexes.py`.
//!
//! Python mixes `types.Tool` values with bare callables, `mcp_types.Tool` and
//! MCP adapters in one `tools` list; a callable is not a `types.Tool`, so it is
//! always AFC-compatible. In Rust every entry is a [`Tool`], and a callable is
//! a `Tool::from_function(..)` whose declared function has a registered
//! callable. Those stand in for Python's non-`Tool` entries (callable and
//! adapter) at the same list positions, so the expected indexes are the same
//! as upstream's. Python's `mcp_types.Tool` entry (a raw MCP tool description,
//! never a `types.Tool`) has no Rust counterpart and is omitted.

use gemini_genai::{
    __test_support::extra_utils::find_afc_incompatible_tool_indexes,
    function_tool,
    types::{
        ComputerUse, FunctionDeclaration, GenerateContentConfig, GoogleMaps, GoogleSearch,
        GoogleSearchRetrieval, McpServer, Retrieval, Tool, ToolCodeExecution, UrlContext,
    },
};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
struct CityArgs {
    city: String,
}

/// A `Tool` whose function has a registered callable (Python: a bare
/// callable or an MCP adapter in `tools`).
fn callable_tool(name: &str) -> Tool {
    Tool::from_function(function_tool::<CityArgs, _, _, _>(
        name,
        "Get the weather in a city.",
        |args: CityArgs| async move { Ok(format!("The weather in {} is sunny.", args.city)) },
    ))
}

fn declaration_tool(name: &str) -> Tool {
    Tool {
        function_declarations: Some(vec![FunctionDeclaration {
            name: Some(name.to_owned()),
            ..Default::default()
        }]),
        ..Default::default()
    }
}

fn config(tools: Vec<Tool>) -> GenerateContentConfig {
    GenerateContentConfig {
        tools: Some(tools),
        ..Default::default()
    }
}

/// The tools of upstream's "all compatible" cases, with `function_declarations`
/// as given.
fn compatible_tools(function_declarations: Option<Vec<FunctionDeclaration>>) -> Vec<Tool> {
    vec![
        Tool {
            google_search_retrieval: Some(GoogleSearchRetrieval::default()),
            ..Default::default()
        },
        Tool {
            retrieval: Some(Retrieval::default()),
            ..Default::default()
        },
        Tool {
            google_search: Some(GoogleSearch::default()),
            ..Default::default()
        },
        Tool {
            code_execution: Some(ToolCodeExecution::default()),
            ..Default::default()
        },
        Tool {
            google_maps: Some(GoogleMaps::default()),
            ..Default::default()
        },
        Tool {
            url_context: Some(UrlContext::default()),
            ..Default::default()
        },
        Tool {
            computer_use: Some(ComputerUse::default()),
            ..Default::default()
        },
        Tool {
            code_execution: Some(ToolCodeExecution::default()),
            ..Default::default()
        },
        Tool {
            function_declarations,
            ..Default::default()
        },
        callable_tool("afc_find_compat_callable"),
        callable_tool("afc_find_compat_adapter"),
    ]
}

// upstream-test: afc/test_find_afc_incompatible_tool_indexes.py::test_no_config_returns_empty_list
#[test]
fn test_no_config_returns_empty_list() {
    assert_eq!(
        find_afc_incompatible_tool_indexes(None, false),
        Vec::<usize>::new()
    );
}

// upstream-test: afc/test_find_afc_incompatible_tool_indexes.py::test_config_with_no_tools_returns_empty_list
#[test]
fn test_config_with_no_tools_returns_empty_list() {
    let config = GenerateContentConfig::default();
    assert_eq!(
        find_afc_incompatible_tool_indexes(Some(&config), false),
        Vec::<usize>::new()
    );
}

// upstream-test: afc/test_find_afc_incompatible_tool_indexes.py::test_empty_tools_list_returns_empty_list
#[test]
fn test_empty_tools_list_returns_empty_list() {
    let config = config(Vec::new());
    assert_eq!(
        find_afc_incompatible_tool_indexes(Some(&config), false),
        Vec::<usize>::new()
    );
}

// upstream-test: afc/test_find_afc_incompatible_tool_indexes.py::test_all_compatible_tools_returns_empty_list_with_empty_fd
#[test]
fn test_all_compatible_tools_returns_empty_list_with_empty_fd() {
    let config = config(compatible_tools(Some(Vec::new())));
    assert_eq!(
        find_afc_incompatible_tool_indexes(Some(&config), false),
        Vec::<usize>::new()
    );
}

// upstream-test: afc/test_find_afc_incompatible_tool_indexes.py::test_all_compatible_tools_returns_empty_list_with_none_fd
#[test]
fn test_all_compatible_tools_returns_empty_list_with_none_fd() {
    let config = config(compatible_tools(None));
    assert_eq!(
        find_afc_incompatible_tool_indexes(Some(&config), false),
        Vec::<usize>::new()
    );
}

// upstream-test: afc/test_find_afc_incompatible_tool_indexes.py::test_all_compatible_tools_returns_empty_list
#[test]
fn test_all_compatible_tools_returns_empty_list() {
    let config = config(compatible_tools(Some(Vec::new())));
    assert_eq!(
        find_afc_incompatible_tool_indexes(Some(&config), false),
        Vec::<usize>::new()
    );
}

// upstream-test: afc/test_find_afc_incompatible_tool_indexes.py::test_single_incompatible_tool
#[test]
fn test_single_incompatible_tool() {
    let config = config(vec![
        Tool {
            google_search_retrieval: Some(GoogleSearchRetrieval::default()),
            ..Default::default()
        },
        Tool {
            retrieval: Some(Retrieval::default()),
            ..Default::default()
        },
        declaration_tool("test_function"),
        callable_tool("afc_find_single_callable"),
        callable_tool("afc_find_single_adapter"),
    ]);
    assert_eq!(
        find_afc_incompatible_tool_indexes(Some(&config), false),
        vec![2]
    );
}

// upstream-test: afc/test_find_afc_incompatible_tool_indexes.py::test_multiple_incompatible_tools
#[test]
fn test_multiple_incompatible_tools() {
    let config = config(vec![
        Tool {
            google_search_retrieval: Some(GoogleSearchRetrieval::default()),
            ..Default::default()
        },
        Tool {
            retrieval: Some(Retrieval::default()),
            ..Default::default()
        },
        declaration_tool("test_function"),
        Tool {
            computer_use: Some(ComputerUse::default()),
            ..Default::default()
        },
        Tool {
            code_execution: Some(ToolCodeExecution::default()),
            ..Default::default()
        },
        declaration_tool("test_function_2"),
        callable_tool("afc_find_multi_callable"),
        callable_tool("afc_find_multi_adapter"),
    ]);
    assert_eq!(
        find_afc_incompatible_tool_indexes(Some(&config), false),
        vec![2, 5]
    );
}

// upstream-test: afc/test_find_afc_incompatible_tool_indexes.py::test_mcp_tool_incompatible
#[test]
fn test_mcp_tool_incompatible() {
    let config = config(vec![
        Tool {
            google_search_retrieval: Some(GoogleSearchRetrieval::default()),
            ..Default::default()
        },
        Tool {
            retrieval: Some(Retrieval::default()),
            ..Default::default()
        },
        declaration_tool("test_function"),
        Tool {
            code_execution: Some(ToolCodeExecution::default()),
            ..Default::default()
        },
        callable_tool("afc_find_mcp_callable"),
        callable_tool("afc_find_mcp_adapter"),
        Tool {
            mcp_servers: Some(vec![McpServer {
                name: Some("test_mcp_server".to_owned()),
                ..Default::default()
            }]),
            ..Default::default()
        },
    ]);
    assert_eq!(
        find_afc_incompatible_tool_indexes(Some(&config), false),
        vec![2, 6]
    );
}
