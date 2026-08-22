//! Integration tests for `client.models().generate_content(...)`, covering
//! multimodal input (US2, T044) and structured output (US2, T045). Runs
//! against the public API only, via `wiremock`, mirroring
//! `tests/chats.rs`'s conventions (`test_client`/`model_reply` helpers,
//! inspecting the raw request body via `server.received_requests()`).

mod common;

use common::test_client;
use google_genai::types::{Content, GenerateContentConfig, Part, Schema, Type};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn model_reply(text: &str) -> serde_json::Value {
    serde_json::json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{"text": text}]},
            "finishReason": "STOP"
        }]
    })
}

#[expect(
    clippy::unwrap_used,
    reason = "test helper: a mock that never receives a request or a malformed captured body here is a test-setup bug"
)]
async fn first_request_body(server: &MockServer) -> serde_json::Value {
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    requests[0].body_json().unwrap()
}

mod multimodal {
    use base64::Engine as _;

    use super::{
        Content, Mock, MockServer, Part, ResponseTemplate, first_request_body, method, model_reply,
        test_client,
    };

    #[tokio::test]
    async fn inline_bytes_part_sends_base64_inline_data() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("ok")))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        let png_bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let contents = vec![
            Part::from_text("what is in this image?"),
            Part::from_bytes(png_bytes.clone(), "image/png"),
        ];
        client
            .models()
            .generate_content("gemini-2.5-flash", contents, None)
            .await
            .unwrap();

        let body = first_request_body(&server).await;
        let parts = body["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        let inline_data = &parts[1]["inlineData"];
        assert_eq!(inline_data["mimeType"], "image/png");
        let expected_b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
        assert_eq!(inline_data["data"], expected_b64);

        server.verify().await;
    }

    #[tokio::test]
    async fn uri_part_sends_file_data() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("ok")))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        let part = Part::from_uri("gs://bucket/video.mp4", "video/mp4");
        client
            .models()
            .generate_content("gemini-2.5-flash", part, None)
            .await
            .unwrap();

        let body = first_request_body(&server).await;
        let file_data = &body["contents"][0]["parts"][0]["fileData"];
        assert_eq!(file_data["fileUri"], "gs://bucket/video.mp4");
        assert_eq!(file_data["mimeType"], "video/mp4");

        server.verify().await;
    }

    #[tokio::test]
    async fn a_conversation_history_is_sent_unchanged_and_in_order() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("ok")))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        let history: Vec<Content> = vec![
            Content::from("first user turn"),
            Content {
                role: Some("model".to_owned()),
                parts: Some(vec![Part::from_text("first model turn")]),
            },
            Content::from("second user turn"),
        ];
        client
            .models()
            .generate_content("gemini-2.5-flash", history, None)
            .await
            .unwrap();

        let body = first_request_body(&server).await;
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "first user turn");
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(contents[1]["parts"][0]["text"], "first model turn");
        assert_eq!(contents[2]["role"], "user");
        assert_eq!(contents[2]["parts"][0]["text"], "second user turn");

        server.verify().await;
    }

    #[tokio::test]
    async fn video_metadata_on_a_part_is_preserved() {
        use google_genai::types::VideoMetadata;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("ok")))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        let mut part = Part::from_uri("gs://bucket/clip.mp4", "video/mp4");
        part.video_metadata = Some(VideoMetadata {
            start_offset: Some("1.5s".to_owned()),
            end_offset: Some("10s".to_owned()),
            fps: Some(2.0),
        });
        client
            .models()
            .generate_content("gemini-2.5-flash", part, None)
            .await
            .unwrap();

        let body = first_request_body(&server).await;
        let video_metadata = &body["contents"][0]["parts"][0]["videoMetadata"];
        // NOTE: `videoMetadata`'s own sub-fields are NOT camelCased here
        // (`start_offset`, not `startOffset`) -- this is not a bug, it's
        // a faithful port of Python's real behavior. Python's own
        // `_Part_to_mldev` passes the nested `VideoMetadata` value
        // through via a bare `setv(..., getv(from_object,
        // ['video_metadata']))` with no per-field renaming, and the
        // request's final `_common.convert_to_dict()` pass dumps any
        // embedded pydantic submodel with `model_dump(exclude_none=True)`
        // -- deliberately *without* `by_alias=True` -- so Python's own
        // wire request carries the same snake_case sub-fields (verified
        // by directly executing the installed google-genai 2.19.0
        // `_GenerateContentParameters_to_mldev` +
        // `_common.convert_to_dict` pipeline). This presumably works in
        // practice because the Gemini API's protobuf-JSON mapping accepts
        // either the original field name or its lowerCamelCase form on
        // input (https://protobuf.dev/programming-guides/json/) -- only
        // JSON *output* is required to be camelCase.
        assert_eq!(video_metadata["start_offset"], "1.5s");
        assert_eq!(video_metadata["end_offset"], "10s");
        assert_eq!(video_metadata["fps"], 2.0);

        server.verify().await;
    }
}

mod structured_output {
    use super::{
        GenerateContentConfig, Mock, MockServer, ResponseTemplate, Schema, Type,
        first_request_body, method, model_reply, test_client,
    };

    #[derive(Debug, serde::Deserialize, schemars::JsonSchema, PartialEq)]
    struct CountryInfo {
        name: String,
        population: u64,
    }

    /// A hand-built [`Schema`] reaches the wire with its field names left
    /// in `snake_case`.
    ///
    /// This is not an oversight: Python's `t_schema` renames only
    /// `additional_properties`/`any_of`/`prefix_items`/`property_ordering`
    /// via `process_schema`, then re-validates the result back into a
    /// `types.Schema` where those spellings are merely aliases, and
    /// `_common.convert_to_dict` finally dumps *without* `by_alias` --- so
    /// the rename round-trips away and google-genai 2.19.0 emits
    /// `snake_case`. proto3 JSON accepts either spelling on input
    /// (<https://protobuf.dev/programming-guides/json/>), and the live API
    /// was confirmed to answer normally for this exact body.
    ///
    /// The enclosing keys (`generationConfig`, `responseSchema`,
    /// `responseMimeType`) stay camelCase: those are written by the
    /// generated converters through `setv`, not by `t_schema`.
    #[tokio::test]
    async fn a_hand_built_response_schema_keeps_snake_case_field_names() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("{}")))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        let mut properties = std::collections::HashMap::new();
        properties.insert(
            "name".to_owned(),
            Schema {
                r#type: Some(Type::String),
                min_length: Some(1),
                ..Default::default()
            },
        );
        let config = GenerateContentConfig {
            response_mime_type: Some("application/json".to_owned()),
            response_schema: Some(Schema {
                r#type: Some(Type::Object),
                properties: Some(properties),
                ..Default::default()
            }),
            ..Default::default()
        };
        client
            .models()
            .generate_content("gemini-2.5-flash", "give me a country", Some(config))
            .await
            .unwrap();

        let body = first_request_body(&server).await;
        let response_schema = &body["generationConfig"]["responseSchema"];
        assert_eq!(response_schema["type"], "OBJECT");
        assert_eq!(response_schema["properties"]["name"]["type"], "STRING");
        assert_eq!(response_schema["properties"]["name"]["min_length"], 1);
        assert_eq!(
            response_schema["properties"]["name"]["minLength"],
            serde_json::Value::Null,
            "t_schema must not camelCase Schema field names"
        );

        server.verify().await;
    }

    #[tokio::test]
    async fn a_raw_json_schema_value_passes_through_to_response_json_schema() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("{}")))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        let raw_schema = serde_json::json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        });
        let config = GenerateContentConfig {
            response_mime_type: Some("application/json".to_owned()),
            response_json_schema: Some(raw_schema.clone()),
            ..Default::default()
        };
        client
            .models()
            .generate_content("gemini-2.5-flash", "give me a country", Some(config))
            .await
            .unwrap();

        let body = first_request_body(&server).await;
        assert_eq!(body["generationConfig"]["responseJsonSchema"], raw_schema);

        server.verify().await;
    }

    #[tokio::test]
    async fn with_json_schema_of_wires_a_schemars_schema_end_to_end() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("{}")))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        let config = GenerateContentConfig::default().with_json_schema_of::<CountryInfo>();
        assert_eq!(
            config.response_mime_type.as_deref(),
            Some("application/json")
        );
        client
            .models()
            .generate_content("gemini-2.5-flash", "give me a country", Some(config))
            .await
            .unwrap();

        let body = first_request_body(&server).await;
        let expected = schemars::schema_for!(CountryInfo).to_value();
        assert_eq!(body["generationConfig"]["responseJsonSchema"], expected);
        assert_eq!(
            body["generationConfig"]["responseMimeType"],
            "application/json"
        );

        server.verify().await;
    }

    #[tokio::test]
    async fn a_json_response_round_trips_into_a_plain_rust_struct() {
        let server = MockServer::start().await;
        let json_text = r#"{"name":"Wakanda","population":6000000}"#;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(model_reply(json_text)))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        let config = GenerateContentConfig::default().with_json_schema_of::<CountryInfo>();
        let response = client
            .models()
            .generate_content("gemini-2.5-flash", "give me a country", Some(config))
            .await
            .unwrap();

        let text = response.text().unwrap();
        let parsed: CountryInfo = serde_json::from_str(&text).unwrap();
        assert_eq!(
            parsed,
            CountryInfo {
                name: "Wakanda".to_owned(),
                population: 6_000_000
            }
        );

        server.verify().await;
    }
}
