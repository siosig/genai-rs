use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use super::common::test_client;

// upstream-test: tunings/test_get.py::test_helper_properties
#[tokio::test]
async fn test_helper_properties() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/v1beta/tunedModels/testdatasetexamples-model-j0fpgpaksvri",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "tunedModels/testdatasetexamples-model-j0fpgpaksvri",
            "state": "ACTIVE"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let job = test_client(server.uri())
        .tunings()
        .get("tunedModels/testdatasetexamples-model-j0fpgpaksvri", None)
        .await
        .unwrap_or_else(|error| panic!("tunings.get failed: {error}"));

    assert!(job.has_ended());
    assert!(job.has_succeeded());
    server.verify().await;
}
