//! Opens a Live API session over WebSocket, sends one turn, and drains the
//! model's streamed reply.
//!
//! Requires the `live` feature (on by default) and `GEMINI_API_KEY` (or
//! `GOOGLE_API_KEY`) in the environment. Run with:
//!
//! ```sh
//! GEMINI_API_KEY=... cargo run --example live_text
//! ```
//!
//! `AUDIO` is requested rather than `TEXT`: the currently served
//! `*-live-preview` models are audio-native and close the connection on
//! `responseModalities: [TEXT]`. The reply therefore arrives as PCM audio
//! in `inline_data` parts, which this example simply counts; transcripts,
//! when the model provides them, arrive as `output_transcription`.

use futures_util::StreamExt;
use google_genai::Client;
use google_genai::types::{Content, LiveConnectConfig, Modality, Part};

#[tokio::main]
async fn main() -> google_genai::Result<()> {
    let client = Client::new()?;

    let mut session = Box::pin(client.live().connect(
        "gemini-3.1-flash-live-preview",
        Some(LiveConnectConfig {
            response_modalities: Some(vec![Modality::Audio]),
            ..Default::default()
        }),
    ))
    .await?;
    println!("connected; setup complete");

    session
        .send_client_content(
            Some(vec![Content {
                role: Some("user".to_owned()),
                parts: Some(vec![Part::from_text("Say hello in one short sentence.")]),
            }]),
            true,
        )
        .await?;

    let mut audio_bytes = 0_usize;
    let mut transcript = String::new();
    {
        let mut messages = Box::pin(session.receive());
        while let Some(message) = messages.next().await {
            let message = message?;
            let Some(content) = message.server_content else {
                continue;
            };
            if let Some(text) = content
                .output_transcription
                .as_ref()
                .and_then(|t| t.text.as_ref())
            {
                transcript.push_str(text);
            }
            for part in content
                .model_turn
                .iter()
                .flat_map(|turn| turn.parts.iter().flatten())
            {
                if let Some(blob) = &part.inline_data {
                    audio_bytes += blob.data.as_ref().map_or(0, Vec::len);
                }
            }
            if content.turn_complete == Some(true) {
                break;
            }
        }
    }

    println!("received {audio_bytes} bytes of audio");
    if !transcript.is_empty() {
        println!("transcript: {transcript}");
    }

    session.close().await?;
    println!("closed");
    Ok(())
}
