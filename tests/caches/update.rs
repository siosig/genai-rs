//! Ports of `caches/test_update.py`.

use gemini_genai::types::UpdateCachedContentConfig;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path},
};

use crate::common::test_client;

const CACHED_CONTENT_NAME_MLDEV: &str = "cachedContents/o239k1gxzz0juy9wqstndhncr85krehehf551hqh";

// upstream-test: caches/test_update.py::test_async_update
#[tokio::test]
async fn test_async_update() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1beta/{CACHED_CONTENT_NAME_MLDEV}")))
        .and(body_json(serde_json::json!({"ttl": "7600s"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": CACHED_CONTENT_NAME_MLDEV,
            "expireTime": "2026-01-01T02:06:40Z",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let config = UpdateCachedContentConfig {
        ttl: Some("7600s".to_owned()),
        ..Default::default()
    };
    let updated = test_client(server.uri())
        .caches()
        .update(CACHED_CONTENT_NAME_MLDEV, Some(config))
        .await
        .unwrap();
    assert_eq!(updated.name.as_deref(), Some(CACHED_CONTENT_NAME_MLDEV));
    server.verify().await;
}
