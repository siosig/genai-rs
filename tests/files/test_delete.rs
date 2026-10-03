//! Ports of `google/genai/tests/files/test_delete.py`.

use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use super::test_client;

// upstream-test: files/test_delete.py::test_async
#[tokio::test]
async fn test_async() -> Result<(), Box<dyn std::error::Error>> {
    // The upstream test body calls `client.aio.files.get(...)` (not
    // `delete`); it is ported as written, and the delete request itself is
    // covered by `delete_sends_a_delete_request_to_the_files_name_path`.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/files/n1gls7dyh90q"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"name": "files/n1gls7dyh90q"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let file = test_client(server.uri())
        .files()
        .get("files/n1gls7dyh90q", None)
        .await?;
    assert_eq!(file.name.as_deref(), Some("files/n1gls7dyh90q"));
    server.verify().await;
    Ok(())
}
