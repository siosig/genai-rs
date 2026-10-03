//! Ported from `google/genai/tests/interactions/test_paths.py` (Developer API
//! branch; the Vertex AI branch of the upstream `client` fixture has no Rust
//! counterpart). The Rust client is async-only, so the upstream sync and async
//! variants both drive the one async API.

use wiremock::ResponseTemplate;

use crate::{
    common::test_client,
    recording::{completed, describe, received, server_answering},
};

const INTERACTION_ID: &str = "test-interaction-id";

async fn check_interactions_paths() {
    let server = server_answering(ResponseTemplate::new(200).set_body_json(completed())).await;
    let client = test_client(server.uri());

    client
        .interactions()
        .get(INTERACTION_ID, &Default::default())
        .await
        .unwrap();
    client.interactions().cancel(INTERACTION_ID).await.unwrap();
    // Upstream answers the DELETE with an empty 200 body.
    let delete_server = server_answering(ResponseTemplate::new(200)).await;
    test_client(delete_server.uri())
        .interactions()
        .delete(INTERACTION_ID)
        .await
        .unwrap();

    let requests: Vec<String> = received(&server).await.iter().map(describe).collect();
    assert_eq!(
        requests,
        [
            format!("GET /v1beta/interactions/{INTERACTION_ID}?stream=false"),
            format!("POST /v1beta/interactions/{INTERACTION_ID}/cancel"),
        ]
    );
    let deletes: Vec<String> = received(&delete_server)
        .await
        .iter()
        .map(describe)
        .collect();
    assert_eq!(
        deletes,
        [format!("DELETE /v1beta/interactions/{INTERACTION_ID}")]
    );
}

// upstream-test: interactions/test_paths.py::test_interactions_paths
#[tokio::test]
async fn test_interactions_paths() {
    check_interactions_paths().await;
}

// upstream-test: interactions/test_paths.py::test_async_interactions_paths
#[tokio::test]
async fn test_async_interactions_paths() {
    check_interactions_paths().await;
}
