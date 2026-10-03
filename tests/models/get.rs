//! Ports of `models/test_get.py` plain (async) functions, Developer API.

use std::collections::HashMap;

use gemini_genai::{
    Error,
    types::{GetModelConfig, HttpOptions},
};

use wiremock::MockServer;

use super::common::test_client;
use super::support::{TestResult, error_body, mount_json, received};

// upstream-test: models/test_get.py::test_async_get_tuned_model
#[tokio::test]
async fn test_async_get_tuned_model() -> TestResult {
    let server = MockServer::start().await;
    mount_json(
        &server,
        "GET",
        200,
        &serde_json::json!({"name": "tunedModels/generate-num-1896"}),
    )
    .await;
    let client = test_client(server.uri());
    let model = client
        .models()
        .get("tunedModels/generate-num-1896", None)
        .await?;
    assert_eq!(model.name.as_deref(), Some("tunedModels/generate-num-1896"));
    Ok(())
}

// upstream-test: models/test_get.py::test_async_get_model
#[tokio::test]
async fn test_async_get_model() -> TestResult {
    let server = MockServer::start().await;
    mount_json(&server, "GET", 404, &error_body(404, "NOT_FOUND")).await;
    let client = test_client(server.uri());
    let config = GetModelConfig {
        http_options: Some(HttpOptions {
            api_version: Some("v1".to_owned()),
            headers: Some(HashMap::from([("test".to_owned(), "headers".to_owned())])),
            ..Default::default()
        }),
    };
    let result = client
        .models()
        .get("models/7687416965014487040", Some(config))
        .await;
    let Err(Error::Api(api)) = result else {
        return Err(format!("expected a 404 API error, got {result:?}").into());
    };
    assert_eq!(api.code, 404);
    let requests = received(&server).await?;
    assert_eq!(requests[0].url.path(), "/v1/models/7687416965014487040");
    Ok(())
}
