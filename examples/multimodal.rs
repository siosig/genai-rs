//! Sends a text prompt alongside inline image bytes to Gemini.
//!
//! Requires `GOOGLE_API_KEY` (or `GEMINI_API_KEY`) in the environment. Run
//! with:
//!
//! ```sh
//! GOOGLE_API_KEY=... cargo run --example multimodal
//! ```

use google_genai::Client;
use google_genai::types::Part;

/// A 1x1 red PNG pixel, so this example needs no external image file.
const RED_PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new()?;

    // `Part::from_bytes` for data already in memory; `Part::from_file_bytes`
    // reads a local file and infers its MIME type from the extension;
    // `Part::from_uri` references a file already uploaded to the API (or
    // any other URI-addressable resource) by URI instead of by value.
    let contents = vec![
        Part::from_text("What color is this image? Answer in one word."),
        Part::from_bytes(RED_PIXEL_PNG, "image/png"),
    ];

    let response = client
        .models()
        .generate_content("gemini-flash-latest", contents, None)
        .await?;

    println!("{}", response.text().unwrap_or_default());
    Ok(())
}
