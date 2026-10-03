//! Port of `google/genai/tests/models/test_generate_content_part.py`
//! (the plain, non-table tests; table cases are served by the oracle corpus).

use base64::Engine as _;
use gemini_genai::{
    Error,
    types::{Content, File, GenerateContentConfig, Part},
};
use serde_json::{Value, json};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use crate::common::test_client;

const MODEL: &str = "gemini-2.5-flash";
const PNG_BYTES: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const SCONES_URI: &str = "gs://generativeai-downloads/images/scones.jpg";

fn model_reply(text: &str) -> Value {
    json!({"candidates": [{"content": {"role": "model", "parts": [{"text": text}]}, "finishReason": "STOP"}]})
}

fn rejection_reply() -> Value {
    json!({"error": {"code": 400, "message": "unsupported file uri", "status": "INVALID_ARGUMENT"}})
}

async fn mount_reply(server: &MockServer, status: u16, body: Value) {
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}

#[expect(
    clippy::unwrap_used,
    reason = "test helper: a missing or malformed captured request is a test-setup bug"
)]
async fn request_body(server: &MockServer) -> Value {
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1, "exactly one request expected");
    requests[0].body_json().unwrap()
}

fn assert_client_error(result: Result<gemini_genai::types::GenerateContentResponse, Error>) {
    match result {
        Err(Error::Api(api)) => assert_eq!(api.code, 400),
        other => panic!("expected a 400 ClientError, got {other:?}"),
    }
}

/// Sends `contents` (and optional `config`) to a mock that answers `text`, and
/// returns the single request body.
#[expect(
    clippy::unwrap_used,
    reason = "test helper: the mocked call is expected to succeed"
)]
async fn send_ok(
    contents: impl Into<gemini_genai::types::Contents>,
    config: Option<GenerateContentConfig>,
) -> Value {
    let server = MockServer::start().await;
    mount_reply(&server, 200, model_reply("ok")).await;
    test_client(server.uri())
        .models()
        .generate_content(MODEL, contents, config)
        .await
        .unwrap();
    request_body(&server).await
}

/// Sends a `gs://` file part (unsupported on the Gemini Developer API) and
/// returns the request body after asserting the 400 is surfaced.
async fn send_rejected(
    contents: impl Into<gemini_genai::types::Contents>,
    config: Option<GenerateContentConfig>,
) -> Value {
    let server = MockServer::start().await;
    mount_reply(&server, 400, rejection_reply()).await;
    let result = test_client(server.uri())
        .models()
        .generate_content(MODEL, contents, config)
        .await;
    assert_client_error(result);
    request_body(&server).await
}

fn system_instruction(text: &str) -> GenerateContentConfig {
    GenerateContentConfig {
        system_instruction: Some(Content::from(text)),
        ..Default::default()
    }
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// upstream-test: models/test_generate_content_part.py::test_empty_part
#[tokio::test]
async fn test_empty_part() {
    let body = send_ok(vec![Content::from("")], None).await;
    assert_eq!(body["contents"][0]["parts"][0]["text"], "");
}

// upstream-test: models/test_generate_content_part.py::test_image_file
#[tokio::test]
async fn test_image_file() {
    let inline: Part = serde_json::from_value(json!({
        "inline_data": {"data": b64(&PNG_BYTES), "mimeType": "image/png"}
    }))
    .unwrap_or_default();
    let body = send_ok(
        vec![Part::from_text("What is this image about?"), inline],
        None,
    )
    .await;
    let parts = &body["contents"][0]["parts"];
    assert_eq!(parts[1]["inlineData"]["data"], b64(&PNG_BYTES));
}

// upstream-test: models/test_generate_content_part.py::test_from_uri
#[tokio::test]
async fn test_from_uri() {
    let body = send_rejected(
        vec![
            Part::from_text("What is this image about?"),
            Part::from_uri(SCONES_URI, "image/jpeg"),
        ],
        None,
    )
    .await;
    assert_eq!(
        body["contents"][0]["parts"][1]["fileData"]["file_uri"],
        SCONES_URI
    );
}

// upstream-test: models/test_generate_content_part.py::test_user_content_text
#[tokio::test]
async fn test_user_content_text() {
    let body = send_ok(Content::from("why is the sky blue?"), None).await;
    assert_eq!(body["contents"][0]["role"], "user");
    assert_eq!(
        body["contents"][0]["parts"][0]["text"],
        "why is the sky blue?"
    );
}

// upstream-test: models/test_generate_content_part.py::test_user_content_part
#[tokio::test]
async fn test_user_content_part() {
    let content = Content::from(vec![
        Part::from_text("what is this image about?"),
        Part::from_uri(SCONES_URI, "image/jpeg"),
    ]);
    let body = send_rejected(content, None).await;
    assert_eq!(body["contents"][0]["role"], "user");
    assert_eq!(
        body["contents"][0]["parts"][1]["fileData"]["mime_type"],
        "image/jpeg"
    );
}

// upstream-test: models/test_generate_content_part.py::test_model_content_text
#[tokio::test]
async fn test_model_content_text() {
    let contents = vec![
        Content::from(vec![
            Part::from_text("what is this image about?"),
            Part::from_uri(SCONES_URI, "image/jpeg"),
        ]),
        Content {
            role: Some("model".to_owned()),
            parts: Some(vec![Part::from_text(
                "The image is about a cozy breakfast or brunch.",
            )]),
        },
        Content::from("Is this a good environment for a family gathering?"),
    ];
    let body = send_rejected(contents, None).await;
    let roles: Vec<&str> = body["contents"]
        .as_array()
        .map(|items| items.iter().filter_map(|c| c["role"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(roles, ["user", "model", "user"]);
}

// upstream-test: models/test_generate_content_part.py::test_from_uploaded_file_uri
#[tokio::test]
async fn test_from_uploaded_file_uri() {
    let server = MockServer::start().await;
    let upload_url = format!("{}/upload-session/story", server.uri());
    Mock::given(method("POST"))
        .and(wiremock::matchers::path("/upload/v1beta/files"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", upload_url.as_str()),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(wiremock::matchers::path("/upload-session/story"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-status", "final")
                .set_body_json(json!({"file": {
                    "name": "files/story", "mimeType": "text/plain",
                    "uri": "https://generativelanguage.googleapis.com/v1beta/files/story"
                }})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(wiremock::matchers::path(format!(
            "/v1beta/models/{MODEL}:generateContent"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("ok")))
        .expect(3)
        .mount(&server)
        .await;

    let client = test_client(server.uri());
    let source = gemini_genai::files::UploadSource::Bytes {
        data: b"a story".to_vec(),
        mime_type: "text/plain".to_owned(),
    };
    let file = client.files().upload(source, None).await.unwrap();
    let uri = file.uri.clone().unwrap_or_default();
    let mime = file.mime_type.clone().unwrap_or_default();
    let file_part = || Part::from_uri(uri.clone(), mime.clone());

    let models = client.models();
    models
        .generate_content(MODEL, file_part(), None)
        .await
        .unwrap();
    models
        .generate_content(
            MODEL,
            vec![Part::from_text("Summarize this file"), file_part()],
            None,
        )
        .await
        .unwrap();
    models
        .generate_content(
            MODEL,
            vec![Part::from_text("Summarize this file"), file_part()],
            None,
        )
        .await
        .unwrap();
    server.verify().await;
    let generate: Vec<Value> = server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.url.path().ends_with(":generateContent"))
        .filter_map(|r| r.body_json().ok())
        .collect();
    assert_eq!(generate.len(), 3);
    for body in &generate {
        let parts = body["contents"][0]["parts"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let last = &parts[parts.len() - 1];
        assert_eq!(last["fileData"]["file_uri"], uri.as_str());
        assert_eq!(last["fileData"]["mime_type"], "text/plain");
    }
}

// upstream-test: models/test_generate_content_part.py::test_audio_uri
#[tokio::test]
async fn test_audio_uri() {
    let uri = "gs://cloud-samples-data/generative-ai/audio/pixel.mp3";
    let body = send_rejected(
        vec![
            Part::from_text("Provide a summary for the audio."),
            Part::from_uri(uri, "audio/mpeg"),
        ],
        Some(system_instruction(
            "You are a helpful assistant for audio transcription.",
        )),
    )
    .await;
    assert_eq!(body["contents"][0]["parts"][1]["fileData"]["file_uri"], uri);
    assert_eq!(
        body["systemInstruction"]["parts"][0]["text"],
        "You are a helpful assistant for audio transcription."
    );
}

// upstream-test: models/test_generate_content_part.py::test_pdf_uri
#[tokio::test]
async fn test_pdf_uri() {
    let uri = "gs://cloud-samples-data/generative-ai/pdf/2403.05530.pdf";
    let body = send_rejected(
        vec![
            Part::from_text("summarize the pdf in concise and professional tone"),
            Part::from_uri(uri, "application/pdf"),
        ],
        Some(system_instruction(
            "You are a helpful assistant for academic literature review.",
        )),
    )
    .await;
    assert_eq!(
        body["contents"][0]["parts"][1]["fileData"]["mime_type"],
        "application/pdf"
    );
    assert!(body["systemInstruction"].is_object());
}

// upstream-test: models/test_generate_content_part.py::test_video_uri
#[tokio::test]
async fn test_video_uri() {
    let uri = "gs://cloud-samples-data/generative-ai/video/pixel8.mp4";
    let body = send_rejected(
        vec![
            Part::from_text("summarize the video in concise and professional tone."),
            Part::from_uri(uri, "video/mp4"),
        ],
        Some(system_instruction(
            "you are a helpful assistant for market research.",
        )),
    )
    .await;
    assert_eq!(body["contents"][0]["parts"][1]["fileData"]["file_uri"], uri);
    assert!(body["systemInstruction"].is_object());
}

// upstream-test: models/test_generate_content_part.py::test_video_audio_uri
#[tokio::test]
async fn test_video_audio_uri() {
    let video = "gs://cloud-samples-data/generative-ai/video/pixel8.mp4";
    let audio = "gs://cloud-samples-data/generative-ai/audio/pixel.mp3";
    let body = send_rejected(
        vec![
            Part::from_text("Is the audio related to the video?"),
            Part::from_uri(video, "video/mp4"),
            Part::from_uri(audio, "audio/mpeg"),
        ],
        Some(system_instruction(
            "you are a helpful assistant for people with visual and hearing disabilities.",
        )),
    )
    .await;
    let parts = &body["contents"][0]["parts"];
    assert_eq!(parts[1]["fileData"]["file_uri"], video);
    assert_eq!(parts[2]["fileData"]["file_uri"], audio);
}

// upstream-test: models/test_generate_content_part.py::test_from_text
#[tokio::test]
async fn test_from_text() {
    let body = send_ok(vec![Part::from_text("What is your name?")], None).await;
    assert_eq!(
        body["contents"][0]["parts"][0]["text"],
        "What is your name?"
    );
}

/// Shared body of the `test_from_bytes_*` tests.
async fn assert_bytes_part_round_trips(prompt: &str, data: &[u8], mime: &str) {
    let body = send_ok(
        vec![
            Part::from_text(prompt),
            Part::from_bytes(data.to_vec(), mime),
        ],
        None,
    )
    .await;
    let blob = &body["contents"][0]["parts"][1]["inlineData"];
    assert_eq!(blob["data"], b64(data));
    assert_eq!(blob["mime_type"], mime);
}

// upstream-test: models/test_generate_content_part.py::test_from_bytes_image
#[tokio::test]
async fn test_from_bytes_image() {
    assert_bytes_part_round_trips("What is this image about?", &PNG_BYTES, "image/png").await;
}

// upstream-test: models/test_generate_content_part.py::test_from_bytes_image_dict
#[tokio::test]
async fn test_from_bytes_image_dict() {
    let text: Part =
        serde_json::from_value(json!({"text": "What is this image about?"})).unwrap_or_default();
    let inline: Part = serde_json::from_value(json!({
        "inline_data": {"data": b64(&PNG_BYTES), "mimeType": "image/png"}
    }))
    .unwrap_or_default();
    let body = send_ok(vec![text, inline], None).await;
    assert_eq!(
        body["contents"][0]["parts"][0]["text"],
        "What is this image about?"
    );
    assert_eq!(
        body["contents"][0]["parts"][1]["inlineData"]["data"],
        b64(&PNG_BYTES)
    );
}

// upstream-test: models/test_generate_content_part.py::test_from_bytes_image_none
#[tokio::test]
async fn test_from_bytes_image_none() {
    let inline: Part = serde_json::from_value(json!({
        "inline_data": {"data": null, "mimeType": "image/png"}
    }))
    .unwrap_or_default();
    let server = MockServer::start().await;
    mount_reply(&server, 400, rejection_reply()).await;
    let result = test_client(server.uri())
        .models()
        .generate_content(
            MODEL,
            vec![Part::from_text("What is this image about?"), inline],
            None,
        )
        .await;
    match result {
        Err(Error::Api(api)) => {
            assert_eq!(api.code, 400);
            assert_eq!(api.status.as_deref(), Some("INVALID_ARGUMENT"));
        }
        other => panic!("expected INVALID_ARGUMENT, got {other:?}"),
    }
    let body = request_body(&server).await;
    assert!(
        body["contents"][0]["parts"][1]["inlineData"]
            .get("data")
            .is_none()
    );
}

// upstream-test: models/test_generate_content_part.py::test_from_bytes_video
#[tokio::test]
async fn test_from_bytes_video() {
    assert_bytes_part_round_trips(
        "What is this video about?",
        b"\x00\x00\x00\x18ftypmp42",
        "video/mp4",
    )
    .await;
}

// upstream-test: models/test_generate_content_part.py::test_from_bytes_audio
#[tokio::test]
async fn test_from_bytes_audio() {
    assert_bytes_part_round_trips(
        "What is this audio about?",
        b"ID3\x04\x00\x00",
        "audio/mpeg",
    )
    .await;
}

// upstream-test: models/test_generate_content_part.py::test_from_bytes_pdf
#[tokio::test]
async fn test_from_bytes_pdf() {
    assert_bytes_part_round_trips("What is this pdf about?", b"%PDF-1.4\n", "application/pdf")
        .await;
}

// upstream-test: models/test_generate_content_part.py::test_from_function_call_response
#[tokio::test]
async fn test_from_function_call_response() {
    let args = std::collections::HashMap::from([("location".to_owned(), json!("Boston"))]);
    let weather = std::collections::HashMap::from([("weather".to_owned(), json!("sunny"))]);
    let body = send_ok(
        vec![
            Part::from_text("what is the weather in Boston?"),
            Part::from_function_call("get_weather", args),
            Part::from_function_response("get_weather", weather),
        ],
        None,
    )
    .await;
    let contents = body["contents"].as_array().cloned().unwrap_or_default();
    let roles: Vec<&str> = contents.iter().filter_map(|c| c["role"].as_str()).collect();
    assert_eq!(roles, ["user", "model", "user"]);
    assert_eq!(
        contents[1]["parts"][0]["functionCall"]["args"]["location"],
        "Boston"
    );
    assert_eq!(
        contents[2]["parts"][0]["functionResponse"]["response"]["weather"],
        "sunny"
    );
}

// upstream-test: models/test_generate_content_part.py::test_image_base64_stream_async
#[tokio::test]
async fn test_image_base64_stream_async() {
    use futures_util::StreamExt;

    let server = MockServer::start().await;
    let sse = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"a logo\"}]},\"finishReason\":\"STOP\"}]}\n\n";
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse)
                .insert_header("content-type", "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let inline: Part = serde_json::from_value(json!({
        "inline_data": {"data": b64(&PNG_BYTES), "mimeType": "image/png"}
    }))
    .unwrap_or_default();
    let mut stream = test_client(server.uri())
        .models()
        .generate_content_stream(
            MODEL,
            vec![Part::from_text("What is this image about?"), inline],
            None,
        )
        .await
        .unwrap();
    let mut chunks = 0;
    while let Some(chunk) = stream.next().await {
        chunk.unwrap();
        chunks += 1;
    }
    assert_eq!(chunks, 1);
    let body = request_body(&server).await;
    assert_eq!(
        body["contents"][0]["parts"][1]["inlineData"]["data"],
        b64(&PNG_BYTES)
    );
}

const FILE_URI: &str = "https://generativelanguage.googleapis.com/v1beta/files/8q6j6weg80ey";

fn uploaded_file(mime_type: Option<&str>) -> File {
    File {
        uri: Some(FILE_URI.to_owned()),
        mime_type: mime_type.map(str::to_owned),
        ..Default::default()
    }
}

fn assert_file_part(part: &Value) {
    assert_eq!(part["fileData"]["file_uri"], FILE_URI, "part: {part}");
    assert_eq!(part["fileData"]["mime_type"], "text/plain", "part: {part}");
}

// upstream-test: models/test_generate_content_part.py::test_from_file_input
#[tokio::test]
async fn test_from_file_input() {
    let file = uploaded_file(Some("text/plain"));
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(model_reply("ok")))
        .expect(3)
        .mount(&server)
        .await;
    let client = test_client(server.uri());
    let summarize = || Part::from_text("Summarize this file");

    // contents=file
    client
        .models()
        .generate_content(MODEL, Part::from(&file), None)
        .await
        .unwrap();
    // contents=['Summarize this file', file]
    client
        .models()
        .generate_content(MODEL, vec![summarize(), Part::from(&file)], None)
        .await
        .unwrap();
    // contents=[['Summarize this file', file]]
    client
        .models()
        .generate_content(
            MODEL,
            vec![Content::from(vec![summarize(), Part::from(&file)])],
            None,
        )
        .await
        .unwrap();

    let bodies: Vec<Value> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| request.body_json().unwrap())
        .collect();
    assert_eq!(bodies.len(), 3);
    assert_file_part(&bodies[0]["contents"][0]["parts"][0]);
    for body in &bodies[1..] {
        assert_eq!(body["contents"].as_array().map(Vec::len), Some(1));
        let parts = &body["contents"][0]["parts"];
        assert_eq!(parts[0]["text"], "Summarize this file");
        assert_file_part(&parts[1]);
    }
}

// upstream-test: models/test_generate_content_part.py::test_file
#[tokio::test]
async fn test_file() {
    let file = uploaded_file(Some("text/plain"));
    let body = send_ok(
        vec![Part::from_text("Summarize this file"), Part::from(&file)],
        None,
    )
    .await;
    let parts = &body["contents"][0]["parts"];
    assert_eq!(parts[0]["text"], "Summarize this file");
    assert_file_part(&parts[1]);
}

// upstream-test: models/test_generate_content_part.py::test_file_error
#[test]
fn test_file_error() {
    // Missing mime_type: Python raises ValueError while building the request;
    // Rust surfaces the same condition when coercing the File to a Part.
    let result = Part::try_from_file(&uploaded_file(None));
    assert!(
        matches!(&result, Err(Error::Validation(msg)) if msg.contains("mime_type")),
        "got {result:?}"
    );
}
