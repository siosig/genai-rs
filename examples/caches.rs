//! Caches a large block of context once, then answers several questions
//! against it. Cached input tokens are billed at a lower rate, so this
//! pays off when the same context is reused across requests.
//!
//! Requires `GEMINI_API_KEY` (or `GOOGLE_API_KEY`) in the environment. Run
//! with:
//!
//! ```sh
//! GEMINI_API_KEY=... cargo run --example caches
//! ```
//!
//! Note: context caching enforces a per-model minimum token count, so the
//! context below is deliberately long. A `400` here usually means the
//! content was still under that minimum.

use gemini_genai::{
    Client,
    types::{Content, CreateCachedContentConfig, GenerateContentConfig, Part},
};

const MODEL: &str = "gemini-flash-latest";

#[tokio::main]
async fn main() -> gemini_genai::Result<()> {
    let client = Client::new()?;

    // Repeated so the cached content clears the model's minimum token count.
    let document = "Ada Lovelace (1815-1852) was an English mathematician who worked with \
        Charles Babbage on the Analytical Engine, and is credited with publishing the first \
        algorithm intended to be carried out by a machine. "
        .repeat(400);

    let cache = client
        .caches()
        .create(
            MODEL,
            Some(CreateCachedContentConfig {
                display_name: Some("lovelace-notes".to_owned()),
                ttl: Some("600s".to_owned()),
                contents: Some(vec![Content {
                    role: Some("user".to_owned()),
                    parts: Some(vec![Part::from_text(document)]),
                }]),
                ..Default::default()
            }),
        )
        .await?;

    let cache_name = cache.name.clone().unwrap_or_default();
    println!("cached as {cache_name}");

    let response = client
        .models()
        .generate_content(
            MODEL,
            "In one sentence, who is the document about?",
            Some(GenerateContentConfig {
                cached_content: Some(cache_name.clone()),
                ..Default::default()
            }),
        )
        .await?;
    println!("{}", response.text().unwrap_or_default());

    if let Some(usage) = response.usage_metadata {
        println!(
            "(cached tokens reused: {})",
            usage.cached_content_token_count.unwrap_or(0)
        );
    }

    client.caches().delete(&cache_name, None).await?;
    println!("deleted {cache_name}");
    Ok(())
}
