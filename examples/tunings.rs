//! Starts a supervised fine-tuning job from a handful of examples and
//! reports the resulting job's state.
//!
//! Requires `GEMINI_API_KEY` (or `GOOGLE_API_KEY`) in the environment, and
//! an API key with tuning access. Run with:
//!
//! ```sh
//! GEMINI_API_KEY=... cargo run --example tunings
//! ```
//!
//! This creates a long-lived tuned model on your project -- unlike the
//! other examples it leaves state behind, so delete the tuned model
//! afterwards via `client.models().delete(..)` if you don't want it.
//!
//! Note: `client.tunings().list(..)` is Vertex-AI-only upstream and returns
//! `Error::UnsupportedByBackend` here; use `client.models().list(..)` with
//! `query_base: Some(false)` to enumerate tuned models instead.

use gemini_genai::Client;
use gemini_genai::types::{CreateTuningJobConfig, TuningDataset, TuningExample};

#[tokio::main]
async fn main() -> gemini_genai::Result<()> {
    let client = Client::new()?;

    let dataset = TuningDataset {
        examples: Some(
            [("1 + 1", "2"), ("3 + 5", "8"), ("10 + 12", "22")]
                .into_iter()
                .map(|(input, output)| TuningExample {
                    text_input: Some(input.to_owned()),
                    output: Some(output.to_owned()),
                })
                .collect(),
        ),
        ..Default::default()
    };

    let job = client
        .tunings()
        .tune(
            "models/gemini-flash-latest",
            dataset,
            Some(CreateTuningJobConfig {
                tuned_model_display_name: Some("example-adder".to_owned()),
                epoch_count: Some(1),
                ..Default::default()
            }),
        )
        .await?;

    println!("tuning job: {:?}", job.name);
    println!("state: {:?}", job.state);
    println!("\nPoll with client.tunings().get(&name, None) until it is ACTIVE.");
    Ok(())
}
