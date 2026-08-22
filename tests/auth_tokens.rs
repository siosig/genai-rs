//! Integration tests for `client.auth_tokens()` (ephemeral Live API auth
//! tokens). Runs against the public API only, via `wiremock`, mirroring
//! `src/auth_tokens.rs`'s own unit tests but exercised from outside the
//! crate.

mod common;

use common::test_client;
use google_genai::types::{CreateAuthTokenConfig, LiveConnectConstraints};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn create_posts_uses_expire_time_and_returns_the_token_name() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/auth_tokens"))
        .and(body_json(serde_json::json!({
            "uses": 10,
            "expireTime": "2025-05-01T00:00:00Z"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "auth_tokens/abc123",
            "expireTime": "2025-05-01T00:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let token = client
        .auth_tokens()
        .create(Some(CreateAuthTokenConfig {
            uses: Some(10),
            expire_time: Some("2025-05-01T00:00:00Z".to_owned()),
            ..Default::default()
        }))
        .await
        .unwrap();

    assert_eq!(token.name.as_deref(), Some("auth_tokens/abc123"));
    assert_eq!(token.expire_time.as_deref(), Some("2025-05-01T00:00:00Z"));
    server.verify().await;
}

#[tokio::test]
async fn create_with_live_connect_constraints_locks_the_setup() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/auth_tokens"))
        .and(body_json(serde_json::json!({
            "uses": 1,
            "bidiGenerateContentSetup": {
                "model": "models/gemini-live-2.5-flash-preview"
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "auth_tokens/xyz789"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let token = client
        .auth_tokens()
        .create(Some(CreateAuthTokenConfig {
            uses: Some(1),
            live_connect_constraints: Some(LiveConnectConstraints {
                model: Some("gemini-live-2.5-flash-preview".to_owned()),
                config: None,
            }),
            ..Default::default()
        }))
        .await
        .unwrap();

    assert_eq!(token.name.as_deref(), Some("auth_tokens/xyz789"));
    server.verify().await;
}
