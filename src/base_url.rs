//! Default base URL resolution for the Gemini Developer API.
//!
//! Mirrors Python's `_base_url.py`. Only the Developer API (`mldev`) half is
//! ported: Vertex AI is out of scope, so there is no `vertex_url` and
//! [`get_base_url`] takes no `vertexai` flag.

use std::{
    env,
    sync::{Mutex, OnceLock, PoisonError},
};

use crate::types::HttpOptions;

/// Environment variable that overrides the Gemini Developer API base URL.
pub(crate) const GOOGLE_GEMINI_BASE_URL: &str = "GOOGLE_GEMINI_BASE_URL";

/// The base URL most recently set by [`set_default_base_urls`]; the
/// counterpart of Python's module-level `_default_base_gemini_url`.
fn default_base_gemini_url() -> &'static Mutex<Option<String>> {
    static DEFAULT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    DEFAULT.get_or_init(|| Mutex::new(None))
}

/// Overrides the process-wide default base URL for the Gemini Developer API.
///
/// Pass `None` to clear a previously set default. Mirrors Python's
/// `set_default_base_urls` (without the Vertex AI URL).
pub fn set_default_base_urls(gemini_url: Option<String>) {
    *default_base_gemini_url()
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = gemini_url;
}

/// Returns the process-wide default base URL set by [`set_default_base_urls`],
/// if any. The Developer-API-only form of upstream's pair of defaults.
#[must_use]
pub fn get_default_base_urls() -> Option<String> {
    default_base_gemini_url()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Returns the base URL to use, by priority:
///
/// 1. `http_options.base_url`.
/// 2. The latest call to [`set_default_base_urls`].
/// 3. The `GOOGLE_GEMINI_BASE_URL` environment variable.
///
/// Like upstream, an empty string counts as unset at every level. Returns
/// `None` when none of them is set (the HTTP client then uses its built-in
/// default).
#[must_use]
pub fn get_base_url(http_options: Option<&HttpOptions>) -> Option<String> {
    http_options
        .and_then(|options| options.base_url.clone())
        .filter(|url| !url.is_empty())
        .or_else(|| get_default_base_urls().filter(|url| !url.is_empty()))
        .or_else(|| {
            env::var(GOOGLE_GEMINI_BASE_URL)
                .ok()
                .filter(|url| !url.is_empty())
        })
}

/// Serializes every test that touches `GOOGLE_*` environment variables or
/// the process-wide default base URL (shared with `client::tests`).
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
#[expect(
    unsafe_code,
    reason = "std::env::set_var/remove_var are unsafe in a multi-threaded process; tests serialize via TEST_ENV_LOCK"
)]
mod tests {
    use super::{GOOGLE_GEMINI_BASE_URL, TEST_ENV_LOCK, get_base_url, set_default_base_urls};
    use crate::{Client, types::HttpOptions};

    const API_KEY_VAR: &str = "GOOGLE_API_KEY";

    fn clear() {
        unsafe { std::env::remove_var(GOOGLE_GEMINI_BASE_URL) };
        set_default_base_urls(None);
    }

    #[test]
    fn base_url_env_var_overrides_default() {
        let _guard = TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear();
        unsafe {
            std::env::set_var(API_KEY_VAR, "k");
            std::env::set_var(GOOGLE_GEMINI_BASE_URL, "https://example.test/");
        }
        let client = Client::builder().build().unwrap();
        assert_eq!(client.http().base_url(), "https://example.test/");
        clear();
        unsafe { std::env::remove_var(API_KEY_VAR) };
    }

    #[test]
    fn set_default_base_urls_is_used_when_no_option_given() {
        let _guard = TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear();
        unsafe { std::env::set_var(GOOGLE_GEMINI_BASE_URL, "https://env.test/") };
        set_default_base_urls(Some("https://default.test/".to_owned()));
        assert_eq!(
            get_base_url(None).as_deref(),
            Some("https://default.test/"),
            "the default set via set_default_base_urls beats the environment variable"
        );
        assert_eq!(
            get_base_url(Some(&HttpOptions::default())).as_deref(),
            Some("https://default.test/")
        );
        clear();
    }

    #[test]
    fn http_options_base_url_wins_over_default() {
        let _guard = TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear();
        set_default_base_urls(Some("https://default.test/".to_owned()));
        let options = HttpOptions {
            base_url: Some("https://option.test/".to_owned()),
            ..Default::default()
        };
        assert_eq!(
            get_base_url(Some(&options)).as_deref(),
            Some("https://option.test/")
        );
        clear();
    }

    #[test]
    fn get_base_url_is_none_when_nothing_is_set() {
        let _guard = TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        clear();
        assert_eq!(get_base_url(None), None);
    }
}
