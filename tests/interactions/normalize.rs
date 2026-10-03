//! Not ported from upstream: checks of the request normalization the Python
//! facade applies in `create` (`_normalize_create_body`), which the upstream
//! suite only exercises through the triggers lifecycle test.

use gemini_genai::{
    Error,
    interactions::{
        Content, CreateInteractionRequestBody, CreateModelInteraction, InteractionsInput, Model,
        normalize_create_body,
    },
};
use serde_json::json;
use wiremock::ResponseTemplate;

use crate::{
    common::test_client,
    recording::{completed, received, server_answering},
};

#[test]
fn content_blocks_are_wrapped_in_one_user_input_step() {
    let body = normalize_create_body(json!({
        "model": "gemini-2.5-flash",
        "input": [{"text": "a"}, {"type": "text", "text": "b"}],
    }))
    .unwrap();

    assert_eq!(
        body["input"],
        json!([{
            "type": "user_input",
            "content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}],
        }])
    );
}

#[test]
fn steps_and_role_turns_are_left_alone() {
    for input in [
        json!([{"type": "user_input", "content": []}]),
        json!([{"role": "user", "content": "hi"}]),
        json!("plain text"),
        json!([]),
    ] {
        let body = normalize_create_body(json!({"model": "m", "input": input.clone()})).unwrap();

        assert_eq!(body["input"], input);
    }
}

#[test]
fn unknown_keys_are_rejected_with_their_names() {
    let err = normalize_create_body(json!({"model": "m", "zeta": 1, "alpha": 2})).unwrap_err();

    assert!(
        matches!(&err, Error::Validation(msg) if msg.contains("alpha, zeta")),
        "{err:?}"
    );
}

#[tokio::test]
async fn typed_create_wraps_a_content_list_input() {
    let server = server_answering(ResponseTemplate::new(200).set_body_json(completed())).await;
    let client = test_client(server.uri());
    let content: Content =
        serde_json::from_value(json!({"type": "text", "text": "Hello"})).unwrap();
    let body = CreateInteractionRequestBody::CreateModelInteraction(CreateModelInteraction {
        model: Some(Model::from("gemini-2.5-flash".to_owned())),
        input: Some(InteractionsInput::List2(vec![content])),
        ..Default::default()
    });

    client.interactions().create(&body).await.unwrap();

    let requests = received(&server).await;
    let sent: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(
        sent["input"],
        json!([{"type": "user_input", "content": [{"type": "text", "text": "Hello"}]}])
    );
}
