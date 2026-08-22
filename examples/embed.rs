//! Generates an embedding vector for a piece of text -- the first step in
//! most retrieval / semantic-search pipelines.
//!
//! Requires `GEMINI_API_KEY` (or `GOOGLE_API_KEY`) in the environment. Run
//! with:
//!
//! ```sh
//! GEMINI_API_KEY=... cargo run --example embed
//! ```

use gemini_genai::{Client, types::EmbedContentConfig};

#[tokio::main]
async fn main() -> gemini_genai::Result<()> {
    let client = Client::new()?;

    let response = client
        .models()
        .embed_content(
            "gemini-embedding-001",
            "The quick brown fox jumps over the lazy dog.",
            Some(EmbedContentConfig {
                // Shorter vectors cost less to store and compare; omit this
                // to get the model's native dimensionality.
                output_dimensionality: Some(256),
                ..Default::default()
            }),
        )
        .await?;

    let values = response
        .embeddings
        .as_ref()
        .and_then(|e| e.first())
        .and_then(|e| e.values.as_ref())
        .map(Vec::as_slice)
        .unwrap_or_default();

    println!("dimensions: {}", values.len());
    println!("first 5: {:?}", &values[..values.len().min(5)]);
    Ok(())
}
