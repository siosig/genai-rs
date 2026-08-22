//! Automatic function calling (AFC): register a plain async Rust function
//! as a tool the model can call, and let `generate_content` drive the
//! request/response loop that invokes it.
//!
//! Requires `GOOGLE_API_KEY` (or `GEMINI_API_KEY`) in the environment.
//!
//! ```sh
//! GOOGLE_API_KEY=... cargo run --example function_calling
//! ```

use gemini_genai::{
    Client, Result,
    afc::function_tool,
    types::{GenerateContentConfig, Tool},
};
use schemars::JsonSchema;
use serde::Deserialize;

/// Arguments the model supplies when it calls `get_weather`.
#[derive(Debug, Deserialize, JsonSchema)]
struct GetWeatherArgs {
    /// The city to look up, e.g. "Tokyo".
    location: String,
}

/// A pretend weather lookup -- swap this out for a real API call.
async fn get_weather(args: GetWeatherArgs) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "location": args.location,
        "condition": "sunny",
        "temperature_celsius": 24,
    }))
}

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::new()?;

    let weather_tool = function_tool(
        "get_weather",
        "Gets the current weather for a city.",
        get_weather,
    );
    let config = GenerateContentConfig {
        tools: Some(vec![Tool::from_function(weather_tool)]),
        ..Default::default()
    };

    let response = client
        .models()
        .generate_content(
            "gemini-flash-latest",
            "What's the weather like in Tokyo right now?",
            Some(config),
        )
        .await?;

    println!("{}", response.text().unwrap_or_default());

    if let Some(history) = &response.automatic_function_calling_history {
        println!(
            "\n({} automatic function call round(s) happened)",
            history.len() / 2
        );
    }

    Ok(())
}
