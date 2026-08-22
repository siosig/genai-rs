//! Starts a video generation job and polls the long-running operation
//! until it finishes.
//!
//! Requires `GEMINI_API_KEY` (or `GOOGLE_API_KEY`) in the environment.
//! Video generation takes minutes and consumes meaningful quota. Run with:
//!
//! ```sh
//! GEMINI_API_KEY=... cargo run --example generate_videos
//! ```

use gemini_genai::{Client, types::GenerateVideosSource};

#[tokio::main]
async fn main() -> gemini_genai::Result<()> {
    let client = Client::new()?;

    let mut operation = client
        .models()
        .generate_videos(
            "veo-3.1-fast-generate-preview",
            GenerateVideosSource {
                prompt: Some("A close-up of a drop of water falling into a still pond".to_owned()),
                ..Default::default()
            },
            None,
        )
        .await?;
    println!("started {}", operation.name.clone().unwrap_or_default());

    while operation.done != Some(true) {
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        operation = client.operations().get(&operation).await?;
        println!("  polling... done={:?}", operation.done);
    }

    if let Some(error) = &operation.error {
        println!("generation failed: {error:?}");
        return Ok(());
    }
    for video in operation
        .response
        .and_then(|r| r.generated_videos)
        .unwrap_or_default()
    {
        if let Some(uri) = video.video.and_then(|v| v.uri) {
            println!("video: {uri}");
        }
    }
    Ok(())
}
