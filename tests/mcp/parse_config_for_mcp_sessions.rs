//! Port of upstream `mcp/test_parse_config_for_mcp_sessions.py`: a session
//! that exposes two tools yields two adapters, keyed by tool name.

use serde_json::json;

use super::mcp_to_gemini_tools::tools_for;

// upstream-test: mcp/test_parse_config_for_mcp_sessions.py::test_parse_config_object_with_tools
#[tokio::test]
async fn test_parse_config_object_with_tools() {
    let schema = json!({"type": "object", "properties": {"location": {"type": "string"}}});
    let tools = tools_for(vec![
        (
            "get_weather".to_owned(),
            "Get the weather in a city.".to_owned(),
            schema.clone(),
        ),
        (
            "get_weather_2".to_owned(),
            "Different tool to get the weather.".to_owned(),
            schema,
        ),
    ])
    .await;
    let names: Vec<_> = tools
        .iter()
        .flat_map(|tool| tool.function_declarations.as_deref().unwrap_or_default())
        .filter_map(|declaration| declaration.name.as_deref())
        .collect();
    assert_eq!(names, ["get_weather", "get_weather_2"]);
}
