//! Asks Gemini for JSON matching a schema derived from a plain Rust type,
//! then parses the response straight into that type.
//!
//! Requires `GOOGLE_API_KEY` (or `GEMINI_API_KEY`) in the environment. Run
//! with:
//!
//! ```sh
//! GOOGLE_API_KEY=... cargo run --example structured_output
//! ```

use gemini_genai::Client;
use gemini_genai::types::GenerateContentConfig;

/// The shape we want Gemini's response to conform to. `schemars::JsonSchema`
/// lets [`GenerateContentConfig::with_json_schema_of`] derive a JSON Schema
/// from it directly, and `serde::Deserialize` lets us parse the response
/// text straight back into it.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct RecipeIdea {
    /// The recipe's name.
    name: String,
    /// Roughly how long the recipe takes, in minutes.
    minutes: u32,
    /// The main ingredients.
    ingredients: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new()?;

    // `with_json_schema_of::<T>()` sets `response_json_schema` from `T`'s
    // derived schema and defaults `response_mime_type` to
    // "application/json". Building a `Schema` by hand and setting
    // `response_schema` instead works too, for full control over the
    // wire schema (see `data-model.md`).
    let config = GenerateContentConfig::default().with_json_schema_of::<RecipeIdea>();

    let response = client
        .models()
        .generate_content(
            "gemini-flash-latest",
            "Suggest a quick weeknight pasta recipe.",
            Some(config),
        )
        .await?;

    let text = response.text().unwrap_or_default();
    let recipe: RecipeIdea = serde_json::from_str(&text)?;
    println!(
        "{} ({} min): {}",
        recipe.name,
        recipe.minutes,
        recipe.ingredients.join(", ")
    );
    Ok(())
}
