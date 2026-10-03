//! Helpers shared by the AFC loop tests that run against a mock server.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use gemini_genai::{
    FunctionTool, function_tool,
    types::{AutomaticFunctionCallingConfig, GenerateContentConfig, Tool},
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

/// Arguments of the weather tools below (upstream's
/// `get_current_weather(location: str)`).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LocationArgs {
    /// Declares the argument the model passes; the tools only count calls.
    #[expect(
        dead_code,
        reason = "present for the declared schema; the counting tools ignore the value"
    )]
    pub location: String,
}

/// A tool named `name` that answers `result` and counts its invocations.
pub fn counting_tool(
    name: &str,
    result: &'static str,
) -> (Arc<dyn FunctionTool>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let tool = function_tool::<LocationArgs, _, _, _>(
        name,
        "Returns the current weather.",
        move |_args: LocationArgs| {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(result)
            }
        },
    );
    (tool, calls)
}

/// `GenerateContentConfig` with `tool` registered and a request budget.
pub fn config_with_tool(
    tool: Arc<dyn FunctionTool>,
    maximum_remote_calls: Option<i64>,
) -> GenerateContentConfig {
    GenerateContentConfig {
        tools: Some(vec![Tool::from_function(tool)]),
        automatic_function_calling: maximum_remote_calls.map(|max| {
            AutomaticFunctionCallingConfig {
                maximum_remote_calls: Some(max),
                ..Default::default()
            }
        }),
        ..Default::default()
    }
}

/// A candidate holding one `functionCall` part.
pub fn function_call_candidate(name: &str, args: &Value) -> Value {
    json!({
        "content": {"role": "model", "parts": [{"functionCall": {"name": name, "args": args}}]}
    })
}

/// A candidate holding one text part.
pub fn text_candidate(text: &str) -> Value {
    json!({"content": {"role": "model", "parts": [{"text": text}]}})
}

/// A unary response body with one candidate.
pub fn unary_reply(candidate: &Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"candidates": [candidate]}))
}

/// A streamed response whose chunks each carry one candidate.
pub fn sse_reply(candidates: &[Value]) -> ResponseTemplate {
    let body: String = candidates
        .iter()
        .map(|candidate| {
            [
                "data: ",
                &json!({"candidates": [candidate]}).to_string(),
                "\n\n",
            ]
            .concat()
        })
        .collect();
    ResponseTemplate::new(200)
        .set_body_string(body)
        .insert_header("content-type", "text/event-stream")
}

/// Mounts `response` for exactly the next unanswered POST; mocks mounted in
/// sequence answer successive requests in order.
pub async fn mount_next(server: &MockServer, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .respond_with(response)
        .up_to_n_times(1)
        .expect(1)
        .mount(server)
        .await;
}

/// Mounts `response` for every POST.
pub async fn mount_always(server: &MockServer, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .respond_with(response)
        .mount(server)
        .await;
}

/// How many requests `server` has received.
#[expect(
    clippy::unwrap_used,
    reason = "test helper: request recording is enabled on every MockServer"
)]
pub async fn request_count(server: &MockServer) -> usize {
    server.received_requests().await.unwrap().len()
}

/// The JSON bodies of the requests `server` has received, oldest first.
#[expect(
    clippy::unwrap_used,
    reason = "test helper: request recording is enabled and every body is JSON"
)]
pub async fn request_bodies(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| request.body_json().unwrap())
        .collect()
}
