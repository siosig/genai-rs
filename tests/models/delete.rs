//! Ports of `models/test_delete.py` plain (async) functions, Developer API.

use std::collections::HashMap;

use gemini_genai::{
    Error,
    types::{DeleteModelConfig, HttpOptions},
};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use super::common::test_client;
use super::support::{TestResult, error_body, mount_json, received};

const TEST_API_VERSION: &str = "test_api_version";

// upstream-test: models/test_delete.py::test_async_delete_model_with_http_options_in_method
#[tokio::test]
async fn test_async_delete_model_with_http_options_in_method() -> TestResult {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(404).set_body_json(error_body(404, "NOT_FOUND")))
        .mount(&server)
        .await;
    let client = test_client(server.uri());
    let config = DeleteModelConfig {
        http_options: Some(HttpOptions {
            api_version: Some(TEST_API_VERSION.to_owned()),
            headers: Some(HashMap::from([("test".to_owned(), "headers".to_owned())])),
            ..Default::default()
        }),
    };
    let result = client
        .models()
        .delete("tunedModels/generate-num-888", Some(config))
        .await;
    let Err(Error::Api(api)) = result else {
        return Err(format!("expected a 404 API error, got {result:?}").into());
    };
    assert_eq!(api.code, 404);
    let requests = received(&server).await?;
    assert_eq!(
        requests[0].url.path(),
        "/test_api_version/tunedModels/generate-num-888"
    );
    assert_eq!(
        requests[0]
            .headers
            .get("test")
            .and_then(|v| v.to_str().ok()),
        Some("headers")
    );
    Ok(())
}

// upstream-test: models/test_delete.py::test_async_delete_tuned_model
#[tokio::test]
async fn test_async_delete_tuned_model() -> TestResult {
    let server = MockServer::start().await;
    mount_json(&server, "DELETE", 200, &serde_json::json!({})).await;
    let client = test_client(server.uri());
    client
        .models()
        .delete("tunedModels/generate-num-888", None)
        .await?;
    let requests = received(&server).await?;
    assert_eq!(
        requests[0].url.path(),
        "/v1beta/tunedModels/generate-num-888"
    );
    Ok(())
}

// upstream-test: models/test_delete.py::test_async_delete_model
#[tokio::test]
async fn test_async_delete_model() -> TestResult {
    let server = MockServer::start().await;
    mount_json(&server, "DELETE", 404, &error_body(404, "NOT_FOUND")).await;
    let client = test_client(server.uri());
    let result = client
        .models()
        .delete("models/1071206899942162432", None)
        .await;
    let Err(Error::Api(api)) = result else {
        return Err(format!("expected a 404 API error, got {result:?}").into());
    };
    assert_eq!(api.code, 404);
    Ok(())
}
