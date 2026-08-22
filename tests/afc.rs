//! Integration tests for automatic function calling (AFC), covering both
//! the low-level building blocks (`Tool` construction,
//! `GenerateContentResponse::function_calls`, `Part::from_function_response`)
//! and the [`gemini_genai::afc`] loop wired into
//! `client.models().generate_content(...)`. Runs against the public API
//! only, via `wiremock`, mirroring `tests/chats.rs`'s conventions
//! (`test_client`/`model_reply` helpers, sequencing responses with
//! `up_to_n_times(1)` mocks mounted in order).

mod common;

use std::collections::HashMap;

use common::test_client;
use gemini_genai::Error;
use gemini_genai::afc::function_tool;
use gemini_genai::error::FunctionCallError;
use gemini_genai::types::{
    AutomaticFunctionCallingConfig, Content, FunctionDeclaration, GenerateContentConfig, Part,
    Schema, Tool, Type,
};
use schemars::JsonSchema;
use serde::Deserialize;
use wiremock::ResponseTemplate;
use wiremock::matchers::{body_partial_json, method};

fn model_reply(text: &str) -> serde_json::Value {
    serde_json::json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{"text": text}]},
            "finishReason": "STOP"
        }]
    })
}

fn function_call_reply(name: &str, args: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{"functionCall": {"name": name, "args": args}}]},
            "finishReason": "STOP"
        }]
    })
}

/// Arguments for the `get_weather` demo tool used throughout this file.
#[derive(Debug, Deserialize, JsonSchema)]
struct WeatherArgs {
    location: String,
}

mod manual_building_blocks {
    use super::{
        Content, FunctionDeclaration, GenerateContentConfig, HashMap, Part, ResponseTemplate,
        Schema, Tool, Type, body_partial_json, function_call_reply, method, model_reply,
        test_client,
    };
    use wiremock::{Mock, MockServer};

    /// Exercises the manual (non-AFC) path end to end: a hand-built `Tool`
    /// declares one function, the request body carries it correctly on the
    /// wire, `response.function_calls()` extracts the model's call, and a
    /// hand-built `Part::from_function_response` round-trips it back to the
    /// model for a final answer -- all without registering a callable, so
    /// `crate::afc`'s loop never engages (mirrors a caller who wants full
    /// manual control over function calling).
    #[tokio::test]
    async fn manual_round_trip_declares_extracts_and_resends_a_function_call() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(body_partial_json(serde_json::json!({
                "tools": [{
                    "functionDeclarations": [{
                        "name": "get_weather",
                        "description": "Gets the weather for a location.",
                        "parameters": {
                            "type": "OBJECT",
                            "properties": {"location": {"type": "STRING"}},
                        },
                    }],
                }],
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(function_call_reply(
                    "get_weather",
                    &serde_json::json!({"location": "NYC"}),
                )),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(serde_json::json!({
                "contents": [
                    {"role": "user", "parts": [{"functionResponse": {"name": "get_weather", "response": {"result": {"tempF": 72}}}}]},
                ],
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("It's 72F in NYC.")))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        let mut properties = HashMap::new();
        properties.insert(
            "location".to_owned(),
            Schema {
                r#type: Some(Type::String),
                ..Default::default()
            },
        );
        let tool = Tool {
            function_declarations: Some(vec![FunctionDeclaration {
                name: Some("get_weather".to_owned()),
                description: Some("Gets the weather for a location.".to_owned()),
                parameters: Some(Schema {
                    r#type: Some(Type::Object),
                    properties: Some(properties),
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let config = GenerateContentConfig {
            tools: Some(vec![tool]),
            ..Default::default()
        };

        let first = client
            .models()
            .generate_content(
                "gemini-2.5-flash",
                "What's the weather in NYC?",
                Some(config),
            )
            .await
            .unwrap();

        // No callable was registered for `get_weather` (this `Tool` was
        // built by hand, not via `Tool::from_function`), so the AFC loop
        // never engaged: the model's function call comes back unconsumed.
        let calls = first.function_calls();
        assert_eq!(calls.len(), 1);
        let call = calls[0];
        assert_eq!(call.name.as_deref(), Some("get_weather"));
        assert_eq!(call.args.as_ref().unwrap().get("location").unwrap(), "NYC");

        let mut response_map = HashMap::new();
        response_map.insert("result".to_owned(), serde_json::json!({"tempF": 72}));
        let response_part = Part::from_function_response("get_weather", response_map);
        let response_content = Content {
            role: Some("user".to_owned()),
            parts: Some(vec![response_part]),
        };

        let second = client
            .models()
            .generate_content("gemini-2.5-flash", vec![response_content], None)
            .await
            .unwrap();
        assert_eq!(second.text().as_deref(), Some("It's 72F in NYC."));

        server.verify().await;
    }
}

mod afc_loop {
    use super::{
        AutomaticFunctionCallingConfig, Error, FunctionCallError, GenerateContentConfig, Tool,
        WeatherArgs, function_call_reply, function_tool, method, model_reply, test_client,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn invokes_the_tool_and_returns_the_final_text_after_two_requests() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(function_call_reply(
                    "afc_default_get_weather",
                    &serde_json::json!({"location": "NYC"}),
                )),
            )
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(model_reply("It's sunny in NYC.")),
            )
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;

        let tool = function_tool::<WeatherArgs, _, _, _>(
            "afc_default_get_weather",
            "Gets the weather for a location.",
            |args: WeatherArgs| async move {
                Ok(serde_json::json!({"tempF": 72, "location": args.location}))
            },
        );
        let config = GenerateContentConfig {
            tools: Some(vec![Tool::from_function(tool)]),
            ..Default::default()
        };

        let client = test_client(server.uri());
        let response = client
            .models()
            .generate_content(
                "gemini-2.5-flash",
                "What's the weather in NYC?",
                Some(config),
            )
            .await
            .unwrap();

        assert_eq!(response.text().as_deref(), Some("It's sunny in NYC."));
        assert!(response.function_calls().is_empty());
        let history = response.automatic_function_calling_history.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role.as_deref(), Some("model"));
        assert_eq!(history[1].role.as_deref(), Some("user"));

        assert_eq!(server.received_requests().await.unwrap().len(), 2);
        server.verify().await;
    }

    #[tokio::test]
    async fn maximum_remote_calls_of_one_stops_after_a_single_round() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(function_call_reply(
                    "afc_max1_get_weather",
                    &serde_json::json!({"location": "NYC"}),
                )),
            )
            .expect(1)
            .mount(&server)
            .await;

        let tool = function_tool::<WeatherArgs, _, _, _>(
            "afc_max1_get_weather",
            "Gets the weather for a location.",
            |args: WeatherArgs| async move { Ok(serde_json::json!({"location": args.location})) },
        );
        let config = GenerateContentConfig {
            tools: Some(vec![Tool::from_function(tool)]),
            automatic_function_calling: Some(AutomaticFunctionCallingConfig {
                maximum_remote_calls: Some(1),
                ..Default::default()
            }),
            ..Default::default()
        };

        let client = test_client(server.uri());
        let response = client
            .models()
            .generate_content(
                "gemini-2.5-flash",
                "What's the weather in NYC?",
                Some(config),
            )
            .await
            .unwrap();

        // Only one remote call was allowed: the tool still ran once (the
        // history records the round), but the loop stopped before sending
        // the tool's result back for a final answer, so the model's
        // original function call comes back unconsumed.
        assert!(!response.function_calls().is_empty());
        assert_eq!(
            response
                .automatic_function_calling_history
                .as_ref()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        server.verify().await;
    }

    #[tokio::test]
    async fn disable_returns_the_raw_function_call_response_without_looping() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(function_call_reply(
                    "afc_disabled_get_weather",
                    &serde_json::json!({"location": "NYC"}),
                )),
            )
            .expect(1)
            .mount(&server)
            .await;

        let tool = function_tool::<WeatherArgs, _, _, _>(
            "afc_disabled_get_weather",
            "Gets the weather for a location.",
            |args: WeatherArgs| async move { Ok(serde_json::json!({"location": args.location})) },
        );
        let config = GenerateContentConfig {
            tools: Some(vec![Tool::from_function(tool)]),
            automatic_function_calling: Some(AutomaticFunctionCallingConfig {
                disable: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };

        let client = test_client(server.uri());
        let response = client
            .models()
            .generate_content(
                "gemini-2.5-flash",
                "What's the weather in NYC?",
                Some(config),
            )
            .await
            .unwrap();

        assert!(!response.function_calls().is_empty());
        assert!(response.automatic_function_calling_history.is_none());
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        server.verify().await;
    }

    #[tokio::test]
    async fn a_tool_callback_error_continues_the_loop_with_an_error_function_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(function_call_reply(
                    "afc_err_get_weather",
                    &serde_json::json!({"location": "NYC"}),
                )),
            )
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("ok, noted.")))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;

        let tool = function_tool::<WeatherArgs, _, _, _>(
            "afc_err_get_weather",
            "Gets the weather for a location.",
            |_args: WeatherArgs| async move {
                Err::<serde_json::Value, _>(Error::Validation("weather service is down".to_owned()))
            },
        );
        let config = GenerateContentConfig {
            tools: Some(vec![Tool::from_function(tool)]),
            ..Default::default()
        };

        let client = test_client(server.uri());
        let response = client
            .models()
            .generate_content(
                "gemini-2.5-flash",
                "What's the weather in NYC?",
                Some(config),
            )
            .await
            .unwrap();

        // The callback's error didn't abort the loop: a second request
        // still happened, and the final response is plain text.
        assert_eq!(response.text().as_deref(), Some("ok, noted."));
        assert_eq!(server.received_requests().await.unwrap().len(), 2);

        let history = response.automatic_function_calling_history.unwrap();
        let response_part = &history[1].parts.as_ref().unwrap()[0];
        let error_field = response_part
            .function_response
            .as_ref()
            .unwrap()
            .response
            .as_ref()
            .unwrap()
            .get("error")
            .unwrap();
        assert!(
            error_field
                .as_str()
                .unwrap()
                .contains("weather service is down")
        );

        server.verify().await;
    }

    #[tokio::test]
    async fn an_unregistered_function_name_aborts_with_unsupported_function_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(function_call_reply(
                    "totally_unknown_fn",
                    &serde_json::json!({}),
                )),
            )
            .expect(1)
            .mount(&server)
            .await;

        let tool = function_tool::<WeatherArgs, _, _, _>(
            "afc_known_only",
            "Gets the weather for a location.",
            |args: WeatherArgs| async move { Ok(serde_json::json!({"location": args.location})) },
        );
        let config = GenerateContentConfig {
            tools: Some(vec![Tool::from_function(tool)]),
            ..Default::default()
        };

        let client = test_client(server.uri());
        let error = client
            .models()
            .generate_content(
                "gemini-2.5-flash",
                "What's the weather in NYC?",
                Some(config),
            )
            .await
            .unwrap_err();

        match error {
            Error::FunctionCall(FunctionCallError::UnsupportedFunction(name)) => {
                assert_eq!(name, "totally_unknown_fn");
            }
            other => panic!("expected Error::FunctionCall(UnsupportedFunction), got {other:?}"),
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        server.verify().await;
    }

    #[tokio::test]
    async fn malformed_function_call_arguments_abort_with_unknown_argument_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(function_call_reply(
                    "afc_bad_args_get_weather",
                    &serde_json::json!({"location": 12345}),
                )),
            )
            .expect(1)
            .mount(&server)
            .await;

        let tool = function_tool::<WeatherArgs, _, _, _>(
            "afc_bad_args_get_weather",
            "Gets the weather for a location.",
            |args: WeatherArgs| async move { Ok(serde_json::json!({"location": args.location})) },
        );
        let config = GenerateContentConfig {
            tools: Some(vec![Tool::from_function(tool)]),
            ..Default::default()
        };

        let client = test_client(server.uri());
        let error = client
            .models()
            .generate_content(
                "gemini-2.5-flash",
                "What's the weather in NYC?",
                Some(config),
            )
            .await
            .unwrap_err();

        match error {
            Error::FunctionCall(FunctionCallError::UnknownArgument { function, .. }) => {
                assert_eq!(function, "afc_bad_args_get_weather");
            }
            other => panic!("expected Error::FunctionCall(UnknownArgument), got {other:?}"),
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        server.verify().await;
    }
}
