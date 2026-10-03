//! Ported from `google/genai/tests/gaos/test_triggers_lifecycle.py`.

use gemini_genai::triggers::{
    TriggerListExecutionsParams, TriggerListParams, normalize_trigger_create_body,
};
use serde_json::{Value, json};

use crate::{
    common::test_client_with_api_key,
    recording::{captured, captured_bodies, recording_server},
};

fn trigger_body() -> Value {
    json!({
        "id": "projects/my-project/locations/my-location/triggers/svc_abc",
        "schedule": "0 0 * * *",
        "time_zone": "UTC",
        "interaction": {
            "agent": "projects/my-project/locations/my-location/agents/my-agent",
            "input": "test-input",
            "environment": {
                "type": "remote",
                "network": {
                    "allowlist": [
                        {
                            "domain": "api.github.com",
                            "transform": {
                                "Authorization": "Bearer test-github-token-placeholder"
                            },
                        },
                        {"domain": "github.com"},
                    ]
                },
            },
        },
    })
}

fn interaction_with_input(input: Value) -> Value {
    json!({
        "agent": "projects/my-project/locations/my-location/agents/my-agent",
        "input": input,
        "environment": {
            "type": "remote",
            "network": {
                "allowlist": [
                    {
                        "domain": "api.github.com",
                        "transform": {
                            "Authorization": "Bearer test-github-token-placeholder"
                        },
                    },
                    {"domain": "github.com"},
                ]
            },
        },
    })
}

// upstream-test: gaos/test_triggers_lifecycle.py::test_python_triggers_lifecycle_routes_through_google_genai_client
#[tokio::test]
async fn test_python_triggers_lifecycle_routes_through_google_genai_client() {
    let step_input =
        json!([{"type": "user_input", "content": [{"type": "text", "text": "test-input-step"}]}]);
    let two_texts = json!([
        {"type": "text", "text": "test-input-content-1"},
        {"type": "text", "text": "test-input-content-2"},
    ]);
    let wrapped_two_texts = json!([{
        "type": "user_input",
        "content": [
            {"type": "text", "text": "test-input-content-1"},
            {"type": "text", "text": "test-input-content-2"},
        ],
    }]);
    let single_content = json!({"type": "text", "text": "test-input-single-content"});
    // The Python facade also accepts content blocks without a `type`; they have no
    // typed Rust form, so that case is normalized from untyped JSON first.
    let shorthand = json!([
        {"text": "test-input-content-shorthand-1"},
        {"text": "test-input-content-shorthand-2"},
    ]);
    let wrapped_shorthand = json!([{
        "type": "user_input",
        "content": [
            {"type": "text", "text": "test-input-content-shorthand-1"},
            {"type": "text", "text": "test-input-content-shorthand-2"},
        ],
    }]);
    // (input value, expected `interaction.input` on the wire), as in the upstream parametrization.
    let cases = [
        (json!("test-input-str"), json!("test-input-str")),
        (step_input.clone(), step_input),
        (two_texts, wrapped_two_texts),
        (single_content.clone(), single_content),
        (shorthand, wrapped_shorthand),
    ];

    for (input_value, expected_input_value) in cases {
        let server = recording_server(|_, _| trigger_body()).await;
        let client = test_client_with_api_key(server.uri(), "test-api-key");
        let triggers = client.triggers();
        let create_body = normalize_trigger_create_body(json!({
            "interaction": interaction_with_input(input_value.clone()),
            "schedule": "0 0 * * *",
            "time_zone": "UTC",
        }))
        .unwrap();

        let trigger = triggers
            .create(&serde_json::from_value(create_body).unwrap())
            .await
            .unwrap_or_else(|err| panic!("create failed for input {input_value}: {err}"));
        triggers
            .list(&TriggerListParams {
                filter: Some("some-filter".to_owned()),
                page_size: Some(10),
                ..Default::default()
            })
            .await
            .unwrap();
        let fetched = triggers.get("svc_abc").await.unwrap();
        triggers
            .update(
                "svc_abc",
                &serde_json::from_value(
                    json!({"display_name": "updated-name", "status": "paused"}),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        triggers.delete("svc_abc").await.unwrap();
        triggers.run("svc_abc").await.unwrap();
        triggers
            .list_executions(
                "svc_abc",
                &TriggerListExecutionsParams {
                    page_size: Some(5),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(trigger.schedule.as_deref(), Some("0 0 * * *"));
        assert_eq!(fetched.schedule.as_deref(), Some("0 0 * * *"));
        assert_eq!(
            captured(&server).await,
            [
                "POST /v1beta/triggers",
                "GET /v1beta/triggers?filter=some-filter&page_size=10",
                "GET /v1beta/triggers/svc_abc",
                "PATCH /v1beta/triggers/svc_abc",
                "DELETE /v1beta/triggers/svc_abc",
                "POST /v1beta/triggers/svc_abc/executions",
                "GET /v1beta/triggers/svc_abc/executions?page_size=5",
            ],
            "input {input_value}"
        );
        // Verify the serialized interaction input in the CREATE request
        let create_body = &captured_bodies(&server).await[0];
        assert_eq!(
            create_body["interaction"]["input"], expected_input_value,
            "input {input_value}"
        );
    }
}
