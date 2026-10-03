//! Port of `google/genai/tests/shared/chats/test_send_message.py`.

use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use crate::common::test_client;

// upstream-test: shared/chats/test_send_message.py::test_send_message
#[tokio::test]
async fn test_send_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "Hi!"}]},
                "finishReason": "STOP"
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let chat_model = "gemini-2.5-flash";
    let mut chat = test_client(server.uri())
        .chats()
        .create(chat_model, None, None);
    chat.send_message("Hello", None).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .url
            .path()
            .ends_with("models/gemini-2.5-flash:generateContent"),
        "path {}",
        requests[0].url.path()
    );
    let body: serde_json::Value = requests[0].body_json().unwrap();
    assert_eq!(body["contents"][0]["role"], "user");
    assert_eq!(body["contents"][0]["parts"][0]["text"], "Hello");
}
