//! Rust port of the [Google Gen AI Python SDK](https://github.com/googleapis/python-genai)
//! (`google-genai` 2.19.0) for the **Gemini Developer API**.
//!
//! The crate is published as `google-genai-rs` but its library name is
//! `google_genai`, so `cargo add google-genai-rs` is followed by
//! `use google_genai::...`.
//!
//! Vertex AI is out of scope: asking for it (via
//! [`ClientBuilder::vertexai`], `project`/`location`, or the
//! `GOOGLE_GENAI_USE_VERTEXAI` environment variable) fails fast with
//! [`Error::UnsupportedBackend`].
//!
//! # Quickstart
//!
//! ```no_run
//! # async fn run() -> google_genai::Result<()> {
//! use google_genai::Client;
//!
//! let client = Client::new()?;
//! let response = client
//!     .models()
//!     .generate_content("gemini-flash-latest", "Why is the sky blue?", None)
//!     .await?;
//! println!("{}", response.text().unwrap_or_default());
//! # Ok(())
//! # }
//! ```
//!
//! [`Client::new`] reads the API key from `GOOGLE_API_KEY`, falling back to
//! `GEMINI_API_KEY`; [`Client::builder`] sets it (and
//! [`HttpOptions`](types::HttpOptions)) explicitly. `GOOGLE_GEMINI_BASE_URL`
//! overrides the API base URL when it is not set on the builder.
//!
//! # Streaming
//!
//! ```no_run
//! # async fn run() -> google_genai::Result<()> {
//! use futures_util::StreamExt;
//! use google_genai::Client;
//!
//! let client = Client::new()?;
//! let stream = client
//!     .models()
//!     .generate_content_stream("gemini-flash-latest", "Write a haiku.", None)
//!     .await?;
//!
//! let mut stream = Box::pin(stream);
//! while let Some(chunk) = stream.next().await {
//!     print!("{}", chunk?.text().unwrap_or_default());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Structured output
//!
//! [`GenerateContentConfig::with_json_schema_of`](types::GenerateContentConfig::with_json_schema_of)
//! derives the response schema from a plain Rust type via
//! [`schemars`], so the reply parses straight back into it.
//!
//! ```no_run
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use google_genai::Client;
//! use google_genai::types::GenerateContentConfig;
//!
//! #[derive(serde::Deserialize, schemars::JsonSchema)]
//! struct Capital {
//!     country: String,
//!     capital: String,
//! }
//!
//! let client = Client::new()?;
//! let config = GenerateContentConfig::default().with_json_schema_of::<Capital>();
//! let response = client
//!     .models()
//!     .generate_content("gemini-flash-latest", "What is the capital of Japan?", Some(config))
//!     .await?;
//!
//! let capital: Capital = serde_json::from_str(&response.text().unwrap_or_default())?;
//! println!("{} -> {}", capital.country, capital.capital);
//! # Ok(())
//! # }
//! ```
//!
//! # Automatic function calling
//!
//! [`afc::function_tool`] wraps an async Rust function as a model-callable
//! tool; [`Tool::from_function`](types::Tool::from_function) declares it in
//! [`GenerateContentConfig`](types::GenerateContentConfig), and
//! `generate_content` then drives the call/response loop itself. See
//! [`afc`] for the loop's limits and for the process-wide registry caveat
//! (callables are keyed by function name).
//!
//! ```no_run
//! # async fn run() -> google_genai::Result<()> {
//! use google_genai::afc::function_tool;
//! use google_genai::types::{GenerateContentConfig, Tool};
//! use google_genai::Client;
//!
//! #[derive(serde::Deserialize, schemars::JsonSchema)]
//! struct WeatherArgs {
//!     /// The city to look up.
//!     city: String,
//! }
//!
//! let tool = Tool::from_function(function_tool(
//!     "get_weather",
//!     "Returns the current weather for a city.",
//!     |args: WeatherArgs| async move {
//!         Ok(serde_json::json!({ "city": args.city, "temperature_c": 21 }))
//!     },
//! ));
//!
//! let client = Client::new()?;
//! let response = client
//!     .models()
//!     .generate_content(
//!         "gemini-flash-latest",
//!         "What is the weather in Kyoto?",
//!         Some(GenerateContentConfig { tools: Some(vec![tool]), ..Default::default() }),
//!     )
//!     .await?;
//! println!("{}", response.text().unwrap_or_default());
//! # Ok(())
//! # }
//! ```
//!
//! # Modules
//!
//! Every API surface hangs off [`Client`], one accessor per Python SDK
//! module: [`models`], [`chats`], [`files`], [`caches`], [`tunings`],
//! [`batches`], [`operations`], [`file_search_stores`] (plus
//! [`documents`]), [`auth_tokens`], and [`live`]. Request/response types
//! live in [`types`], errors in [`error`], and list endpoints return a
//! [`Pager<T>`](Pager) with `page()`, `next_page().await`, and
//! `into_stream()`.
//!
//! # Cargo features
//!
//! | Feature | Default | Enables |
//! |---|---|---|
//! | `rustls-tls` | yes | TLS via `rustls` + `webpki-roots` (no system OpenSSL) |
//! | `native-tls` | no | TLS via the platform's native stack instead |
//! | `live` | yes | [`live`]: the bidirectional realtime (WebSocket) API |
//! | `blocking` | no | `google_genai::blocking`: a synchronous mirror of this API |
//! | `mcp` | no | `google_genai::mcp`: exposes MCP server tools to the model |
//! | `live-tests` | no | Opts the repo's own network-dependent tests in; not used by library code |
//!
//! # Errors
//!
//! Every fallible call returns [`Result<T>`](Result), whose error is the
//! [`Error`] enum: [`Error::Api`] (boxed [`ApiError`] with `code`,
//! `status`, `message`), [`Error::Http`], [`Error::Json`],
//! [`Error::Validation`], [`Error::UnsupportedByBackend`],
//! [`Error::UnsupportedBackend`], [`Error::FunctionCall`],
//! [`Error::Stream`], [`Error::Upload`], [`Error::NoMorePages`], and
//! [`Error::BlockingInsideRuntime`].
//!
//! ```
//! use google_genai::{ApiError, Error};
//!
//! fn describe(error: &Error) -> String {
//!     match error {
//!         Error::Api(api) if api.code == 429 => "rate limited; back off".to_owned(),
//!         Error::Api(api) => format!("API error {}: {}", api.code, api.message),
//!         Error::Validation(message) => format!("bad request: {message}"),
//!         other => other.to_string(),
//!     }
//! }
//!
//! let error = Error::Api(Box::new(ApiError {
//!     code: 429,
//!     status: Some("RESOURCE_EXHAUSTED".to_owned()),
//!     message: "quota exceeded".to_owned(),
//!     details: Vec::new(),
//!     response_headers: std::collections::HashMap::new(),
//! }));
//! assert_eq!(describe(&error), "rate limited; back off");
//! ```
//!
//! # Further reading
//!
//! - `docs/parity.md` (generated): the full Python-to-Rust method and type
//!   mapping, including what is deliberately not ported.
//! - `docs/migrating-from-python.md`: a migration guide for people coming
//!   from the Python SDK.

pub mod afc;
pub mod auth_tokens;
pub mod batches;
pub mod caches;
pub mod chats;
pub(crate) mod client;
pub mod converters;
pub mod documents;
pub mod error;
pub mod file_search_stores;
pub mod files;
pub(crate) mod http;
#[cfg(feature = "live")]
pub mod live;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod models;
pub mod operations;
pub mod pager;
pub(crate) mod transformers;
pub mod tunings;
pub mod types;

#[cfg(feature = "blocking")]
pub mod blocking;

/// The `google-genai` (Python SDK) release this crate's generated types and
/// wire converters were produced from, and verified against.
///
/// `google-genai` ships frequently. This crate's type surface and
/// request/response converters are a 1:1 mechanical port of one exact
/// upstream release, so this constant is the answer to "which upstream
/// behaviour does this build actually match?" -- useful when diagnosing a
/// field the API now returns that this crate doesn't yet know about.
///
/// The same value is stamped into every generated file's header, pinned in
/// `tools/codegen/requirements.txt`, and recorded under
/// `[package.metadata.upstream]` in `Cargo.toml`;
/// `tools/codegen/upstream.py` is the single source of truth that keeps
/// them in sync and documents the upgrade procedure.
pub const UPSTREAM_GENAI_VERSION: &str = "2.19.0";

pub use client::{Backend, Client, ClientBuilder};
pub use error::{ApiError, Error, Result};
pub use pager::Pager;
