//! Serialized access to the process-wide state a client reads while it is
//! being built: the API-key environment variables, `GOOGLE_GEMINI_BASE_URL`
//! and the default set by `set_default_base_urls`.
//!
//! `std::env::set_var` is `unsafe` because it races with concurrent reads, so
//! every test that touches these goes through [`with_env`], which holds one
//! lock for the whole closure. `Client::builder().build()` and
//! `get_base_url` read their inputs eagerly, so the closure only has to
//! *build* the client; requests can be made afterwards without the lock.

#![expect(
    unsafe_code,
    reason = "std::env::set_var/remove_var are unsafe in a multi-threaded process; with_env serializes them behind ENV_LOCK"
)]

use std::sync::{Mutex, PoisonError};

use gemini_genai::base_url::set_default_base_urls;

pub const GOOGLE_API_KEY: &str = "GOOGLE_API_KEY";
pub const GEMINI_API_KEY: &str = "GEMINI_API_KEY";
pub const GOOGLE_GEMINI_BASE_URL: &str = "GOOGLE_GEMINI_BASE_URL";
pub const USE_VERTEXAI: &str = "GOOGLE_GENAI_USE_VERTEXAI";

const MANAGED_VARS: [&str; 4] = [
    GOOGLE_API_KEY,
    GEMINI_API_KEY,
    GOOGLE_GEMINI_BASE_URL,
    USE_VERTEXAI,
];

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn clear() {
    for var in MANAGED_VARS {
        unsafe { std::env::remove_var(var) };
    }
    set_default_base_urls(None);
}

/// Runs `f` with exactly `vars` set (everything else in [`MANAGED_VARS`]
/// removed, and no default base URL), then restores a clean state.
pub fn with_env<T>(vars: &[(&str, &str)], f: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    clear();
    for (name, value) in vars {
        unsafe { std::env::set_var(name, value) };
    }
    let result = f();
    clear();
    result
}
