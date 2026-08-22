//! Streams a response chunk by chunk instead of waiting for the whole
//! answer, so text can be shown as it is produced.
//!
//! Requires `GEMINI_API_KEY` (or `GOOGLE_API_KEY`) in the environment. Run
//! with:
//!
//! ```sh
//! GEMINI_API_KEY=... cargo run --example generate_content_stream
//! ```

use std::io::Write;

use futures_util::StreamExt;
use gemini_genai::Client;

#[tokio::main]
async fn main() -> gemini_genai::Result<()> {
    let client = Client::new()?;

    let stream = client
        .models()
        .generate_content_stream(
            "gemini-flash-latest",
            "In three sentences, explain why the sky is blue.",
            None,
        )
        .await?;

    let mut stream = Box::pin(stream);
    while let Some(chunk) = stream.next().await {
        if let Some(text) = chunk?.text() {
            print!("{text}");
            // Chunks arrive mid-line, so flush to show them as they land.
            std::io::stdout().flush().ok();
        }
    }
    println!();
    Ok(())
}
