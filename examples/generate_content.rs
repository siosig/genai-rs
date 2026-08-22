//! The smallest possible Gemini call: one prompt in, one answer out.
//!
//! Requires `GEMINI_API_KEY` (or `GOOGLE_API_KEY`) in the environment. Run
//! with:
//!
//! ```sh
//! GEMINI_API_KEY=... cargo run --example generate_content
//! ```

use gemini_genai::Client;

#[tokio::main]
async fn main() -> gemini_genai::Result<()> {
    let client = Client::new()?;

    let response = client
        .models()
        .generate_content("gemini-flash-latest", "Why is the sky blue?", None)
        .await?;

    println!("{}", response.text().unwrap_or_default());

    if let Some(usage) = response.usage_metadata {
        println!(
            "\n({} prompt + {} response = {} tokens)",
            usage.prompt_token_count.unwrap_or(0),
            usage.candidates_token_count.unwrap_or(0),
            usage.total_token_count.unwrap_or(0),
        );
    }
    Ok(())
}
