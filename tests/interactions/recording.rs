//! Helpers shared by the Interactions tests: a catch-all wiremock server and
//! request inspection.

use gemini_genai::{
    Client,
    interactions::{
        CreateInteractionRequestBody, CreateModelInteraction, InteractionsInput, Model,
    },
};
use serde_json::{Value, json};
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::any};

/// The `{"status": "completed"}` interaction every upstream test answers with.
pub fn completed() -> Value {
    json!({"status": "completed"})
}

/// A server answering every request with `template`.
pub async fn server_answering(template: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(template)
        .mount(&server)
        .await;
    server
}

/// The requests the server received, in order.
pub async fn received(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.unwrap_or_default()
}

/// `METHOD path?query` of `request`.
pub fn describe(request: &Request) -> String {
    let target = request.url.query().map_or_else(
        || request.url.path().to_owned(),
        |query| format!("{}?{query}", request.url.path()),
    );
    format!("{} {target}", request.method)
}

/// Value of the request header `name`, if present and valid UTF-8.
pub fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request.headers.get(name).and_then(|v| v.to_str().ok())
}

/// `create(model="gemini-1.5-flash", input="Hello")`, as in the upstream auth tests.
pub async fn create_hello(
    client: &Client,
) -> gemini_genai::Result<gemini_genai::interactions::Interaction> {
    client.interactions().create(&hello_body()).await
}

/// The request body of `create_hello`.
pub fn hello_body() -> CreateInteractionRequestBody {
    CreateInteractionRequestBody::CreateModelInteraction(CreateModelInteraction {
        model: Some(Model::from("gemini-1.5-flash".to_owned())),
        input: Some(InteractionsInput::Text("Hello".to_owned())),
        ..Default::default()
    })
}
