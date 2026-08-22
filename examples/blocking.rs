//! The same call as `generate_content.rs`, but through the synchronous
//! `blocking` wrapper -- no `async fn`, no runtime to set up.
//!
//! Requires the `blocking` feature and `GEMINI_API_KEY` (or
//! `GOOGLE_API_KEY`) in the environment. Run with:
//!
//! ```sh
//! GEMINI_API_KEY=... cargo run --features blocking --example blocking
//! ```
//!
//! Note: `blocking::Client` owns its own Tokio runtime, so building or
//! calling one from inside an existing async runtime returns
//! `Error::BlockingInsideRuntime` rather than panicking.

fn main() -> gemini_genai::Result<()> {
    let client = gemini_genai::blocking::Client::new()?;

    let response =
        client
            .models()
            .generate_content("gemini-flash-latest", "Why is the sky blue?", None)?;
    println!("{}", response.text().unwrap_or_default());

    // Streams become plain iterators in the blocking API.
    let stream = client.models().generate_content_stream(
        "gemini-flash-latest",
        "Name three primary colors, comma separated.",
        None,
    )?;
    print!("\nstreamed: ");
    for chunk in stream {
        if let Some(text) = chunk?.text() {
            print!("{text}");
        }
    }
    println!();
    Ok(())
}
