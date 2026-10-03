//! Ports of `models/test_count_tokens.py` plain functions (Developer API).

use gemini_genai::{
    Error,
    types::{CountTokensConfig, HttpOptions},
};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use super::common::test_client;
use super::support::{TestResult, error_body, mount_json, received};

const MODEL: &str = "gemini-2.5-flash";
const PROMPT: &str = "Tell me a story in 300 words.";

// upstream-test: models/test_count_tokens.py::test_async
#[tokio::test]
async fn test_async() -> TestResult {
    let server = MockServer::start().await;
    mount_json(&server, "POST", 200, &serde_json::json!({"totalTokens": 7})).await;
    let client = test_client(server.uri());
    let response = client.models().count_tokens(MODEL, PROMPT, None).await?;
    assert_eq!(response.total_tokens, Some(7));
    Ok(())
}

// upstream-test: models/test_count_tokens.py::test_different_model_names
#[tokio::test]
async fn test_different_model_names() -> TestResult {
    let server = MockServer::start().await;
    mount_json(&server, "POST", 200, &serde_json::json!({"totalTokens": 7})).await;
    let client = test_client(server.uri());
    for name in ["gemini-2.5-flash", "models/gemini-2.5-flash"] {
        let response = client.models().count_tokens(name, PROMPT, None).await?;
        assert_eq!(response.total_tokens, Some(7), "model name {name}");
    }
    let paths: Vec<String> = received(&server)
        .await?
        .iter()
        .map(|r| r.url.path().to_owned())
        .collect();
    assert_eq!(
        paths,
        vec![
            "/v1beta/models/gemini-2.5-flash:countTokens".to_owned(),
            "/v1beta/models/gemini-2.5-flash:countTokens".to_owned(),
        ],
        "both spellings resolve to the same endpoint"
    );
    Ok(())
}

// upstream-test: models/test_count_tokens.py::test_extra_body
#[tokio::test]
async fn test_extra_body() -> TestResult {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_json(error_body(400, "INVALID_ARGUMENT")))
        .mount(&server)
        .await;
    let client = test_client(server.uri());
    let extra: serde_json::Map<String, serde_json::Value> = serde_json::from_value(
        serde_json::json!({"systemInstruction": {"parts": [{"text": "you are a chatbot."}], "role": "user"}}),
    )?;
    let config = CountTokensConfig {
        http_options: Some(HttpOptions {
            extra_body: Some(extra),
            ..Default::default()
        }),
        ..Default::default()
    };
    let result = client
        .models()
        .count_tokens(MODEL, PROMPT, Some(config))
        .await;
    let Err(Error::Api(api)) = result else {
        return Err(format!("expected a 4xx API error, got {result:?}").into());
    };
    assert_eq!(api.code, 400);
    let body: serde_json::Value = received(&server).await?[0].body_json()?;
    assert_eq!(
        body["systemInstruction"]["parts"][0]["text"], "you are a chatbot.",
        "extra_body is merged into the request body: {body}"
    );
    Ok(())
}
