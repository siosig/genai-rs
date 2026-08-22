//! Submits a batch job: many prompts queued for offline processing at a
//! lower price than interactive calls. Turnaround is measured in hours, so
//! this example creates the job, reports its state, and cancels it rather
//! than waiting.
//!
//! Requires `GEMINI_API_KEY` (or `GOOGLE_API_KEY`) in the environment. Run
//! with:
//!
//! ```sh
//! GEMINI_API_KEY=... cargo run --example batches
//! ```

use gemini_genai::{
    Client,
    types::{BatchJobSource, Content, CreateBatchJobConfig, InlinedRequest, Part},
};

const MODEL: &str = "gemini-flash-latest";

#[tokio::main]
async fn main() -> gemini_genai::Result<()> {
    let client = Client::new()?;

    let requests: Vec<InlinedRequest> = ["Why is the sky blue?", "Why is the sea salty?"]
        .into_iter()
        .map(|prompt| InlinedRequest {
            model: Some(format!("models/{MODEL}")),
            contents: Some(vec![Content {
                role: Some("user".to_owned()),
                parts: Some(vec![Part::from_text(prompt)]),
            }]),
            ..Default::default()
        })
        .collect();

    let job = client
        .batches()
        .create(
            MODEL,
            BatchJobSource {
                inlined_requests: Some(requests),
                ..Default::default()
            },
            Some(CreateBatchJobConfig {
                display_name: Some("example-batch".to_owned()),
                ..Default::default()
            }),
        )
        .await?;

    let name = job.name.clone().unwrap_or_default();
    println!("created {name}");

    let fetched = client.batches().get(&name, None).await?;
    println!("state: {:?}", fetched.state);

    // A real job would be polled until it reaches a terminal state; this
    // example cancels so it doesn't keep consuming quota after exiting.
    client.batches().cancel(&name, None).await?;
    println!("cancelled {name}");
    Ok(())
}
