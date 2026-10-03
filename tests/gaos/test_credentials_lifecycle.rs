//! Ported from `google/genai/tests/gaos/test_credentials_lifecycle.py`.
//!
//! The Rust client is async-only, so the upstream sync and async variants both
//! drive the one async API; the upstream local recording HTTP server becomes a
//! wiremock server that answers with the same bodies.

use gemini_genai::{
    credentials::{
        Credential, CredentialCreateParams, CredentialListParams, CredentialListResponse,
        EnvironmentVariableConfig, EnvironmentVariableConfigInjectionLocation,
        EnvironmentVariableUpdateConfig, HTTPBearerConfig, HTTPBearerUpdateConfig,
        InjectionLocationEnum, OAuth2Config, OAuth2UpdateConfig,
    },
    types::HttpOptions,
};
use serde_json::{Value, json};

use crate::{
    common::test_client_with_api_key,
    recording::{captured, captured_bodies, recording_server},
};

fn credential_body() -> Value {
    json!({
        "id": "cred_bearer_123",
        "status": "active",
        "type": "bearer_token",
        "create_time": "2026-07-22T15:18:38Z",
        "update_time": "2026-07-22T15:18:38Z",
    })
}

fn credential_list_body() -> Value {
    json!({
        "credentials": [
            credential_body(),
            {
                "id": "cred_env_123",
                "status": "active",
                "type": "environment_variable",
                "create_time": "2026-07-22T15:18:38Z",
                "update_time": "2026-07-22T15:18:38Z",
            },
            {
                "id": "cred_oauth_123",
                "status": "active",
                "type": "oauth2",
                "create_time": "2026-07-22T15:18:38Z",
                "update_time": "2026-07-22T15:18:38Z",
            },
        ],
        "next_page_token": "token_next_123",
    })
}

/// The upstream handler: the list endpoint returns the list body, everything
/// else the single bearer credential.
fn credential_payload(method: &str, path_and_query: &str) -> Value {
    if method == "GET"
        && (path_and_query == "/v1beta/credentials"
            || path_and_query.starts_with("/v1beta/credentials?"))
    {
        credential_list_body()
    } else {
        credential_body()
    }
}

fn create_params(body: Value) -> CredentialCreateParams {
    serde_json::from_value(body).unwrap()
}

// upstream-test: gaos/test_credentials_lifecycle.py::test_python_credentials_lifecycle_routes_through_google_genai_client
#[tokio::test]
async fn test_python_credentials_lifecycle_routes_through_google_genai_client() {
    let server = recording_server(credential_payload).await;
    let client = test_client_with_api_key(server.uri(), "test-api-key");
    let bearer_body = json!({
        "id": "cred_bearer_123",
        "token": "test-bearer-token",
        "header_name": "X-Custom-Auth",
        "prefix": "Token",
        "type": "bearer_token",
    });
    let env_body = json!({
        "id": "cred_env_123",
        "value": "super-secret-key",
        "injection_location": ["header", "query"],
        "trusted_domains": ["api.example.com", "service.example.org"],
        "type": "environment_variable",
    });
    let oauth_body = json!({
        "id": "cred_oauth_123",
        "client_id": "test-client-id",
        "client_secret": "test-client-secret",
        "refresh_token": "test-refresh-token",
        "token_url": "https://oauth2.googleapis.com/token",
        "scopes": ["https://www.googleapis.com/auth/cloud-platform"],
        "type": "oauth2",
    });
    let credentials = client.credentials();

    // 1. Create Bearer Token Credential with header_name and prefix
    let bearer_cred = credentials
        .create(&create_params(bearer_body.clone()))
        .await
        .unwrap();
    assert_eq!(bearer_cred.id.as_deref(), Some("cred_bearer_123"));
    // 2. Create Environment Variable Credential with injection_location and trusted_domains
    let env_cred = credentials
        .create(&create_params(env_body.clone()))
        .await
        .unwrap();
    assert_eq!(env_cred.id.as_deref(), Some("cred_bearer_123"));
    // 3. Create OAuth2 Credential with scopes
    let oauth_cred = credentials
        .create(&create_params(oauth_body.clone()))
        .await
        .unwrap();
    assert_eq!(oauth_cred.id.as_deref(), Some("cred_bearer_123"));
    // 4. List credentials
    let list_res = credentials.list(&Default::default()).await.unwrap();
    assert_eq!(list_res.credentials.as_ref().map(Vec::len), Some(3));
    // 5. Get credential
    let fetched = credentials.get("cred_bearer_123").await.unwrap();
    assert_eq!(fetched.id.as_deref(), Some("cred_bearer_123"));
    // 6. Update Bearer Token Credential
    credentials
        .update(
            "cred_bearer_123",
            &serde_json::from_value(json!({
                "token": "updated-bearer-token",
                "header_name": "Authorization",
                "prefix": "Bearer",
                "type": "bearer_token",
            }))
            .unwrap(),
            &Default::default(),
        )
        .await
        .unwrap();
    // 7. Update Environment Variable Credential
    credentials
        .update(
            "cred_env_123",
            &serde_json::from_value(json!({
                "value": "updated-secret",
                "injection_location": "header",
                "trusted_domains": ["api.example.com"],
                "type": "environment_variable",
            }))
            .unwrap(),
            &Default::default(),
        )
        .await
        .unwrap();
    // 8. Update OAuth2 Credential
    credentials
        .update(
            "cred_oauth_123",
            &serde_json::from_value(json!({
                "client_secret": "updated-secret",
                "scopes": ["scope1", "scope2"],
                "type": "oauth2",
            }))
            .unwrap(),
            &Default::default(),
        )
        .await
        .unwrap();
    // 9. Delete credential
    credentials.delete("cred_bearer_123").await.unwrap();

    assert_eq!(
        captured(&server).await,
        [
            "POST /v1beta/credentials",
            "POST /v1beta/credentials",
            "POST /v1beta/credentials",
            "GET /v1beta/credentials",
            "GET /v1beta/credentials/cred_bearer_123",
            "PATCH /v1beta/credentials/cred_bearer_123",
            "PATCH /v1beta/credentials/cred_env_123",
            "PATCH /v1beta/credentials/cred_oauth_123",
            "DELETE /v1beta/credentials/cred_bearer_123",
        ]
    );
    // Verify captured bodies
    let bodies = captured_bodies(&server).await;
    assert_eq!(bodies[0], bearer_body);
    assert_eq!(bodies[1], env_body);
    assert_eq!(bodies[2], oauth_body);
}

// upstream-test: gaos/test_credentials_lifecycle.py::test_python_credentials_async_lifecycle
#[tokio::test]
async fn test_python_credentials_async_lifecycle() {
    let server = recording_server(credential_payload).await;
    let client = test_client_with_api_key(server.uri(), "test-api-key");
    let credentials = client.credentials();

    let credential = credentials
        .create(&create_params(json!({
            "id": "cred_env_123",
            "value": "super-secret-key",
            "injection_location": ["header", "query"],
            "trusted_domains": ["api.example.com"],
            "type": "environment_variable",
        })))
        .await
        .unwrap();
    let list_res = credentials.list(&Default::default()).await.unwrap();
    let fetched = credentials.get("cred_env_123").await.unwrap();
    let updated = credentials
        .update(
            "cred_env_123",
            &serde_json::from_value(json!({
                "value": "updated-secret",
                "injection_location": "header",
                "type": "environment_variable",
            }))
            .unwrap(),
            &Default::default(),
        )
        .await
        .unwrap();
    credentials.delete("cred_env_123").await.unwrap();

    assert_eq!(credential.id.as_deref(), Some("cred_bearer_123"));
    assert_eq!(fetched.id.as_deref(), Some("cred_bearer_123"));
    assert_eq!(updated.id.as_deref(), Some("cred_bearer_123"));
    assert_eq!(list_res.credentials.as_ref().map(Vec::len), Some(3));
    assert_eq!(
        captured(&server).await,
        [
            "POST /v1beta/credentials",
            "GET /v1beta/credentials",
            "GET /v1beta/credentials/cred_env_123",
            "PATCH /v1beta/credentials/cred_env_123",
            "DELETE /v1beta/credentials/cred_env_123",
        ]
    );
}

// upstream-test: gaos/test_credentials_lifecycle.py::test_python_credentials_with_raw_response
#[tokio::test]
async fn test_python_credentials_with_raw_response() {
    // Python's `with_raw_response.list().parse()` is the parsed list response: the
    // Rust client has no raw-response wrapper and returns the parsed value directly.
    let server = recording_server(credential_payload).await;
    let client = test_client_with_api_key(server.uri(), "test-api-key");

    let parsed = client
        .credentials()
        .list(&Default::default())
        .await
        .unwrap();

    let credentials = parsed.credentials.unwrap();
    assert_eq!(credentials.len(), 3);
    assert_eq!(credentials[0].id.as_deref(), Some("cred_bearer_123"));
}

// upstream-test: gaos/test_credentials_lifecycle.py::test_python_credentials_types_and_models
#[test]
fn test_python_credentials_types_and_models() {
    let cred: Credential = serde_json::from_value(json!({
        "id": "cred_123",
        "status": "active",
        "type": "bearer_token",
        "create_time": "2026-07-22T15:18:38Z",
        "update_time": "2026-07-22T15:18:38Z",
    }))
    .unwrap();
    assert_eq!(cred.id.as_deref(), Some("cred_123"));
    assert_eq!(cred.status.as_ref().map(|s| s.as_str()), Some("active"));
    assert_eq!(
        cred.r#type.as_ref().map(|t| t.as_str()),
        Some("bearer_token")
    );
    assert!(cred.create_time.is_some());
    assert!(cred.update_time.is_some());

    let list_resp = CredentialListResponse {
        credentials: Some(vec![cred]),
        next_page_token: Some("next_tok".to_owned()),
    };
    assert_eq!(list_resp.credentials.as_ref().map(Vec::len), Some(1));
    assert_eq!(list_resp.next_page_token.as_deref(), Some("next_tok"));

    // HTTP Bearer Config
    let bearer = HTTPBearerConfig {
        id: Some("cred_bearer".to_owned()),
        token: Some("secret-token".to_owned()),
        header_name: Some("X-Auth".to_owned()),
        prefix: Some("Bearer".to_owned()),
        ..Default::default()
    };
    assert_eq!(bearer.id.as_deref(), Some("cred_bearer"));
    assert_eq!(bearer.token.as_deref(), Some("secret-token"));
    assert_eq!(bearer.header_name.as_deref(), Some("X-Auth"));
    assert_eq!(bearer.prefix.as_deref(), Some("Bearer"));
    assert_eq!(bearer.r#type, "bearer_token");

    let bearer_update = HTTPBearerUpdateConfig {
        token: Some("new-token".to_owned()),
        header_name: Some("Authorization".to_owned()),
        prefix: Some("Token".to_owned()),
        ..Default::default()
    };
    assert_eq!(bearer_update.token.as_deref(), Some("new-token"));
    assert_eq!(bearer_update.header_name.as_deref(), Some("Authorization"));
    assert_eq!(bearer_update.prefix.as_deref(), Some("Token"));
    assert_eq!(bearer_update.r#type, "bearer_token");

    // OAuth2 Config
    let oauth = OAuth2Config {
        id: Some("cred_oauth".to_owned()),
        client_id: Some("cid".to_owned()),
        client_secret: Some("csecret".to_owned()),
        refresh_token: Some("rtoken".to_owned()),
        token_url: Some("https://example.com/token".to_owned()),
        scopes: Some(vec![
            "https://www.googleapis.com/auth/cloud-platform".to_owned(),
        ]),
        ..Default::default()
    };
    assert_eq!(oauth.id.as_deref(), Some("cred_oauth"));
    assert_eq!(oauth.client_id.as_deref(), Some("cid"));
    assert_eq!(oauth.client_secret.as_deref(), Some("csecret"));
    assert_eq!(oauth.refresh_token.as_deref(), Some("rtoken"));
    assert_eq!(
        oauth.token_url.as_deref(),
        Some("https://example.com/token")
    );
    assert_eq!(
        oauth.scopes,
        Some(vec![
            "https://www.googleapis.com/auth/cloud-platform".to_owned()
        ])
    );
    assert_eq!(oauth.r#type, "oauth2");

    let oauth_update = OAuth2UpdateConfig {
        client_secret: Some("new-secret".to_owned()),
        scopes: Some(vec!["scope1".to_owned(), "scope2".to_owned()]),
        ..Default::default()
    };
    assert_eq!(oauth_update.client_secret.as_deref(), Some("new-secret"));
    assert_eq!(
        oauth_update.scopes,
        Some(vec!["scope1".to_owned(), "scope2".to_owned()])
    );
    assert_eq!(oauth_update.r#type, "oauth2");

    // Environment Variable Config with single and multiple injection locations
    let env_var = EnvironmentVariableConfig {
        id: Some("cred_env".to_owned()),
        value: Some("secret-key".to_owned()),
        injection_location: Some(EnvironmentVariableConfigInjectionLocation::List(vec![
            InjectionLocationEnum::Header,
            InjectionLocationEnum::Query,
        ])),
        trusted_domains: Some(vec!["api.example.com".to_owned()]),
        ..Default::default()
    };
    assert_eq!(env_var.id.as_deref(), Some("cred_env"));
    assert_eq!(env_var.value.as_deref(), Some("secret-key"));
    assert_eq!(
        env_var.injection_location,
        Some(EnvironmentVariableConfigInjectionLocation::List(vec![
            InjectionLocationEnum::Header,
            InjectionLocationEnum::Query,
        ]))
    );
    assert_eq!(
        env_var.trusted_domains,
        Some(vec!["api.example.com".to_owned()])
    );
    assert_eq!(env_var.r#type, "environment_variable");

    let env_var_single = EnvironmentVariableConfig {
        id: Some("cred_env_2".to_owned()),
        value: Some("secret-key".to_owned()),
        injection_location: Some(
            EnvironmentVariableConfigInjectionLocation::InjectionLocationEnum(
                InjectionLocationEnum::Header,
            ),
        ),
        ..Default::default()
    };
    assert_eq!(
        env_var_single.injection_location,
        Some(
            EnvironmentVariableConfigInjectionLocation::InjectionLocationEnum(
                InjectionLocationEnum::Header
            )
        )
    );

    let env_var_update = EnvironmentVariableUpdateConfig {
        value: Some("updated-key".to_owned()),
        injection_location: Some(
            EnvironmentVariableConfigInjectionLocation::InjectionLocationEnum(
                InjectionLocationEnum::Query,
            ),
        ),
        trusted_domains: Some(vec!["new.example.com".to_owned()]),
        ..Default::default()
    };
    assert_eq!(env_var_update.value.as_deref(), Some("updated-key"));
    assert_eq!(
        env_var_update.injection_location,
        Some(
            EnvironmentVariableConfigInjectionLocation::InjectionLocationEnum(
                InjectionLocationEnum::Query
            )
        )
    );
    assert_eq!(
        env_var_update.trusted_domains,
        Some(vec!["new.example.com".to_owned()])
    );
    assert_eq!(env_var_update.r#type, "environment_variable");

    // Request models: the Rust list request carries the query parameters only;
    // `id` and `api_version` are a path argument and a per-call `HttpOptions`.
    let list_req = CredentialListParams {
        page_size: Some(10),
        page_token: Some("tok".to_owned()),
    };
    assert_eq!(list_req.page_size, Some(10));
    assert_eq!(list_req.page_token.as_deref(), Some("tok"));
}

/// The upstream request models carry a per-call `api_version`; in Rust it is a
/// per-call `HttpOptions` on the handle.
#[tokio::test]
async fn credentials_get_honors_a_per_call_api_version() {
    let server = recording_server(credential_payload).await;
    let client = test_client_with_api_key(server.uri(), "test-api-key");

    client
        .credentials()
        .with_http_options(HttpOptions {
            api_version: Some("v1alpha".to_owned()),
            ..Default::default()
        })
        .get("cred_123")
        .await
        .unwrap();

    assert_eq!(
        captured(&server).await,
        ["GET /v1alpha/credentials/cred_123"]
    );
}
