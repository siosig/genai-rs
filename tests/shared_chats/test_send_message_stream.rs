//! Port of `google/genai/tests/shared/chats/test_send_message_stream.py`.

use futures_util::StreamExt;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use crate::common::test_client;

// upstream-test: shared/chats/test_send_message_stream.py::test_send_message_stream
#[tokio::test]
async fn test_send_message_stream() {
    let server = MockServer::start().await;
    let sse = concat!(
        "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Why did \"}]}}]}\n\n",
        "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"the chicken...\"}]},\"finishReason\":\"STOP\"}]}\n\n",
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

    let mut chat = test_client(server.uri())
        .chats()
        .create("gemini-2.5-flash", None, None);
    let mut stream = chat
        .send_message_stream("Tell a joke.", None)
        .await
        .unwrap();
    let mut chunks = 0;
    while let Some(chunk) = stream.next().await {
        chunk.unwrap();
        chunks += 1;
    }
    drop(stream);

    assert_eq!(chunks, 2);
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests[0]
            .url
            .path()
            .ends_with("models/gemini-2.5-flash:streamGenerateContent"),
        "path {}",
        requests[0].url.path()
    );
    let body: serde_json::Value = requests[0].body_json().unwrap();
    assert_eq!(body["contents"][0]["parts"][0]["text"], "Tell a joke.");
    assert_eq!(chat.get_history(true).len(), 3);
}
