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
use gemini_genai::{
    Client,
    types::{
        Content, CountTokensConfig, CreateCachedContentConfig, EmbedContentConfig,
        GenerateContentConfig, Part, Tool, UpdateCachedContentConfig,
    },
};

/// A small, currently-served text model. `*-latest` aliases are used
/// deliberately: pinned snapshots (e.g. `gemini-2.5-flash`) get retired for
/// new projects and would make these tests fail for reasons unrelated to
/// this crate.
const TEXT_MODEL: &str = "gemini-flash-latest";
/// The current embedding model.
const EMBED_MODEL: &str = "gemini-embedding-001";
/// An image-capable model driven through `generate_content`.
///
/// Deliberately *not* `models().generate_images` (Imagen's `{model}:predict`):
/// enumerating this key's catalogue via `models().list(..)` shows **no**
/// model advertising `predict` at all (the served generation methods are
/// `generateContent`, `countTokens`, `createCachedContent`,
/// `batchGenerateContent`, `embedContent`, `countTextTokens`,
/// `asyncBatchEmbedContent`, `generateAnswer`, `predictLongRunning` and
/// `bidiGenerateContent`), so `:predict` has nothing to talk to. That
/// matches the upstream deprecation of `generate_images` in favour of
/// `generate_content` with an image-capable model, which this model is:
/// it returns an `inline_data` image part with no `response_modalities`
/// override needed.
const IMAGE_MODEL: &str = "gemini-2.5-flash-image";
/// A model that advertises `createCachedContent`. Explicit caching has a
/// minimum cached-token count, so [`long_cacheable_text`] is sized well
/// past it.
const CACHE_MODEL: &str = TEXT_MODEL;

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
    let tool = Tool::from_function(gemini_genai::afc::function_tool(
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
    use gemini_genai::files::UploadSource;

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

/// Builds a block of text long enough to satisfy explicit caching's
/// minimum cached-token count (this comes out at roughly 4.6k tokens).
/// The per-section readings are distinctive so the model has to consult
/// the cached content to answer the question in
/// [`test_e2e_cached_content_create_and_use`].
fn long_cacheable_text() -> String {
    use std::fmt::Write as _;

    let mut text = String::new();
    for i in 1..=120 {
        // Writing to a `String` is infallible, so the `fmt::Result` is
        // discarded rather than unwrapped.
        let _ = writeln!(
            text,
            "Section {i}: the Aurora Borealis Research Station logged a magnetometer \
             reading of {i} nanotesla at local midnight, with clear skies and no \
             auroral substorm activity."
        );
    }
    text
}

/// Creates a cached content over [`long_cacheable_text`], returning it
/// together with its resource name.
#[expect(
    clippy::expect_used,
    reason = "test helper: a failed cache create/name here is a live-API or test-setup problem the caller wants surfaced as a panic, exactly as if it were inline in the #[test] fn"
)]
async fn create_test_cache(client: &Client) -> (String, gemini_genai::types::CachedContent) {
    let cached = client
        .caches()
        .create(
            CACHE_MODEL,
            Some(CreateCachedContentConfig {
                contents: Some(vec![Content {
                    role: Some("user".to_owned()),
                    parts: Some(vec![Part::from_text(long_cacheable_text())]),
                }]),
                ttl: Some("300s".to_owned()),
                display_name: Some("genai-rs e2e cache".to_owned()),
                ..Default::default()
            }),
        )
        .await
        .expect("caches create failed");
    let name = cached.name.clone().expect("cached content has no name");
    (name, cached)
}

/// US7 scenario 1: an image-generation request returns image bytes and a
/// MIME type.
#[tokio::test]
#[ignore = "calls the live Gemini API; run with --ignored"]
async fn test_e2e_generate_image() {
    let client = client_or_skip!();
    let response = client
        .models()
        .generate_content(
            IMAGE_MODEL,
            "Generate a simple picture of a red circle on a white background.",
            None,
        )
        .await
        .expect("image generate_content failed");

    let images: Vec<_> = response
        .candidates
        .iter()
        .flatten()
        .filter_map(|candidate| candidate.content.as_ref())
        .filter_map(|content| content.parts.as_ref())
        .flatten()
        .filter_map(|part| part.inline_data.as_ref())
        .collect();

    let image = images
        .first()
        .expect("response carried no inline image part");
    let mime = image.mime_type.as_deref().unwrap_or_default();
    assert!(
        mime.starts_with("image/"),
        "inline part was not an image: {mime:?}"
    );
    let bytes = image.data.as_ref().map_or(0, Vec::len);
    assert!(bytes > 0, "image part carried no bytes");
    eprintln!("generated {bytes} bytes of {mime}");
}

/// US6 scenario 3: a cache is created with a name and an expiry, and a
/// generation that references it bills the cached tokens as cached.
#[tokio::test]
#[ignore = "calls the live Gemini API; run with --ignored"]
async fn test_e2e_cached_content_create_and_use() {
    let client = client_or_skip!();
    let (name, cached) = create_test_cache(&client).await;
    assert!(
        cached.expire_time.as_deref().is_some_and(|t| !t.is_empty()),
        "cached content has no expire_time"
    );
    assert!(
        cached
            .usage_metadata
            .and_then(|u| u.total_token_count)
            .unwrap_or(0)
            > 0,
        "cached content reported no cached tokens"
    );

    let response = client
        .models()
        .generate_content(
            CACHE_MODEL,
            "What was the reading in Section 42? Answer with the number only.",
            Some(GenerateContentConfig {
                cached_content: Some(name.clone()),
                ..Default::default()
            }),
        )
        .await
        .expect("generate_content with cached_content failed");

    let cached_tokens = response
        .usage_metadata
        .as_ref()
        .and_then(|u| u.cached_content_token_count)
        .unwrap_or(0);
    assert!(
        cached_tokens > 0,
        "usage metadata did not attribute any tokens to the cache: {:?}",
        response.usage_metadata
    );
    assert!(
        response.text().unwrap_or_default().contains("42"),
        "model did not answer from the cached content: {:?}",
        response.text()
    );

    client
        .caches()
        .delete(&name, None)
        .await
        .expect("caches delete failed");
}

/// US6 scenario 4: updating a cache returns the new expiry, and getting a
/// deleted cache is an error.
#[tokio::test]
#[ignore = "calls the live Gemini API; run with --ignored"]
async fn test_e2e_cached_content_update_and_delete() {
    let client = client_or_skip!();
    let (name, cached) = create_test_cache(&client).await;
    let original_expiry = cached.expire_time.clone().expect("no expire_time");

    let updated = client
        .caches()
        .update(
            &name,
            Some(UpdateCachedContentConfig {
                ttl: Some("900s".to_owned()),
                ..Default::default()
            }),
        )
        .await
        .expect("caches update failed");
    let new_expiry = updated.expire_time.clone().expect("no expire_time");
    assert_ne!(
        new_expiry, original_expiry,
        "expire_time did not move after extending the ttl"
    );

    client
        .caches()
        .delete(&name, None)
        .await
        .expect("caches delete failed");

    // The Gemini API answers a get on a deleted cache with 403, not 404,
    // so this asserts on the error kind rather than a status code.
    let error = client
        .caches()
        .get(&name, None)
        .await
        .expect_err("getting a deleted cache should fail");
    assert!(
        matches!(error, gemini_genai::Error::Api(_)),
        "expected an API error after deletion, got {error:?}"
    );
}
