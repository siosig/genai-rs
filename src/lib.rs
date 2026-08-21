//! Rust port of the [Google Gen AI Python SDK](https://github.com/googleapis/python-genai)
//! (`google-genai`) for the Gemini Developer API.
//!
//! ```no_run
//! # async fn run() -> google_genai::Result<()> {
//! use google_genai::Client;
//!
//! let client = Client::new()?;
//! let response = client
//!     .models()
//!     .generate_content("gemini-2.5-flash", "Why is the sky blue?", None)
//!     .await?;
//! println!("{}", response.text().unwrap_or_default());
//! # Ok(())
//! # }
//! ```
//!
//! See `docs/parity.md` (generated) for the Python-to-Rust API mapping and
//! `docs/migrating-from-python.md` for a migration guide.

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

pub use client::{Backend, Client, ClientBuilder};
pub use error::{ApiError, Error, Result};
pub use pager::Pager;
