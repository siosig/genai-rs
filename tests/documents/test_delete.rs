//! Ports of the plain tests in `documents/test_delete.py`.

use gemini_genai::types::DeleteDocumentConfig;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

use crate::common::test_client;

// upstream-test: documents/test_delete.py::test_async_delete
#[tokio::test]
async fn test_async_delete() {
    let name = "fileSearchStores/fr3l0ri2so25-a3r1ump9x821/documents/asurveyofmodernistpoetrytxt-uvmqjtmkm1h2";
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1beta/{name}")))
        .and(query_param("force", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    test_client(server.uri())
        .file_search_stores()
        .documents()
        .delete(
            name,
            Some(DeleteDocumentConfig {
                force: Some(true),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    server.verify().await;
}
