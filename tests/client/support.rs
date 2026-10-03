//! Helpers shared by the client tests: a mock server that answers model
//! lookups, one request through the public API, and request inspection.

use gemini_genai::{
    Client, Error,
    types::{GetModelConfig, HttpOptions},
};
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{method, path_regex},
};

/// Model name used by every request these tests issue.
pub const MODEL: &str = "gemini-2.5-flash";

/// Starts a server that answers `GET .../models/<anything>` with a model.
pub async fn model_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"/models/[^/]+$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": format!("models/{MODEL}"),
        })))
        .mount(&server)
        .await;
    server
}

/// Issues `models().get(MODEL)`, optionally with per-request `http_options`.
pub async fn get_model(client: &Client, http_options: Option<HttpOptions>) -> Result<(), Error> {
    let config = http_options.map(|options| GetModelConfig {
        http_options: Some(options),
    });
    client.models().get(MODEL, config).await.map(|_| ())
}

/// The requests the server has received so far.
#[expect(
    clippy::unwrap_used,
    reason = "test helper: request recording is always enabled on a wiremock server"
)]
pub async fn received(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.unwrap()
}

/// The value of header `name` on `request`, if present and valid UTF-8.
pub fn header_value(request: &Request, name: &str) -> Option<String> {
    request
        .headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}
