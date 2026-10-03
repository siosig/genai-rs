//! Ports of `batches/test_create_with_inlined_requests.py`.

use gemini_genai::types::{BatchJobSource, Content, InlinedRequest, Part};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

const GEMINI_MODEL: &str = "gemini-2.5-flash";

fn inlined_request(text: &str, key: &str) -> InlinedRequest {
    InlinedRequest {
        contents: Some(vec![Content {
            role: Some("user".to_owned()),
            parts: Some(vec![Part {
                text: Some(text.to_owned()),
                ..Default::default()
            }]),
        }]),
        metadata: Some([("key".to_owned(), key.to_owned())].into()),
        ..Default::default()
    }
}

// upstream-test: batches/test_create_with_inlined_requests.py::test_async_create
#[tokio::test]
async fn test_async_create() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/v1beta/models/{GEMINI_MODEL}:batchGenerateContent"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "batches/abc123",
            "metadata": {
                "state": "BATCH_STATE_PENDING",
                "model": format!("models/{GEMINI_MODEL}"),
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let src = BatchJobSource {
        inlined_requests: Some(vec![
            inlined_request(
                "what is the number after 1? return just the number.",
                "request-1",
            ),
            inlined_request(
                "what is the number after 2? return just the number.",
                "request-2",
            ),
        ]),
        ..Default::default()
    };
    let batch_job = client
        .batches()
        .create(GEMINI_MODEL, src, None)
        .await
        .unwrap();
    assert!(batch_job.name.as_deref().unwrap().starts_with("batches/"));
    assert_eq!(
        batch_job.model.as_deref(),
        Some(format!("models/{GEMINI_MODEL}").as_str())
    );

    let received = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
    let requests = &body["batch"]["inputConfig"]["requests"]["requests"];
    assert_eq!(requests.as_array().map(Vec::len), Some(2));
    assert_eq!(requests[1]["metadata"]["key"], "request-2");
    server.verify().await;
}
