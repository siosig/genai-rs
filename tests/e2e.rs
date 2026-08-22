//! End-to-end acceptance tests against the **live** Gemini Developer API.
//!
//! Every test here is `#[ignore]`d, so a plain `cargo test` never spends
//! quota or requires credentials. Run them explicitly:
//!
//! ```sh
//! GEMINI_API_KEY=... cargo test --all-features --test e2e -- --ignored
//! ```
//!
//! `Client::new()` resolves `GOOGLE_API_KEY` first and falls back to
//! `GEMINI_API_KEY`, so either variable works; the tests skip (rather than
//! fail) when neither is set, so `--ignored` stays runnable in CI without
//! secrets.
//!
//! Expensive/long-running surfaces (video generation, tuning, batches,
//! Live) live in `tests/e2e_expensive.rs` behind a second opt-in flag.

use futures_util::StreamExt;
use google_genai::Client;
use google_genai::types::{
    Content, CountTokensConfig, EmbedContentConfig, GenerateContentConfig, Part, Tool,
};

/// A small, currently-served text model. `*-latest` aliases are used
/// deliberately: pinned snapshots (e.g. `gemini-2.5-flash`) get retired for
/// new projects and would make these tests fail for reasons unrelated to
/// this crate.
const TEXT_MODEL: &str = "gemini-flash-latest";
/// The current embedding model.
const EMBED_MODEL: &str = "gemini-embedding-001";

/// Builds a live client, or returns `None` when no API key is configured
/// (in which case the caller skips instead of failing).
fn live_client() -> Option<Client> {
    if std::env::var("GOOGLE_API_KEY").is_err() && std::env::var("GEMINI_API_KEY").is_err() {
        eprintln!("skipping: neither GOOGLE_API_KEY nor GEMINI_API_KEY is set");
        return None;
    }
    match Client::new() {
        Ok(client) => Some(client),
        Err(error) => panic!("building a live client failed: {error}"),
    }
}

/// Expands to `let Some(client) = live_client() else { return; };`.
macro_rules! client_or_skip {
    () => {
        match live_client() {
            Some(client) => client,
            None => return,
        }
    };
}

/// A 1x1 opaque PNG, used as the smallest valid multimodal image input.
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// US1: single-shot text generation returns text and usage metadata.
#[tokio::test]
#[ignore = "calls the live Gemini API; run with --ignored"]
async fn test_e2e_generate_content() {
    let client = client_or_skip!();
    let response = client
        .models()
        .generate_content(TEXT_MODEL, "Reply with exactly: OK", None)
        .await
        .expect("generate_content failed");

    let text = response.text().expect("response had no text");
    assert!(!text.trim().is_empty(), "text was blank: {text:?}");
    let usage = response.usage_metadata.expect("no usage metadata");
    assert!(
        usage.total_token_count.unwrap_or(0) > 0,
        "total_token_count was not positive: {usage:?}"
    );
}

/// US1: streaming yields multiple chunks that concatenate into the answer.
#[tokio::test]
#[ignore = "calls the live Gemini API; run with --ignored"]
async fn test_e2e_generate_content_stream() {
    let client = client_or_skip!();
    let stream = client
        .models()
        .generate_content_stream(TEXT_MODEL, "Count from 1 to 5, one number per line.", None)
        .await
        .expect("generate_content_stream failed");

    let mut stream = Box::pin(stream);
    let mut chunks = 0_usize;
    let mut combined = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("stream yielded an error");
        chunks += 1;
        if let Some(text) = chunk.text() {
            combined.push_str(&text);
        }
    }
    assert!(chunks > 0, "stream produced no chunks");
    assert!(
        combined.contains('1') && combined.contains('5'),
        "streamed text missing expected digits: {combined:?}"
    );
}

/// US2: an inline image part is accepted alongside text (multimodal input).
#[tokio::test]
#[ignore = "calls the live Gemini API; run with --ignored"]
async fn test_e2e_multimodal_inline_image() {
    let client = client_or_skip!();
    let contents = vec![Content {
        role: Some("user".to_owned()),
        parts: Some(vec![
            Part::from_text("Answer in one word: what shape is this image?"),
            Part::from_bytes(TINY_PNG.to_vec(), "image/png"),
        ]),
    }];
    let response = client
        .models()
        .generate_content(TEXT_MODEL, contents, None)
        .await
        .expect("multimodal generate_content failed");
    assert!(
        response.text().is_some_and(|t| !t.trim().is_empty()),
        "multimodal response had no text"
    );
}

/// US2: `with_json_schema_of::<T>()` yields JSON that deserializes into `T`.
#[tokio::test]
#[ignore = "calls the live Gemini API; run with --ignored"]
async fn test_e2e_structured_output() {
    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[expect(
        dead_code,
        reason = "fields exist to prove the model's JSON deserializes into this shape"
    )]
    struct Capital {
        country: String,
        capital: String,
    }

    let client = client_or_skip!();
    let config = GenerateContentConfig::default().with_json_schema_of::<Capital>();
    let response = client
        .models()
        .generate_content(TEXT_MODEL, "What is the capital of Japan?", Some(config))
        .await
        .expect("structured-output generate_content failed");

    let text = response.text().expect("no text in structured response");
    let parsed: Capital = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("not valid JSON for Capital: {e}\n{text}"));
    assert!(!parsed.capital.trim().is_empty());
}

/// US3: a chat replays history, so the model can resolve a back-reference.
#[tokio::test]
#[ignore = "calls the live Gemini API; run with --ignored"]
async fn test_e2e_chat_multi_turn() {
    let client = client_or_skip!();
    let mut chat = client.chats().create(TEXT_MODEL, None, None);

    chat.send_message("My favourite colour is teal. Remember it.", None)
        .await
        .expect("first send_message failed");
    let second = chat
        .send_message(
            "What is my favourite colour? Answer with the colour only.",
            None,
        )
        .await
        .expect("second send_message failed");

    let text = second.text().unwrap_or_default().to_lowercase();
    assert!(
        text.contains("teal"),
        "model did not recall history; got: {text:?}"
    );
    // 2 user turns + 2 model turns.
    assert_eq!(chat.get_history(false).len(), 4);
}

/// US4: an AFC-registered Rust function is invoked automatically and its
/// result reaches the final answer.
#[tokio::test]
#[ignore = "calls the live Gemini API; run with --ignored"]
async fn test_e2e_automatic_function_calling() {
    #[derive(serde::Deserialize, schemars::JsonSchema)]
    struct WeatherArgs {
        /// The city to look up.
        city: String,
    }

    let client = client_or_skip!();
    let tool = Tool::from_function(google_genai::afc::function_tool(
        "get_weather",
        "Returns the current weather for a city.",
        |args: WeatherArgs| async move {
            Ok(serde_json::json!({
                "city": args.city,
                "temperature_c": 21,
                "condition": "sunny",
            }))
        },
    ));

    let config = GenerateContentConfig {
        tools: Some(vec![tool]),
        ..Default::default()
    };
    let response = client
        .models()
        .generate_content(
            TEXT_MODEL,
            "What is the weather in Kyoto? Use the tool, then state the temperature.",
            Some(config),
        )
        .await
        .expect("AFC generate_content failed");

    let text = response.text().unwrap_or_default();
    assert!(
        text.contains("21"),
        "final answer did not incorporate the tool result: {text:?}"
    );
    assert!(
        response
            .automatic_function_calling_history
            .as_ref()
            .is_some_and(|h| !h.is_empty()),
        "automatic_function_calling_history was empty"
    );
}

/// US5: embeddings come back with a non-empty vector.
#[tokio::test]
#[ignore = "calls the live Gemini API; run with --ignored"]
async fn test_e2e_embed_content() {
    let client = client_or_skip!();
    let response = client
        .models()
        .embed_content(
            EMBED_MODEL,
            "The quick brown fox.",
            Some(EmbedContentConfig {
                output_dimensionality: Some(256),
                ..Default::default()
            }),
        )
        .await
        .expect("embed_content failed");

    let embeddings = response.embeddings.expect("no embeddings returned");
    let values = embeddings
        .first()
        .and_then(|e| e.values.as_ref())
        .expect("first embedding had no values");
    assert_eq!(values.len(), 256, "unexpected embedding dimensionality");
}

/// US5: token counting returns a positive count.
#[tokio::test]
#[ignore = "calls the live Gemini API; run with --ignored"]
async fn test_e2e_count_tokens() {
    let client = client_or_skip!();
    let response = client
        .models()
        .count_tokens(
            TEXT_MODEL,
            "How many tokens is this sentence?",
            None::<CountTokensConfig>,
        )
        .await
        .expect("count_tokens failed");
    assert!(
        response.total_tokens.unwrap_or(0) > 0,
        "total_tokens was not positive: {response:?}"
    );
}

/// US5: `models().list()` pages through the real catalogue.
#[tokio::test]
#[ignore = "calls the live Gemini API; run with --ignored"]
async fn test_e2e_models_list() {
    let client = client_or_skip!();
    let pager = client
        .models()
        .list(None)
        .await
        .expect("models list failed");
    let mut stream = Box::pin(pager.into_stream());

    let mut count = 0_usize;
    let mut saw_a_gemini = false;
    while let Some(model) = stream.next().await {
        let model = model.expect("paging failed");
        count += 1;
        if model.name.as_deref().is_some_and(|n| n.contains("gemini")) {
            saw_a_gemini = true;
        }
    }
    assert!(count > 0, "no models listed");
    assert!(saw_a_gemini, "catalogue contained no gemini model");
}

/// US6: upload -> get -> use in a prompt -> delete, over the resumable
/// upload protocol.
#[tokio::test]
#[ignore = "calls the live Gemini API; run with --ignored"]
async fn test_e2e_files_upload_get_delete() {
    use google_genai::files::UploadSource;

    let client = client_or_skip!();
    let uploaded = client
        .files()
        .upload(
            UploadSource::Bytes {
                data: b"Sphinx of black quartz, judge my vow.".to_vec(),
                mime_type: "text/plain".to_owned(),
            },
            None,
        )
        .await
        .expect("files upload failed");

    let name = uploaded.name.clone().expect("uploaded file has no name");
    let fetched = client
        .files()
        .get(&name, None)
        .await
        .expect("files get failed");
    assert_eq!(fetched.name.as_deref(), Some(name.as_str()));

    client
        .files()
        .delete(&name, None)
        .await
        .expect("files delete failed");
}
