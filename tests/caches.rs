//! Integration tests for `client.caches()`: create/get/list/update/delete
//! against a wiremock server, driven entirely through the crate's public
//! API (this is a separate test crate, so only `pub` items are visible).

mod common;

use common::test_client;
use gemini_genai::types::{
    Content, CreateCachedContentConfig, ListCachedContentsConfig, Part, UpdateCachedContentConfig,
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path, query_param, query_param_is_missing},
};

#[tokio::test]
async fn create_posts_the_flattened_config_body_to_cached_contents() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/cachedContents"))
        .and(body_json(serde_json::json!({
            "model": "models/gemini-2.5-flash",
            "ttl": "86400s",
            "displayName": "test cache",
            "contents": [{"role": "user", "parts": [{"text": "cache me"}]}],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "cachedContents/abc123",
            "model": "models/gemini-2.5-flash",
            "displayName": "test cache",
            "createTime": "2026-01-01T00:00:00Z",
            "updateTime": "2026-01-01T00:00:00Z",
            "expireTime": "2026-01-02T00:00:00Z",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let config = CreateCachedContentConfig {
        display_name: Some("test cache".to_owned()),
        ttl: Some("86400s".to_owned()),
        contents: Some(vec![Content {
            role: Some("user".to_owned()),
            parts: Some(vec![Part {
                text: Some("cache me".to_owned()),
                ..Default::default()
            }]),
        }]),
        ..Default::default()
    };

    let client = test_client(server.uri());
    let cached = client
        .caches()
        .create("gemini-2.5-flash", Some(config))
        .await
        .unwrap();

    assert_eq!(cached.name.as_deref(), Some("cachedContents/abc123"));
    assert_eq!(cached.display_name.as_deref(), Some("test cache"));
    assert_eq!(cached.create_time.as_deref(), Some("2026-01-01T00:00:00Z"));
    assert_eq!(cached.expire_time.as_deref(), Some("2026-01-02T00:00:00Z"));
    server.verify().await;
}

#[tokio::test]
async fn create_without_config_sends_only_the_model_field() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/cachedContents"))
        .and(body_json(serde_json::json!({
            "model": "models/gemini-2.5-flash",
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"name": "cachedContents/abc123"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    client
        .caches()
        .create("gemini-2.5-flash", None)
        .await
        .unwrap();
    server.verify().await;
}

#[tokio::test]
async fn get_fetches_by_normalized_resource_name() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/cachedContents/abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "cachedContents/abc123",
            "model": "models/gemini-2.5-flash",
            "expireTime": "2026-01-02T00:00:00Z",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    // A bare id is normalized to `cachedContents/{id}` by `t_cached_content_name`.
    let cached = client.caches().get("abc123", None).await.unwrap();
    assert_eq!(cached.name.as_deref(), Some("cachedContents/abc123"));
    assert_eq!(cached.expire_time.as_deref(), Some("2026-01-02T00:00:00Z"));
    server.verify().await;
}

#[tokio::test]
async fn get_maps_a_client_error_to_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": {"code": 404, "message": "not found", "status": "NOT_FOUND"}
        })))
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let err = client.caches().get("missing", None).await.unwrap_err();
    match err {
        gemini_genai::Error::Api(api_err) => assert_eq!(api_err.code, 404),
        other => panic!("expected Error::Api, got {other:?}"),
    }
}

#[tokio::test]
async fn list_returns_a_pager_that_fetches_subsequent_pages() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/cachedContents"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "cachedContents": [{"name": "cachedContents/a"}],
            "nextPageToken": "tok1",
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1beta/cachedContents"))
        .and(query_param("pageToken", "tok1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "cachedContents": [{"name": "cachedContents/b"}],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let mut pager = client.caches().list(None).await.unwrap();
    assert_eq!(pager.name(), gemini_genai::pager::PagedItem::CachedContents);
    assert_eq!(pager.page().len(), 1);
    assert_eq!(pager.page()[0].name.as_deref(), Some("cachedContents/a"));

    let second = pager.next_page().await.unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].name.as_deref(), Some("cachedContents/b"));

    let err = pager.next_page().await.unwrap_err();
    assert!(matches!(err, gemini_genai::Error::NoMorePages));
    server.verify().await;
}

#[tokio::test]
async fn list_sends_page_size_as_a_query_parameter() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/cachedContents"))
        .and(query_param("pageSize", "5"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"cachedContents": []})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let config = ListCachedContentsConfig {
        page_size: Some(5),
        ..Default::default()
    };
    let client = test_client(server.uri());
    let pager = client.caches().list(Some(config)).await.unwrap();
    assert_eq!(pager.page().len(), 0);
    server.verify().await;
}

#[tokio::test]
async fn update_patches_by_name_with_the_ttl_body() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/v1beta/cachedContents/abc123"))
        .and(body_json(serde_json::json!({"ttl": "7600s"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "cachedContents/abc123",
            "expireTime": "2026-01-01T02:06:40Z",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let config = UpdateCachedContentConfig {
        ttl: Some("7600s".to_owned()),
        ..Default::default()
    };
    let client = test_client(server.uri());
    let updated = client
        .caches()
        .update("cachedContents/abc123", Some(config))
        .await
        .unwrap();
    assert_eq!(updated.expire_time.as_deref(), Some("2026-01-01T02:06:40Z"));
    server.verify().await;
}

#[tokio::test]
async fn delete_removes_by_name_and_deserializes_the_empty_response() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v1beta/cachedContents/abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let deleted = client.caches().delete("abc123", None).await.unwrap();
    assert_eq!(deleted.sdk_http_response, None);
    server.verify().await;
}

#[tokio::test]
async fn delete_deserializes_the_sdk_http_response_alias() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v1beta/cachedContents/abc123"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"sdkHttpResponse": {}})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let deleted = client.caches().delete("abc123", None).await.unwrap();
    assert!(deleted.sdk_http_response.is_some());
    server.verify().await;
}
