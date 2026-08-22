//! The `Client` entry point: backend/credential resolution and access to
//! per-module handles (`models()`, `chats()`, ...). Mirrors Python's
//! `client.py`.

use std::{env, sync::Arc};

use secrecy::SecretString;

pub use crate::error::Backend;
use crate::{
    error::{Error, Result},
    http::HttpClient,
    types::HttpOptions,
};

const GOOGLE_API_KEY_VAR: &str = "GOOGLE_API_KEY";
const GEMINI_API_KEY_VAR: &str = "GEMINI_API_KEY";
const USE_VERTEXAI_VAR: &str = "GOOGLE_GENAI_USE_VERTEXAI";
const BASE_URL_VAR: &str = "GOOGLE_GEMINI_BASE_URL";

#[derive(Debug)]
pub(crate) struct ClientInner {
    http: HttpClient,
}

/// The entry point for all Gemini API calls: `Client::new()` or
/// `Client::builder()`, then `client.models()`, `client.chats()`, etc.
///
/// Cheap to clone: internally reference-counted.
#[derive(Clone, Debug)]
pub struct Client {
    pub(crate) inner: Arc<ClientInner>,
}

impl Client {
    /// Builds a [`Client`] using the Gemini Developer API and an API key
    /// resolved from the environment (`GOOGLE_API_KEY`, falling back to
    /// `GEMINI_API_KEY`).
    ///
    /// # Errors
    /// Returns [`Error::Validation`] if neither environment variable is
    /// set, or [`Error::UnsupportedBackend`] if `GOOGLE_GENAI_USE_VERTEXAI`
    /// requests the (not yet implemented) Vertex AI backend.
    pub fn new() -> Result<Self> {
        Self::builder().build()
    }

    /// Starts building a [`Client`] with explicit configuration.
    #[must_use]
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// The shared HTTP transport this client was built with, used by
    /// every resource handle to issue requests.
    #[must_use]
    pub(crate) fn http(&self) -> &HttpClient {
        &self.inner.http
    }

    /// Text/image generation, embeddings, token counting, and model
    /// listing (`client.models()...`).
    #[must_use]
    pub fn models(&self) -> crate::models::Models {
        crate::models::Models {
            client: self.clone(),
        }
    }

    /// Multi-turn chat sessions (`client.chats().create(...)`).
    #[must_use]
    pub fn chats(&self) -> crate::chats::Chats {
        crate::chats::Chats {
            client: self.clone(),
        }
    }

    /// File upload/get/list/delete/download (`client.files()...`).
    #[must_use]
    pub fn files(&self) -> crate::files::Files {
        crate::files::Files {
            client: self.clone(),
        }
    }

    /// Context cache create/get/list/update/delete
    /// (`client.caches()...`).
    #[must_use]
    pub fn caches(&self) -> crate::caches::Caches {
        crate::caches::Caches {
            client: self.clone(),
        }
    }

    /// Fine-tuning job create/get/list/cancel (`client.tunings()...`).
    #[must_use]
    pub fn tunings(&self) -> crate::tunings::Tunings {
        crate::tunings::Tunings {
            client: self.clone(),
        }
    }

    /// Batch job create/get/list/cancel/delete (`client.batches()...`).
    #[must_use]
    pub fn batches(&self) -> crate::batches::Batches {
        crate::batches::Batches {
            client: self.clone(),
        }
    }

    /// Long-running operation polling (`client.operations()...`).
    #[must_use]
    pub fn operations(&self) -> crate::operations::Operations {
        crate::operations::Operations {
            client: self.clone(),
        }
    }

    /// File Search store create/get/list/delete/import
    /// (`client.file_search_stores()...`).
    #[must_use]
    pub fn file_search_stores(&self) -> crate::file_search_stores::FileSearchStores {
        crate::file_search_stores::FileSearchStores {
            client: self.clone(),
        }
    }

    /// Ephemeral auth token creation for the Live API
    /// (`client.auth_tokens()...`).
    #[must_use]
    pub fn auth_tokens(&self) -> crate::auth_tokens::AuthTokens {
        crate::auth_tokens::AuthTokens {
            client: self.clone(),
        }
    }

    /// Bidirectional realtime (Live API) sessions
    /// (`client.live().connect(...)`).
    #[cfg(feature = "live")]
    #[must_use]
    pub fn live(&self) -> crate::live::Live {
        crate::live::Live {
            client: self.clone(),
        }
    }
}

/// Builder for [`Client`]. See [`Client::builder`].
#[derive(Default)]
pub struct ClientBuilder {
    api_key: Option<String>,
    http_options: HttpOptions,
    vertexai: Option<bool>,
    project: Option<String>,
    location: Option<String>,
}

impl ClientBuilder {
    /// Sets the Gemini Developer API key explicitly, overriding the
    /// environment variables.
    #[must_use]
    pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets the client-level [`HttpOptions`] (base URL, timeout, retry
    /// policy, extra headers/body).
    #[must_use]
    pub fn http_options(mut self, http_options: HttpOptions) -> Self {
        self.http_options = http_options;
        self
    }

    /// Requests the Vertex AI backend. **Not yet implemented**: `build()`
    /// returns [`Error::UnsupportedBackend`] when this is `true`.
    #[must_use]
    pub fn vertexai(mut self, vertexai: bool) -> Self {
        self.vertexai = Some(vertexai);
        self
    }

    /// Sets the Google Cloud project for the (not yet implemented) Vertex
    /// AI backend.
    #[must_use]
    pub fn project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Sets the Google Cloud region for the (not yet implemented) Vertex
    /// AI backend.
    #[must_use]
    pub fn location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// Resolves configuration (arguments, then environment variables) and
    /// builds the [`Client`].
    ///
    /// # Errors
    /// See [`Client::new`].
    pub fn build(self) -> Result<Client> {
        if resolve_vertexai(self.vertexai) || self.project.is_some() || self.location.is_some() {
            return Err(Error::UnsupportedBackend("vertexai"));
        }

        let api_key = resolve_api_key(self.api_key)?;

        let mut http_options = self.http_options;
        if http_options.base_url.is_none() {
            http_options.base_url = env::var(BASE_URL_VAR).ok();
        }

        let http = HttpClient::new(SecretString::from(api_key), &http_options)?;
        Ok(Client {
            inner: Arc::new(ClientInner { http }),
        })
    }
}

fn resolve_vertexai(explicit: Option<bool>) -> bool {
    if let Some(value) = explicit {
        return value;
    }
    env::var(USE_VERTEXAI_VAR)
        .is_ok_and(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes"))
}

fn resolve_api_key(explicit: Option<String>) -> Result<String> {
    if let Some(key) = explicit {
        return Ok(key);
    }
    let google = env::var(GOOGLE_API_KEY_VAR).ok().filter(|s| !s.is_empty());
    let gemini = env::var(GEMINI_API_KEY_VAR).ok().filter(|s| !s.is_empty());
    match (google, gemini) {
        (Some(google), Some(_gemini)) => {
            tracing::warn!(
                "both {GOOGLE_API_KEY_VAR} and {GEMINI_API_KEY_VAR} are set; using {GOOGLE_API_KEY_VAR}"
            );
            Ok(google)
        }
        (Some(google), None) => Ok(google),
        (None, Some(gemini)) => Ok(gemini),
        (None, None) => Err(Error::Validation(format!(
            "no API key: set {GOOGLE_API_KEY_VAR} or {GEMINI_API_KEY_VAR}, or pass one to Client::builder().api_key(...)"
        ))),
    }
}

#[cfg(test)]
#[allow(
    unsafe_code,
    reason = "std::env::set_var/remove_var are unsafe in a multi-threaded process; tests serialize via ENV_LOCK"
)]
mod tests {
    use std::sync::Mutex;

    use super::{BASE_URL_VAR, Client, GEMINI_API_KEY_VAR, GOOGLE_API_KEY_VAR, USE_VERTEXAI_VAR};

    // Environment variables are process-global, so tests that touch them
    // must not run concurrently with each other.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_env() {
        for var in [
            GOOGLE_API_KEY_VAR,
            GEMINI_API_KEY_VAR,
            USE_VERTEXAI_VAR,
            BASE_URL_VAR,
        ] {
            unsafe { std::env::remove_var(var) };
        }
    }

    #[test]
    fn explicit_api_key_wins_over_environment() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_env();
        unsafe { std::env::set_var(GOOGLE_API_KEY_VAR, "env-key") };
        let client = Client::builder().api_key("explicit-key").build();
        assert!(client.is_ok());
        clear_env();
    }

    #[test]
    fn google_api_key_wins_over_gemini_api_key() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_env();
        unsafe {
            std::env::set_var(GOOGLE_API_KEY_VAR, "google-key");
            std::env::set_var(GEMINI_API_KEY_VAR, "gemini-key");
        }
        assert!(Client::new().is_ok());
        clear_env();
    }

    #[test]
    fn missing_api_key_is_a_validation_error() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_env();
        let err = Client::new().unwrap_err();
        assert!(matches!(err, crate::Error::Validation(_)));
        clear_env();
    }

    #[test]
    fn vertexai_env_flag_is_unsupported() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_env();
        unsafe {
            std::env::set_var(GOOGLE_API_KEY_VAR, "k");
            std::env::set_var(USE_VERTEXAI_VAR, "true");
        }
        let err = Client::new().unwrap_err();
        assert!(matches!(err, crate::Error::UnsupportedBackend("vertexai")));
        clear_env();
    }

    #[test]
    fn base_url_env_var_overrides_default() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear_env();
        unsafe {
            std::env::set_var(GOOGLE_API_KEY_VAR, "k");
            std::env::set_var(BASE_URL_VAR, "https://example.test/");
        }
        let client = Client::new().unwrap();
        assert_eq!(client.http().base_url(), "https://example.test/");
        clear_env();
    }
}
