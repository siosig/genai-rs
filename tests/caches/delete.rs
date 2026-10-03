//! Ports of `caches/test_delete.py`.

use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

const CACHED_CONTENT_NAME_MLDEV: &str = "cachedContents/o239k1gxzz0juy9wqstndhncr85krehehf551hqh";

// upstream-test: caches/test_delete.py::test_async_delete
#[tokio::test]
async fn test_async_delete() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1beta/{CACHED_CONTENT_NAME_MLDEV}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .expect(1)
        .mount(&server)
        .await;

    test_client(server.uri())
        .caches()
        .delete(CACHED_CONTENT_NAME_MLDEV, None)
        .await
        .unwrap();
    server.verify().await;
}
