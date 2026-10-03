use gemini_genai::types::{CreateAuthTokenConfig, HttpOptions};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use super::common::test_client;

// upstream-test: tokens/test_create.py::test_async_create_no_lock
#[tokio::test]
async fn test_async_create_no_lock() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1alpha/auth_tokens"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"name": "auth_tokens/no-lock"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let token = test_client(server.uri())
        .auth_tokens()
        .create(Some(CreateAuthTokenConfig {
            http_options: Some(HttpOptions {
                api_version: Some("v1alpha".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        }))
        .await
        .unwrap_or_else(|error| panic!("auth_tokens.create failed: {error}"));

    assert_eq!(token.name.as_deref(), Some("auth_tokens/no-lock"));
    server.verify().await;
}
