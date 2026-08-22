//! Generates an image with the Imagen `:predict` endpoint and writes it to
//! disk.
//!
//! Requires `GEMINI_API_KEY` (or `GOOGLE_API_KEY`) in the environment, and
//! an API key with Imagen access. Run with:
//!
//! ```sh
//! GEMINI_API_KEY=... cargo run --example generate_images
//! ```
//!
//! `generate_images` is deprecated upstream in favour of generating images
//! through `generate_content` with an image-capable model (e.g.
//! `gemini-3.1-flash-image`), which returns the image as an `inline_data`
//! part. It is kept here because the Python SDK still exposes it.

use gemini_genai::Client;
use gemini_genai::types::GenerateImagesConfig;

#[tokio::main]
async fn main() -> gemini_genai::Result<()> {
    let client = Client::new()?;

    #[expect(
        deprecated,
        reason = "the example exists to demonstrate this deprecated method"
    )]
    let response = client
        .models()
        .generate_images(
            "imagen-4.0-generate-001",
            "A watercolour painting of a lighthouse at dawn",
            Some(GenerateImagesConfig {
                number_of_images: Some(1),
                ..Default::default()
            }),
        )
        .await?;

    let Some(images) = response.generated_images else {
        println!("no images returned (the model may have filtered the prompt)");
        return Ok(());
    };
    for (index, generated) in images.iter().enumerate() {
        let Some(bytes) = generated
            .image
            .as_ref()
            .and_then(|i| i.image_bytes.as_ref())
        else {
            continue;
        };
        let path = format!("generated_image_{index}.png");
        std::fs::write(&path, bytes)?;
        println!("wrote {path} ({} bytes)", bytes.len());
    }
    Ok(())
}
