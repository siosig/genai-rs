//! Retry policy handling, mirroring Python's `_api_client.py` `retry_args`.

use std::time::Duration;

use crate::types::HttpRetryOptions;

/// Default retry status codes, matching the Gemini API client library
/// convention (Cloud Storage retry strategy): request timeout, rate limit,
/// and 5xx server errors.
pub(crate) const DEFAULT_RETRY_STATUS_CODES: [u16; 6] = [408, 429, 500, 502, 503, 504];

/// A resolved, ready-to-use retry policy.
#[derive(Debug, Clone)]
pub(crate) struct RetryPolicy {
    /// Total attempts, including the first (non-retry) attempt.
    pub attempts: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub exp_base: f64,
    pub jitter: f64,
    pub status_codes: Vec<u16>,
}

impl Default for RetryPolicy {
    /// No retries: a single attempt only. Matches the Python client's
    /// behavior when `HttpRetryOptions` is not configured.
    fn default() -> Self {
        Self {
            attempts: 1,
            initial_delay: Duration::from_secs_f64(1.0),
            max_delay: Duration::from_secs(60),
            exp_base: 2.0,
            jitter: 1.0,
            status_codes: DEFAULT_RETRY_STATUS_CODES.to_vec(),
        }
    }
}

impl RetryPolicy {
    /// Resolves an optional `HttpRetryOptions` into a concrete policy,
    /// applying the SDK's documented defaults for any unset field.
    pub(crate) fn from_options(options: Option<&HttpRetryOptions>) -> Self {
        let Some(options) = options else {
            return Self::default();
        };

        let defaults = Self {
            attempts: 5,
            ..Self::default()
        };

        Self {
            attempts: options.attempts.map_or(defaults.attempts, |a| a.max(1).try_into().unwrap_or(u32::MAX)),
            initial_delay: options
                .initial_delay
                .map_or(defaults.initial_delay, Duration::from_secs_f64),
            max_delay: options.max_delay.map_or(defaults.max_delay, Duration::from_secs_f64),
            exp_base: options.exp_base.unwrap_or(defaults.exp_base),
            jitter: options.jitter.unwrap_or(defaults.jitter),
            status_codes: options
                .http_status_codes
                .clone()
                .map(|codes| codes.into_iter().filter_map(|c| u16::try_from(c).ok()).collect())
                .unwrap_or(defaults.status_codes),
        }
    }

    /// Whether a response with this status code should be retried.
    #[must_use]
    pub(crate) fn should_retry_status(&self, status: u16) -> bool {
        self.status_codes.contains(&status)
    }
}

#[cfg(test)]
mod tests {
    use super::RetryPolicy;
    use crate::types::HttpRetryOptions;

    #[test]
    fn default_policy_has_a_single_attempt() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.attempts, 1);
    }

    #[test]
    fn configured_policy_defaults_to_five_attempts() {
        let policy = RetryPolicy::from_options(Some(&HttpRetryOptions::default()));
        assert_eq!(policy.attempts, 5);
        assert!(policy.should_retry_status(503));
        assert!(!policy.should_retry_status(400));
    }

    #[test]
    fn explicit_attempts_and_codes_are_honored() {
        let options = HttpRetryOptions {
            attempts: Some(3),
            http_status_codes: Some(vec![503]),
            ..Default::default()
        };
        let policy = RetryPolicy::from_options(Some(&options));
        assert_eq!(policy.attempts, 3);
        assert!(policy.should_retry_status(503));
        assert!(!policy.should_retry_status(429));
    }
}
