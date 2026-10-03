//! Ports of the plain (non-table) tests in `models/test_generate_content.py`.
//!
//! Python replays recorded HTTP exchanges; these tests serve the same
//! scenarios from `wiremock` and assert on the request the crate sends and on
//! the typed response. Python's "sync" and "async" variants share the one
//! async Rust API. Tests whose subject is a pydantic model, a Python `typing`
//! construct, a Python callable or Python logging are excluded in
//! `tools/codegen/upstream_tests.toml` instead.
#![expect(
    clippy::unwrap_used,
    reason = "test helpers: a malformed mock, body or config literal here is a test-setup bug"
)]

use base64::Engine as _;
use futures_util::StreamExt;
use gemini_genai::{
    Error,
    types::{GenerateContentConfig, GenerateContentResponse, Part, Schema},
};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method},
};

use crate::common::test_client;

const GEMINI_FLASH_LATEST: &str = "gemini-2.5-flash";
const GEMINI_FLASH_2_0: &str = "gemini-2.0-flash-001";
const GEMINI_FLASH_IMAGE_LATEST: &str = "gemini-2.5-flash-image";
const STORY_PROMPT: &str = "Tell me a story in 300 words.";
const RESPONSE_HEADER: &str = "x-test-response";

fn text_reply(text: &str) -> Value {
    json!({"candidates": [{"content": {"role": "model", "parts": [{"text": text}]}, "finishReason": "STOP"}]})
}

/// One SSE event for `value`.
fn event(value: &Value) -> String {
    format!("data: {value}\n\n")
}

/// A `generateContent` mock answering `reply`, with a response header set.
async fn unary_server(reply: &Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(reply)
                .insert_header(RESPONSE_HEADER, "yes"),
        )
        .expect(1)
        .mount(&server)
        .await;
    server
}

/// A `streamGenerateContent` mock answering `chunks` as SSE events.
async fn stream_server(chunks: &[Value]) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(chunks.iter().map(event).collect::<String>())
                .insert_header("content-type", "text/event-stream")
                .insert_header(RESPONSE_HEADER, "yes"),
        )
        .expect(1)
        .mount(&server)
        .await;
    server
}

async fn error_server(status: u16, body: &Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&server)
        .await;
    server
}

fn config(value: Value) -> GenerateContentConfig {
    serde_json::from_value(value).unwrap()
}

async fn request_body(server: &MockServer) -> Value {
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1, "exactly one request expected");
    requests[0].body_json().unwrap()
}

async fn request_url_path(server: &MockServer) -> String {
    server.received_requests().await.unwrap()[0]
        .url
        .path()
        .to_owned()
}

/// Drains a stream, returning every chunk (any error fails the test).
async fn collect_chunks(
    server: &MockServer,
    model: &str,
    contents: &str,
    cfg: Option<GenerateContentConfig>,
) -> Vec<GenerateContentResponse> {
    test_client(server.uri())
        .models()
        .generate_content_stream(model, contents, cfg)
        .await
        .unwrap()
        .map(|item| item.unwrap())
        .collect()
        .await
}

fn http_options_config() -> GenerateContentConfig {
    config(json!({"http_options": {"api_version": "v1", "headers": {"test": "headers"}}}))
}

fn has_text_or_finish(chunk: &GenerateContentResponse) -> bool {
    chunk.text().is_some()
        || chunk
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .is_some_and(|c| c.finish_reason.is_some())
}

fn stream_chunks() -> Vec<Value> {
    vec![
        json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "Once "}]}}]}),
        json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "upon a time"}]}, "finishReason": "STOP"}]}),
    ]
}

// upstream-test: models/test_generate_content.py::test_sync_with_headers
#[tokio::test]
async fn test_sync_with_headers() {
    let server = unary_server(&text_reply("story")).await;
    let response = test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, STORY_PROMPT, None)
        .await
        .unwrap();
    let http = response.sdk_http_response.unwrap();
    assert!(http.headers.unwrap().contains_key(RESPONSE_HEADER));
    assert!(http.body.is_none());
}

// upstream-test: models/test_generate_content.py::test_sync_with_full_response
#[tokio::test]
async fn test_sync_with_full_response() {
    let server = unary_server(&text_reply("story")).await;
    let cfg = config(json!({"should_return_http_response": true}));
    let response = test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, STORY_PROMPT, Some(cfg))
        .await
        .unwrap();
    let http = response.sdk_http_response.unwrap();
    assert!(http.headers.unwrap().contains_key(RESPONSE_HEADER));
    let body = http.body.unwrap();
    assert!(body.contains("candidates"));
    assert!(body.contains("content"));
    assert!(body.contains("parts"));
    // The parsed fields are absent when the raw response is requested.
    assert!(response.candidates.is_none());
}

// upstream-test: models/test_generate_content.py::test_async
#[tokio::test]
async fn test_async() {
    let server = unary_server(&text_reply("story")).await;
    let response = test_client(server.uri())
        .models()
        .generate_content(
            GEMINI_FLASH_LATEST,
            STORY_PROMPT,
            Some(http_options_config()),
        )
        .await
        .unwrap();
    assert_eq!(response.text().as_deref(), Some("story"));
    // Per-request http_options: api_version and headers reach the wire.
    assert_eq!(
        request_url_path(&server).await,
        "/v1/models/gemini-2.5-flash:generateContent"
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[0].headers.get("test").unwrap(), "headers");
}

// upstream-test: models/test_generate_content.py::test_async_with_headers
#[tokio::test]
async fn test_async_with_headers() {
    let server = unary_server(&text_reply("story")).await;
    let response = test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, STORY_PROMPT, None)
        .await
        .unwrap();
    let http = response.sdk_http_response.unwrap();
    assert!(http.headers.unwrap().contains_key(RESPONSE_HEADER));
    assert!(http.body.is_none());
}

// upstream-test: models/test_generate_content.py::test_async_with_full_response
#[tokio::test]
async fn test_async_with_full_response() {
    let server = unary_server(&text_reply("story")).await;
    let cfg = config(json!({"should_return_http_response": true}));
    let response = test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, STORY_PROMPT, Some(cfg))
        .await
        .unwrap();
    let http = response.sdk_http_response.unwrap();
    assert!(http.headers.is_some());
    let body = http.body.unwrap();
    for needle in ["candidates", "content", "parts"] {
        assert!(body.contains(needle), "body lacks {needle}");
    }
}

// upstream-test: models/test_generate_content.py::test_sync_stream
#[tokio::test]
async fn test_sync_stream() {
    let server = stream_server(&stream_chunks()).await;
    let chunks = collect_chunks(
        &server,
        GEMINI_FLASH_LATEST,
        STORY_PROMPT,
        Some(http_options_config()),
    )
    .await;
    assert!(!chunks.is_empty());
    assert!(chunks.iter().all(has_text_or_finish));
    assert_eq!(
        request_url_path(&server).await,
        "/v1/models/gemini-2.5-flash:streamGenerateContent"
    );
}

// upstream-test: models/test_generate_content.py::test_sync_stream_with_should_return_http_headers
#[tokio::test]
async fn test_sync_stream_with_should_return_http_headers() {
    let server = stream_server(&stream_chunks()).await;
    let chunks = collect_chunks(
        &server,
        GEMINI_FLASH_LATEST,
        STORY_PROMPT,
        Some(http_options_config()),
    )
    .await;
    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert!(has_text_or_finish(chunk));
        let headers = chunk.sdk_http_response.as_ref().unwrap().headers.as_ref();
        assert!(headers.unwrap().contains_key(RESPONSE_HEADER));
    }
}

/// Image-then-text chunks followed by a finishing chunk, as an image model streams.
fn image_chunks() -> Vec<Value> {
    vec![
        json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "Here it is."}]}}]}),
        json!({"candidates": [{"content": {"role": "model", "parts": [
            {"inlineData": {"mimeType": "image/png", "data": "iVBORw0KGgo="}}]}}]}),
        json!({"candidates": [{"finishReason": "STOP"}]}),
    ]
}

async fn assert_non_text_modality_stream(model: &str) {
    let server = stream_server(&image_chunks()).await;
    let cfg = config(json!({"response_modalities": ["IMAGE", "TEXT"]}));
    let chunks = collect_chunks(
        &server,
        model,
        "Generate an image of the Eiffel tower with fireworks in the background.",
        Some(cfg),
    )
    .await;
    assert!(!chunks.is_empty());
    let mut saw_inline_data = false;
    for chunk in &chunks {
        let candidate = &chunk.candidates.as_ref().unwrap()[0];
        if candidate.finish_reason.is_some() {
            continue;
        }
        for part in chunk.parts().unwrap_or_default() {
            assert!(part.text.is_some() || part.inline_data.is_some());
            saw_inline_data |= part.inline_data.is_some();
        }
    }
    assert!(saw_inline_data);
    assert_eq!(
        request_body(&server).await["generationConfig"]["responseModalities"],
        json!(["IMAGE", "TEXT"])
    );
}

// upstream-test: models/test_generate_content.py::test_sync_stream_with_non_text_modality
#[tokio::test]
async fn test_sync_stream_with_non_text_modality() {
    assert_non_text_modality_stream("gemini-2.0-flash-preview-image-generation").await;
}

// upstream-test: models/test_generate_content.py::test_async_stream
#[tokio::test]
async fn test_async_stream() {
    let server = stream_server(&stream_chunks()).await;
    let chunks = collect_chunks(
        &server,
        GEMINI_FLASH_LATEST,
        STORY_PROMPT,
        Some(http_options_config()),
    )
    .await;
    assert_eq!(chunks.len(), 2);
    assert!(chunks.iter().all(has_text_or_finish));
}

// upstream-test: models/test_generate_content.py::test_async_stream_with_headers
#[tokio::test]
async fn test_async_stream_with_headers() {
    let server = stream_server(&stream_chunks()).await;
    let chunks = collect_chunks(
        &server,
        GEMINI_FLASH_LATEST,
        STORY_PROMPT,
        Some(http_options_config()),
    )
    .await;
    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert!(has_text_or_finish(chunk));
        let headers = chunk.sdk_http_response.as_ref().unwrap().headers.as_ref();
        assert!(headers.unwrap().contains_key(RESPONSE_HEADER));
    }
}

// upstream-test: models/test_generate_content.py::test_async_stream_with_non_text_modality
#[tokio::test]
async fn test_async_stream_with_non_text_modality() {
    assert_non_text_modality_stream(GEMINI_FLASH_IMAGE_LATEST).await;
}

fn shared_generation_config(max_output_tokens: i64) -> GenerateContentConfig {
    config(json!({
        "max_output_tokens": max_output_tokens,
        "top_k": 2,
        "temperature": 0.5,
        "top_p": 0.5,
        "response_mime_type": "application/json",
        "stop_sequences": ["\n"],
        "seed": 42,
    }))
}

fn assert_shared_generation_config(body: &Value, max_output_tokens: i64) {
    let generation_config = &body["generationConfig"];
    assert_eq!(generation_config["maxOutputTokens"], max_output_tokens);
    assert_eq!(generation_config["topK"].as_f64(), Some(2.0));
    assert_eq!(generation_config["temperature"], 0.5);
    assert_eq!(generation_config["topP"], 0.5);
    assert_eq!(generation_config["responseMimeType"], "application/json");
    assert_eq!(generation_config["stopSequences"], json!(["\n"]));
    assert_eq!(generation_config["seed"], 42);
}

// upstream-test: models/test_generate_content.py::test_simple_shared_generation_config_stream
#[tokio::test]
async fn test_simple_shared_generation_config_stream() {
    let server = stream_server(&stream_chunks()).await;
    let chunks = collect_chunks(
        &server,
        GEMINI_FLASH_LATEST,
        "tell me a story in 300 words",
        Some(shared_generation_config(1000)),
    )
    .await;
    assert!(!chunks.is_empty());
    assert!(chunks.iter().all(has_text_or_finish));
    assert_shared_generation_config(&request_body(&server).await, 1000);
}

// upstream-test: models/test_generate_content.py::test_simple_shared_generation_config_async
#[tokio::test]
async fn test_simple_shared_generation_config_async() {
    let server = unary_server(&text_reply("{}")).await;
    test_client(server.uri())
        .models()
        .generate_content(
            GEMINI_FLASH_LATEST,
            "tell me a story in 300 words",
            Some(shared_generation_config(4000)),
        )
        .await
        .unwrap();
    assert_shared_generation_config(&request_body(&server).await, 4000);
}

// upstream-test: models/test_generate_content.py::test_simple_shared_generation_config_stream_async
#[tokio::test]
async fn test_simple_shared_generation_config_stream_async() {
    let server = stream_server(&stream_chunks()).await;
    let chunks = collect_chunks(
        &server,
        GEMINI_FLASH_2_0,
        "tell me a story in 300 words",
        Some(shared_generation_config(400)),
    )
    .await;
    assert!(!chunks.is_empty());
    assert!(chunks.iter().all(has_text_or_finish));
    assert_shared_generation_config(&request_body(&server).await, 400);
}

// upstream-test: models/test_generate_content.py::test_log_probs
#[tokio::test]
async fn test_log_probs() {
    let server = unary_server(&text_reply("I am a model")).await;
    let cfg = config(json!({
        "logprobs": 2,
        "presence_penalty": 0.5,
        "frequency_penalty": 0.5,
        "response_logprobs": true,
    }));
    test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_2_0, "What is your name?", Some(cfg))
        .await
        .unwrap();
    let generation_config = &request_body(&server).await["generationConfig"];
    assert_eq!(generation_config["logprobs"], 2);
    assert_eq!(generation_config["presencePenalty"], 0.5);
    assert_eq!(generation_config["frequencyPenalty"], 0.5);
    assert_eq!(generation_config["responseLogprobs"], true);
}

// upstream-test: models/test_generate_content.py::test_simple_config
#[tokio::test]
async fn test_simple_config() {
    let server = unary_server(&text_reply("I am a model")).await;
    let cfg = config(json!({"max_output_tokens": 300, "top_k": 2}));
    let response = test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, "What is your name?", Some(cfg))
        .await
        .unwrap();
    assert!(response.text().is_some());
    let generation_config = &request_body(&server).await["generationConfig"];
    assert_eq!(generation_config["maxOutputTokens"], 300);
    assert_eq!(generation_config["topK"].as_f64(), Some(2.0));
}

// upstream-test: models/test_generate_content.py::test_safety_settings
#[tokio::test]
async fn test_safety_settings() {
    let server = unary_server(&text_reply("I am a model")).await;
    let cfg = config(json!({"safety_settings": [{
        "category": "HARM_CATEGORY_HATE_SPEECH",
        "threshold": "BLOCK_ONLY_HIGH",
    }]}));
    let response = test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, "What is your name?", Some(cfg))
        .await
        .unwrap();
    assert!(response.text().is_some());
    assert_eq!(
        request_body(&server).await["safetySettings"],
        json!([{"category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_ONLY_HIGH"}])
    );
}

/// On the Gemini Developer API a safety setting's `method` is rejected before
/// any request is sent (Python raises `ValueError` mentioning `method`).
async fn assert_safety_method_rejected_on_stream(safety_settings: Value) {
    let server = MockServer::start().await;
    let cfg = config(json!({"safety_settings": safety_settings}));
    let result = test_client(server.uri())
        .models()
        .generate_content_stream(GEMINI_FLASH_LATEST, "What is your name?", Some(cfg))
        .await;
    let Err(error) = result else {
        panic!("a safety setting with `method` must be rejected");
    };
    assert!(
        matches!(
            error,
            Error::UnsupportedByBackend {
                field: "method",
                ..
            }
        ),
        "unexpected error: {error}"
    );
    assert!(error.to_string().contains("method"));
    assert!(server.received_requests().await.unwrap().is_empty());
}

// upstream-test: models/test_generate_content.py::test_safety_settings_on_difference_stream
#[tokio::test]
async fn test_safety_settings_on_difference_stream() {
    assert_safety_method_rejected_on_stream(json!([
        {"category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_ONLY_HIGH", "method": "SEVERITY"},
        {"category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_LOW_AND_ABOVE", "method": "PROBABILITY"},
    ]))
    .await;
}

// upstream-test: models/test_generate_content.py::test_safety_settings_on_difference_stream_with_lower_enum
#[tokio::test]
async fn test_safety_settings_on_difference_stream_with_lower_enum() {
    assert_safety_method_rejected_on_stream(json!([
        {"category": "harm_category_hate_speech", "threshold": "block_only_high", "method": "severity"},
        {"category": "harm_category_dangerous_content", "threshold": "block_low_and_above", "method": "probability"},
    ]))
    .await;
}

fn country_properties(population_key: &str, case: fn(&str) -> String) -> Value {
    json!({
        "name": {"type": case("STRING")},
        population_key: {"type": case("INTEGER")},
        "capital": {"type": case("STRING")},
        "continent": {"type": case("STRING")},
        "gdp": {"type": case("INTEGER")},
        "official_language": {"type": case("STRING")},
        "total_area_sq_mi": {"type": case("INTEGER")},
    })
}

/// Sends `response_schema` and returns the request's `responseSchema`.
async fn response_schema_on_wire(schema: Value) -> Value {
    let server = unary_server(&text_reply("{}")).await;
    let cfg = config(json!({
        "response_mime_type": "application/json",
        "response_schema": schema,
    }));
    test_client(server.uri())
        .models()
        .generate_content(
            GEMINI_FLASH_LATEST,
            "Give me information of the United States.",
            Some(cfg),
        )
        .await
        .unwrap();
    request_body(&server).await["generationConfig"]["responseSchema"].clone()
}

// upstream-test: models/test_generate_content.py::test_json_schema
#[tokio::test]
async fn test_json_schema() {
    let schema = response_schema_on_wire(json!({
        "required": ["name", "population", "capital", "continent", "gdp", "official_language", "total_area_sq_mi"],
        "properties": country_properties("population", str::to_owned),
        "type": "OBJECT",
    }))
    .await;
    assert_eq!(schema["type"], "OBJECT");
    assert_eq!(schema["properties"]["population"]["type"], "INTEGER");
    assert_eq!(schema["properties"]["name"]["type"], "STRING");
    assert_eq!(schema["required"].as_array().unwrap().len(), 7);
}

// upstream-test: models/test_generate_content.py::test_json_schema_with_lower_enum
#[tokio::test]
async fn test_json_schema_with_lower_enum() {
    let schema = response_schema_on_wire(json!({
        "required": ["name", "pupulation", "capital"],
        "properties": country_properties("pupulation", str::to_lowercase),
        "type": "OBJECT",
    }))
    .await;
    // Lower-case type names are accepted and normalised to the canonical enum.
    assert_eq!(schema["type"], "OBJECT");
    assert_eq!(schema["properties"]["pupulation"]["type"], "INTEGER");
    assert_eq!(schema["properties"]["capital"]["type"], "STRING");
}

fn fruit_basket_schema() -> Value {
    json!({
        "type": "OBJECT",
        "title": "Fruit Basket",
        "description": "A structured representation of a fruit basket",
        "required": ["fruit"],
        "properties": {"fruit": {
            "type": "ARRAY",
            "description": "An ordered list of the fruit in the basket",
            "items": {
                "description": "A piece of fruit",
                "any_of": [
                    {
                        "title": "Apple",
                        "description": "Describes an apple",
                        "type": "OBJECT",
                        "properties": {
                            "type": {"type": "STRING", "description": "Always 'apple'"},
                            "color": {"type": "STRING", "description": "The color of the apple (e.g., 'red')"},
                        },
                        "property_ordering": ["type", "color"],
                        "required": ["type", "color"],
                    },
                    {
                        "title": "Orange",
                        "description": "Describes an orange",
                        "type": "OBJECT",
                        "properties": {
                            "type": {"type": "STRING", "description": "Always 'orange'"},
                            "size": {"type": "STRING", "description": "The size of the orange (e.g., 'medium')"},
                        },
                        "property_ordering": ["type", "size"],
                        "required": ["type", "size"],
                    },
                ],
            },
        }},
    })
}

fn assert_fruit_basket_on_wire(schema: &Value) {
    assert_eq!(schema["title"], "Fruit Basket");
    let items = &schema["properties"]["fruit"]["items"];
    let any_of = items["any_of"].as_array().unwrap();
    assert_eq!(any_of.len(), 2);
    assert_eq!(any_of[0]["title"], "Apple");
    assert_eq!(any_of[1]["properties"]["size"]["type"], "STRING");
    assert_eq!(any_of[1]["property_ordering"], json!(["type", "size"]));
}

// upstream-test: models/test_generate_content.py::test_json_schema_with_any_of
#[tokio::test]
async fn test_json_schema_with_any_of() {
    assert_fruit_basket_on_wire(&response_schema_on_wire(fruit_basket_schema()).await);
}

// upstream-test: models/test_generate_content.py::test_schema_with_any_of
#[tokio::test]
async fn test_schema_with_any_of() {
    // The same schema built from typed `Schema` values rather than a dict.
    let schema: Schema = serde_json::from_value(fruit_basket_schema()).unwrap();
    let server = unary_server(&text_reply("{}")).await;
    let cfg = GenerateContentConfig {
        response_mime_type: Some("application/json".to_owned()),
        response_schema: Some(schema),
        ..Default::default()
    };
    test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, "Give me a fruit basket.", Some(cfg))
        .await
        .unwrap();
    assert_fruit_basket_on_wire(&request_body(&server).await["generationConfig"]["responseSchema"]);
}

// upstream-test: models/test_generate_content.py::test_json_schema_with_streaming
#[tokio::test]
async fn test_json_schema_with_streaming() {
    let server = stream_server(&stream_chunks()).await;
    let cfg = config(json!({
        "response_mime_type": "application/json",
        "response_schema": {
            "properties": country_properties("population", str::to_owned),
            "type": "OBJECT",
        },
    }));
    let chunks = collect_chunks(
        &server,
        GEMINI_FLASH_LATEST,
        "Give me information of the United States.",
        Some(cfg),
    )
    .await;
    for chunk in &chunks {
        for part in chunk.parts().unwrap_or_default() {
            assert!(part.text.is_some());
        }
    }
    let body = request_body(&server).await;
    assert_eq!(
        body["generationConfig"]["responseSchema"]["properties"]["gdp"]["type"],
        "INTEGER"
    );
}

/// A JSON Schema of the shape pydantic's `model_json_schema()` emits for
/// `class Foo(BaseModel): bar: str; baz: int; qux: list[str]`.
fn foo_model_schema() -> Value {
    json!({
        "properties": {
            "bar": {"title": "Bar", "type": "string"},
            "baz": {"title": "Baz", "type": "integer"},
            "qux": {"items": {"type": "string"}, "title": "Qux", "type": "array"},
        },
        "required": ["bar", "baz", "qux"],
        "title": "Foo",
        "type": "object",
    })
}

fn assert_foo_schema_on_wire(schema: &Value) {
    assert_eq!(schema["type"], "OBJECT");
    assert_eq!(schema["properties"]["bar"]["type"], "STRING");
    assert_eq!(schema["properties"]["baz"]["type"], "INTEGER");
    assert_eq!(schema["properties"]["qux"]["type"], "ARRAY");
    assert_eq!(schema["properties"]["qux"]["items"]["type"], "STRING");
    assert_eq!(schema["required"], json!(["bar", "baz", "qux"]));
}

// upstream-test: models/test_generate_content.py::test_pydantic_schema_from_json
#[tokio::test]
async fn test_pydantic_schema_from_json() {
    assert_foo_schema_on_wire(&response_schema_on_wire(foo_model_schema()).await);
}

// upstream-test: models/test_generate_content.py::test_schema_from_json
#[tokio::test]
async fn test_schema_from_json() {
    let schema: Schema = serde_json::from_value(foo_model_schema()).unwrap();
    let server = unary_server(&text_reply("{}")).await;
    let cfg = GenerateContentConfig {
        response_mime_type: Some("application/json".to_owned()),
        response_schema: Some(schema),
        ..Default::default()
    };
    let response = test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, "Fill in the Foo.", Some(cfg))
        .await
        .unwrap();
    assert!(response.text().is_some());
    assert_foo_schema_on_wire(&request_body(&server).await["generationConfig"]["responseSchema"]);
}

// upstream-test: models/test_generate_content.py::test_schema_from_model_schema
#[tokio::test]
async fn test_schema_from_model_schema() {
    // A raw JSON-Schema dict as `response_json_schema` goes out untouched.
    let server = unary_server(&text_reply("{}")).await;
    let cfg = config(json!({
        "response_mime_type": "application/json",
        "response_json_schema": foo_model_schema(),
    }));
    test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, "Fill in the Foo.", Some(cfg))
        .await
        .unwrap();
    assert_eq!(
        request_body(&server).await["generationConfig"]["responseJsonSchema"],
        foo_model_schema()
    );
}

// upstream-test: models/test_generate_content.py::test_schema_with_additional_properties
#[tokio::test]
async fn test_schema_with_additional_properties() {
    let server = MockServer::start().await;
    let cfg = config(json!({
        "response_mime_type": "application/json",
        "response_schema": {
            "type": "OBJECT",
            "properties": {
                "bar": {"type": "STRING"},
                "qux": {"type": "OBJECT", "additional_properties": {"type": "STRING"}},
            },
        },
    }));
    let error = test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, "What is your name?", Some(cfg))
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains(
            "additionalProperties is only supported in Gemini Enterprise Agent Platform mode"
        ),
        "unexpected error: {error}"
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

// upstream-test: models/test_generate_content.py::test_replicated_voice_config
#[tokio::test]
async fn test_replicated_voice_config() {
    let sample = b"RIFF....WAVEfmt ";
    let sample_b64 = base64::engine::general_purpose::STANDARD.encode(sample);
    let server = unary_server(&text_reply("Cheese")).await;
    let cfg = config(json!({
        "response_modalities": ["audio"],
        "speech_config": {"voice_config": {"replicated_voice_config": {
            "voice_sample_audio": sample_b64,
            "mime_type": "audio/wav",
        }}},
    }));
    test_client(server.uri())
        .models()
        .generate_content(
            "gemini-2.5-flash-preview-tts-voice-replication-rev22-2025-10-28",
            "Produce a speech response saying \"Cheese\"",
            Some(cfg),
        )
        .await
        .unwrap();
    let body = request_body(&server).await;
    let replicated =
        &body["generationConfig"]["speechConfig"]["voice_config"]["replicated_voice_config"];
    assert_eq!(replicated["voice_sample_audio"], sample_b64);
    assert_eq!(replicated["mime_type"], "audio/wav");
}

// upstream-test: models/test_generate_content.py::test_catch_stack_trace_in_error_handling
#[tokio::test]
async fn test_catch_stack_trace_in_error_handling() {
    let server = error_server(
        400,
        &json!({"error": {
            "code": 400,
            "message": "Multi-modal output is not supported.",
            "status": "INVALID_ARGUMENT",
            "details": [{
                "@type": "type.googleapis.com/google.rpc.DebugInfo",
                "detail": "[ORIGINAL ERROR] generic::invalid_argument: Multi-modal output is not supported.",
            }],
        }}),
    )
    .await;
    let cfg = config(json!({"response_modalities": ["AUDIO"]}));
    let error = test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, "What is your name?", Some(cfg))
        .await
        .unwrap_err();
    let Error::Api(api) = error else {
        panic!("expected an API error, got {error}");
    };
    assert_eq!(api.code, 400);
    assert_eq!(api.status.as_deref(), Some("INVALID_ARGUMENT"));
    assert_eq!(api.details.len(), 1);
}

/// The wire contents of one request must be a single user turn holding one part per input text.
fn assert_single_user_turn_with_texts(body: &Value, expected: &[&str]) {
    let contents = body["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0]["role"], "user");
    let texts: Vec<&str> = contents[0]["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, expected);
}

const SHAKESPEARE: &str = "Summarize Shakespeare's life work in a few sentences";
const HEMINGWAY: &str = "Summarize Hemingway's life work";

// upstream-test: models/test_generate_content.py::test_multiple_strings
#[tokio::test]
async fn test_multiple_strings() {
    // Python's list of plain strings becomes parts of one user turn; the
    // pydantic `list[SummaryResponses]` schema/`parsed` half is Python-only.
    let server = unary_server(&text_reply(
        r#"[{"summary": "Shakespeare wrote plays", "person": "Shakespeare"}, {"summary": "Hemingway wrote novels", "person": "Hemingway"}]"#,
    ))
    .await;
    let contents = vec![Part::from_text(SHAKESPEARE), Part::from_text(HEMINGWAY)];
    let response = test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, contents, Some(foo_list_config()))
        .await
        .unwrap();
    let text = response.text().unwrap();
    assert!(text.contains("Shakespeare") && text.contains("Hemingway"));
    assert_single_user_turn_with_texts(&request_body(&server).await, &[SHAKESPEARE, HEMINGWAY]);
}

fn foo_list_config() -> GenerateContentConfig {
    config(json!({
        "response_mime_type": "application/json",
        "response_schema": {"type": "ARRAY", "items": {
            "type": "OBJECT",
            "properties": {"summary": {"type": "STRING"}, "person": {"type": "STRING"}},
            "required": ["summary", "person"],
        }},
    }))
}

// upstream-test: models/test_generate_content.py::test_multiple_parts
#[tokio::test]
async fn test_multiple_parts() {
    let server = unary_server(&text_reply(
        r#"[{"summary": "s", "person": "Shakespeare"}, {"summary": "h", "person": "Hemingway"}]"#,
    ))
    .await;
    let contents = vec![Part::from_text(SHAKESPEARE), Part::from_text(HEMINGWAY)];
    let response = test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, contents, Some(foo_list_config()))
        .await
        .unwrap();
    let parsed: Value = serde_json::from_str(&response.text().unwrap()).unwrap();
    assert!(
        parsed[0]["person"]
            .as_str()
            .unwrap()
            .contains("Shakespeare")
    );
    assert!(parsed[1]["person"].as_str().unwrap().contains("Hemingway"));
    assert_single_user_turn_with_texts(&request_body(&server).await, &[SHAKESPEARE, HEMINGWAY]);
}

fn function_declaration(name: &str, description: &str, argument: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "parameters": {
            "type": "OBJECT",
            "properties": {argument: {"type": "STRING"}},
        },
    })
}

// upstream-test: models/test_generate_content.py::test_multiple_function_calls
#[tokio::test]
async fn test_multiple_function_calls() {
    let server = unary_server(&text_reply(
        "It is sunny and 100 degrees in Boston; GOOG is $100.",
    ))
    .await;
    let contents = vec![
        Part::from_text("What is the weather in Boston?"),
        Part::from_text("What is the stock price of GOOG?"),
        Part::from_function_call(
            "get_weather",
            [("location".to_owned(), json!("Boston"))].into(),
        ),
        Part::from_function_call(
            "get_stock_price",
            [("symbol".to_owned(), json!("GOOG"))].into(),
        ),
        Part::from_function_response(
            "get_weather",
            [("response".to_owned(), json!("It is sunny and 100 degrees."))].into(),
        ),
        Part::from_function_response(
            "get_stock_price",
            [("response".to_owned(), json!("The stock price is $100."))].into(),
        ),
    ];
    let cfg = config(json!({"tools": [{"function_declarations": [
        function_declaration("get_weather", "Get the weather in a city.", "location"),
        function_declaration("get_stock_price", "Get the stock price of a symbol.", "symbol"),
    ]}]}));
    let response = test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, contents, Some(cfg))
        .await
        .unwrap();
    let text = response.text().unwrap();
    assert!(text.contains("Boston") && text.contains("sunny"));
    assert!(text.contains("100 degrees") && text.contains("$100"));

    // user text / model calls / user responses: role alternation of t_contents.
    let body = request_body(&server).await;
    let contents = body["contents"].as_array().unwrap();
    let roles: Vec<&str> = contents
        .iter()
        .map(|c| c["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["user", "model", "user"]);
    assert_eq!(contents[0]["parts"].as_array().unwrap().len(), 2);
    assert_eq!(
        contents[1]["parts"][0]["functionCall"]["name"],
        "get_weather"
    );
    assert_eq!(
        contents[1]["parts"][1]["functionCall"]["args"]["symbol"],
        "GOOG"
    );
    assert_eq!(
        contents[2]["parts"][1]["functionResponse"]["name"],
        "get_stock_price"
    );
    let tools = body["tools"][0]["functionDeclarations"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
}

// upstream-test: models/test_generate_content.py::test_usage_metadata_part_types
#[tokio::test]
async fn test_usage_metadata_part_types() {
    let mut reply = text_reply("A logo.");
    reply["usageMetadata"] = json!({
        "promptTokenCount": 267,
        "candidatesTokenCount": 3,
        "totalTokenCount": 270,
        "promptTokensDetails": [
            {"modality": "IMAGE", "tokenCount": 258},
            {"modality": "TEXT", "tokenCount": 9},
        ],
        "candidatesTokensDetails": [{"modality": "TEXT", "tokenCount": 3}],
    });
    let server = unary_server(&reply).await;
    let contents = vec![
        Part::from_text("Hello world."),
        Part::from_bytes(vec![0x89, b'P', b'N', b'G'], "image/png"),
    ];
    let response = test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_2_0, contents, None)
        .await
        .unwrap();
    let usage = response.usage_metadata.unwrap();
    assert!(usage.candidates_token_count.unwrap() > 0);
    assert!(usage.prompt_token_count.unwrap() > 0);
    let modalities = |details: &Option<Vec<gemini_genai::types::ModalityTokenCount>>| {
        let mut names: Vec<String> = details
            .as_ref()
            .unwrap()
            .iter()
            .map(|d| String::from(d.modality.clone().unwrap()))
            .collect();
        names.sort();
        names
    };
    assert_eq!(modalities(&usage.candidates_tokens_details), ["TEXT"]);
    assert_eq!(modalities(&usage.prompt_tokens_details), ["IMAGE", "TEXT"]);
}

fn developer_instruction_error() -> Value {
    json!({"error": {
        "code": 400,
        "message": "Developer instruction is not enabled for models/gemini-2.5-flash-image",
        "status": "INVALID_ARGUMENT",
    }})
}

fn image_edit_contents() -> Vec<gemini_genai::types::Content> {
    vec![gemini_genai::types::Content {
        role: Some("user".to_owned()),
        parts: Some(vec![
            Part::from_bytes(vec![0x89, b'P', b'N', b'G'], "image/png"),
            Part::from_text("Make sky more beautiful."),
        ]),
    }]
}

fn image_edit_config() -> GenerateContentConfig {
    config(json!({
        "response_mime_type": "text/plain",
        "response_modalities": ["IMAGE", "TEXT"],
        "system_instruction": {"parts": [{"text": "make the sky more beautiful."}]},
    }))
}

fn assert_developer_instruction_error(error: Error) {
    let Error::Api(api) = error else {
        panic!("expected an API error, got {error}");
    };
    assert_eq!(api.code, 400);
    assert_eq!(
        api.message,
        "Developer instruction is not enabled for models/gemini-2.5-flash-image"
    );
}

async fn assert_unary_developer_instruction_error() {
    let server = error_server(400, &developer_instruction_error()).await;
    let error = test_client(server.uri())
        .models()
        .generate_content(
            GEMINI_FLASH_IMAGE_LATEST,
            image_edit_contents(),
            Some(image_edit_config()),
        )
        .await
        .unwrap_err();
    assert_developer_instruction_error(error);
}

async fn assert_stream_developer_instruction_error() {
    let server = error_server(400, &developer_instruction_error()).await;
    let result = test_client(server.uri())
        .models()
        .generate_content_stream(
            GEMINI_FLASH_IMAGE_LATEST,
            image_edit_contents(),
            Some(image_edit_config()),
        )
        .await;
    let Err(error) = result else {
        panic!("a 400 on the stream request must be an error");
    };
    assert_developer_instruction_error(error);
}

// upstream-test: models/test_generate_content.py::test_error_handling_stream
#[tokio::test]
async fn test_error_handling_stream() {
    assert_stream_developer_instruction_error().await;
}

// upstream-test: models/test_generate_content.py::test_error_handling_unary
#[tokio::test]
async fn test_error_handling_unary() {
    assert_unary_developer_instruction_error().await;
}

// upstream-test: models/test_generate_content.py::test_error_handling_unary_async
#[tokio::test]
async fn test_error_handling_unary_async() {
    assert_unary_developer_instruction_error().await;
}

// upstream-test: models/test_generate_content.py::test_error_handling_stream_async
#[tokio::test]
async fn test_error_handling_stream_async() {
    assert_stream_developer_instruction_error().await;
}

// upstream-test: models/test_generate_content.py::test_provisioned_output_dedicated
#[tokio::test]
async fn test_provisioned_output_dedicated() {
    let mut reply = text_reply("2");
    reply["usageMetadata"] = json!({"promptTokenCount": 8, "candidatesTokenCount": 1});
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("X-Vertex-AI-LLM-Request-Type", "dedicated"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&reply))
        .expect(1)
        .mount(&server)
        .await;
    let cfg =
        config(json!({"http_options": {"headers": {"X-Vertex-AI-LLM-Request-Type": "dedicated"}}}));
    let response = test_client(server.uri())
        .models()
        .generate_content(GEMINI_FLASH_LATEST, "What is 1 + 1?", Some(cfg))
        .await
        .unwrap();
    // On the Gemini Developer API no traffic type is reported.
    assert!(response.usage_metadata.unwrap().traffic_type.is_none());
    server.verify().await;
}

// upstream-test: models/test_generate_content.py::test_response_json_schema_with_one_of
#[tokio::test]
async fn test_response_json_schema_with_one_of() {
    let schema_with_one_of = json!({
        "type": "object",
        "properties": {"resource_config": {"oneOf": [
            {"type": "object", "properties": {"size": {"type": "integer"}}, "required": ["size"]},
            {"type": "object", "properties": {"tier": {"type": "string"}}, "required": ["tier"]},
        ]}},
    });
    let server = unary_server(&text_reply(r#"{"resource_config": {"size": 10}}"#)).await;
    let cfg = config(json!({
        "response_mime_type": "application/json",
        "response_json_schema": schema_with_one_of,
    }));
    let response = test_client(server.uri())
        .models()
        .generate_content(
            GEMINI_FLASH_LATEST,
            "Generate a configuration for a resource with size 10.",
            Some(cfg),
        )
        .await
        .unwrap();
    // `response_json_schema` reaches the wire verbatim, `oneOf` included.
    assert_eq!(
        request_body(&server).await["generationConfig"]["responseJsonSchema"],
        schema_with_one_of
    );
    let parsed: Value = serde_json::from_str(&response.text().unwrap()).unwrap();
    assert_eq!(parsed["resource_config"], json!({"size": 10}));
}

// upstream-test: models/test_generate_content.py::test_audio_wav_input
#[tokio::test]
async fn test_audio_wav_input() {
    let server = unary_server(&text_reply("A voice sample.")).await;
    let cfg = config(json!({"audio_transcription_config": {
        "diarization": true,
        "word_timestamp": true,
        "language_auto": {},
    }}));
    let contents = vec![
        Part::from_text("What is this audio about?"),
        Part::from_bytes(b"RIFF....WAVEfmt ".to_vec(), "audio/wav"),
    ];
    let response = test_client(server.uri())
        .models()
        .generate_content("gemini-2.5-flash-preview-tts", contents, Some(cfg))
        .await
        .unwrap();
    assert!(response.text().is_some());
    let body = request_body(&server).await;
    assert_eq!(
        body["contents"][0]["parts"][1]["inlineData"]["mime_type"],
        "audio/wav"
    );
    let transcription = &body["generationConfig"]["audioTranscriptionConfig"];
    assert_eq!(transcription["diarization"], true);
    assert_eq!(transcription["word_timestamp"], true);
}
