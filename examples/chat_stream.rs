//! A multi-turn chat that streams each reply. The `Chat` keeps the
//! conversation history, so the second question can refer back to the
//! first answer without repeating it.
//!
//! Requires `GEMINI_API_KEY` (or `GOOGLE_API_KEY`) in the environment. Run
//! with:
//!
//! ```sh
//! GEMINI_API_KEY=... cargo run --example chat_stream
//! ```

use std::io::Write;

use futures_util::StreamExt;
use gemini_genai::Client;

#[tokio::main]
async fn main() -> gemini_genai::Result<()> {
    let client = Client::new()?;
    let mut chat = client.chats().create("gemini-flash-latest", None, None);

    for prompt in [
        "Name one planet in our solar system. Answer with just the name.",
        "How many moons does that planet have? One short sentence.",
    ] {
        println!("\n> {prompt}");
        let stream = chat.send_message_stream(prompt, None).await?;
        let mut stream = Box::pin(stream);
        while let Some(chunk) = stream.next().await {
            if let Some(text) = chunk?.text() {
                print!("{text}");
                std::io::stdout().flush().ok();
            }
        }
        println!();
    }

    // More than 4 entries is expected, not a bug: like Python's
    // `Chat.send_message_stream`, each streamed chunk that carries model
    // content is appended to history as its own `Content`, so a reply that
    // arrived in three chunks contributes three model turns.
    println!("\nhistory entries: {}", chat.get_history(false).len());
    Ok(())
}
