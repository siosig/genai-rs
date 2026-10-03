//! Shared helpers for the plain-function ports of `models/test_*.py`
//! (list / get / update / delete / count / embed / videos / streaming).
//! Written to propagate failures with `?` so no helper needs `unwrap`.

use std::error::Error;

use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::method};

pub type TestResult = Result<(), Box<dyn Error>>;

/// Mounts a catch-all mock for `verb` that answers `status` with `body`.
pub async fn mount_json(server: &MockServer, verb: &str, status: u16, body: &serde_json::Value) {
    Mock::given(method(verb))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

/// Every request the mock server has received so far.
pub async fn received(server: &MockServer) -> Result<Vec<Request>, Box<dyn Error>> {
    server
        .received_requests()
        .await
        .ok_or_else(|| "request recording is disabled".into())
}

/// The Google API error envelope for `code`.
pub fn error_body(code: u16, status: &str) -> serde_json::Value {
    serde_json::json!({"error": {"code": code, "message": "not found", "status": status}})
}
