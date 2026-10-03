//! Ports of `models/test_update.py` plain (async) functions, Developer API.

use std::collections::HashMap;

use gemini_genai::{
    Error,
    types::{HttpOptions, UpdateModelConfig},
};
use wiremock::MockServer;

use super::common::test_client;
use super::support::{TestResult, error_body, mount_json, received};

// upstream-test: models/test_update.py::test_async_update_model
#[tokio::test]
async fn test_async_update_model() -> TestResult {
    let server = MockServer::start().await;
    mount_json(&server, "PATCH", 404, &error_body(404, "NOT_FOUND")).await;
    let client = test_client(server.uri());
    let config = UpdateModelConfig {
        display_name: Some("My tuned gemini model".to_owned()),
        http_options: Some(HttpOptions {
            headers: Some(HashMap::from([("test".to_owned(), "headers".to_owned())])),
            ..Default::default()
        }),
        ..Default::default()
    };
    let result = client
        .models()
        .update("models/2171259487439028224", config)
        .await;
    let Err(Error::Api(api)) = result else {
        return Err(format!("expected a 404 API error, got {result:?}").into());
    };
    assert_eq!(api.code, 404);
    let requests = received(&server).await?;
    assert_eq!(requests[0].url.path(), "/v1beta/models/2171259487439028224");
    Ok(())
}
