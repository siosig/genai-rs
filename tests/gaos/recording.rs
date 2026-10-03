//! A wiremock stand-in for the upstream `_RecordingHandler`: it records every
//! request as `"METHOD path?query"` plus its JSON body, and answers each one
//! with a canned JSON payload.

use serde_json::Value;
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::any};

/// Starts a server that answers every request with `payload(method, path_and_query)`.
pub async fn recording_server(
    payload: impl Fn(&str, &str) -> Value + Send + Sync + 'static,
) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(move |request: &Request| {
            ResponseTemplate::new(200)
                .set_body_json(payload(request.method.as_str(), &path_and_query(request)))
        })
        .mount(&server)
        .await;
    server
}

/// `path?query` of `request`.
pub fn path_and_query(request: &Request) -> String {
    request.url.query().map_or_else(
        || request.url.path().to_owned(),
        |query| format!("{}?{query}", request.url.path()),
    )
}

/// Every request the server received, as `"METHOD path?query"`.
pub async fn captured(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .map(|request| format!("{} {}", request.method, path_and_query(request)))
        .collect()
}

/// The JSON bodies of the POST/PATCH/PUT requests the server received, in order.
pub async fn captured_bodies(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| matches!(request.method.as_str(), "POST" | "PATCH" | "PUT"))
        .filter_map(|request| serde_json::from_slice(&request.body).ok())
        .collect()
}
