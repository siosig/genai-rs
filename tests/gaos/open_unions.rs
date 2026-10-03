//! Forward compatibility of the open (`parse_open_union`) unions: a discriminator value this
//! crate does not know must survive decoding and re-encoding unchanged, and known variants must
//! keep decoding next to it. Hand-written; driven through the public client API only.

use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::test_client;

#[expect(
    clippy::expect_used,
    clippy::default_trait_access,
    reason = "test helper: a failed decode should panic; the query struct is private to the crate"
)]
async fn get_interaction(body: &Value) -> Value {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/interactions/abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;
    let client = test_client(server.uri());
    let interaction = client
        .interactions()
        .get("abc", &Default::default())
        .await
        .expect("interaction decodes");
    serde_json::to_value(&interaction).expect("interaction encodes")
}

#[tokio::test]
async fn unknown_step_type_round_trips_unchanged_beside_known_steps() {
    let body = json!({
        "id": "abc",
        "steps": [
            {"type": "thought", "signature": "sig"},
            {"type": "step_from_the_future", "payload": {"nested": [1, 2, 3]}},
        ],
    });

    let round_tripped = get_interaction(&body).await;

    assert_eq!(round_tripped["steps"][1], body["steps"][1]);
    assert_eq!(round_tripped["steps"][0]["type"], "thought");
    assert_eq!(round_tripped["steps"][0]["signature"], "sig");
}

#[tokio::test]
async fn unknown_enum_value_round_trips_unchanged() {
    let body = json!({"id": "abc", "status": "a_status_from_the_future"});

    let round_tripped = get_interaction(&body).await;

    assert_eq!(round_tripped["status"], "a_status_from_the_future");
}
