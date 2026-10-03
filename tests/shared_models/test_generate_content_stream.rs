//! Port of `google/genai/tests/shared/models/test_generate_content_stream.py`.

use futures_util::StreamExt;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use crate::common::test_client;

// upstream-test: shared/models/test_generate_content_stream.py::test_generate_content_stream
#[tokio::test]
async fn test_generate_content_stream() {
    let server = MockServer::start().await;
    let sse = concat!(
        "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"The \"}]}}]}\n\n",
        "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"fox.\"}]},\"finishReason\":\"STOP\"}]}\n\n",
    );
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse)
                .insert_header("content-type", "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = test_client(server.uri())
        .models()
        .generate_content_stream(
            "gemini-2.5-flash",
            "The quick brown fox jumps over the lazy dog.",
            None,
        )
        .await
        .unwrap();
    let mut chunks = 0;
    while let Some(chunk) = stream.next().await {
        chunk.unwrap();
        chunks += 1;
    }

    assert_eq!(chunks, 2);
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests[0]
            .url
            .path()
            .ends_with("models/gemini-2.5-flash:streamGenerateContent")
    );
    let body: serde_json::Value = requests[0].body_json().unwrap();
    assert_eq!(
        body["contents"][0]["parts"][0]["text"],
        "The quick brown fox jumps over the lazy dog."
    );
}
