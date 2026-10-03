//! Ports of `caches/test_get.py`.

use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

const CACHE_ID: &str = "o239k1gxzz0juy9wqstndhncr85krehehf551hqh";

async fn mount_get(server: &MockServer, expected_calls: u64) {
    Mock::given(method("GET"))
        .and(path(format!("/v1beta/cachedContents/{CACHE_ID}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": format!("cachedContents/{CACHE_ID}"),
            "model": "models/gemini-2.5-flash",
        })))
        .expect(expected_calls)
        .mount(server)
        .await;
}

// upstream-test: caches/test_get.py::test_async_get
#[tokio::test]
async fn test_async_get() {
    let server = MockServer::start().await;
    mount_get(&server, 1).await;

    let cached = test_client(server.uri())
        .caches()
        .get(&format!("cachedContents/{CACHE_ID}"), None)
        .await
        .unwrap();
    assert_eq!(
        cached.name.as_deref(),
        Some(format!("cachedContents/{CACHE_ID}").as_str())
    );
    server.verify().await;
}

// Only the Gemini Developer API formats (full resource name, bare id) apply;
// the `projects/.../locations/...` forms are Vertex AI names.
// upstream-test: caches/test_get.py::test_different_cache_name_formats
#[tokio::test]
async fn test_different_cache_name_formats() {
    let server = MockServer::start().await;
    mount_get(&server, 2).await;

    let client = test_client(server.uri());
    for name in [format!("cachedContents/{CACHE_ID}"), CACHE_ID.to_owned()] {
        let cached = client.caches().get(&name, None).await.unwrap();
        assert_eq!(
            cached.name.as_deref(),
            Some(format!("cachedContents/{CACHE_ID}").as_str()),
            "name format {name:?}"
        );
    }
    server.verify().await;
}
