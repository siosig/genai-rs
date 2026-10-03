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
            attempts: options.attempts.map_or(defaults.attempts, |a| {
                a.max(1).try_into().unwrap_or(u32::MAX)
            }),
            initial_delay: options
                .initial_delay
                .map_or(defaults.initial_delay, Duration::from_secs_f64),
            max_delay: options
                .max_delay
                .map_or(defaults.max_delay, Duration::from_secs_f64),
            exp_base: options.exp_base.unwrap_or(defaults.exp_base),
            jitter: options.jitter.unwrap_or(defaults.jitter),
            status_codes: options
                .http_status_codes
                .clone()
                .map(|codes| {
                    codes
                        .into_iter()
                        .filter_map(|c| u16::try_from(c).ok())
                        .collect()
                })
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

/// The retry delay sequence for a [`RetryPolicy`], as a `backon::Backoff`
/// (which is simply `Iterator<Item = Duration> + Send + Sync + Unpin`).
///
/// This exists instead of `ExponentialBuilder::with_jitter()` because the
/// two apply *different* jitter. `backon` adds a random amount within
/// `(0, current_delay)` -- proportional to how long the wait already is,
/// and taking no parameter, so a configured
/// [`crate::types::HttpRetryOptions::jitter`] had no effect at all.
/// Python uses `tenacity.wait_exponential_jitter`, whose documented formula
/// is
///
/// ```text
/// min(initial * exp_base^(attempt - 1) + uniform(0, jitter), max)
/// ```
///
/// i.e. a *fixed-range* jitter in seconds, added before the cap is applied.
/// That is what this reproduces, so `jitter` means the same thing here as
/// it does in the Python SDK and in `contracts/wire-protocol.md`.
pub(crate) struct JitteredBackoff {
    initial_delay_secs: f64,
    max_delay_secs: f64,
    exp_base: f64,
    jitter: f64,
    /// Retries already yielded; `attempt - 1` in the formula above.
    retries_yielded: u32,
    /// Total attempts allowed including the first, so this yields at most
    /// `attempts - 1` delays.
    max_retries: u32,
}

impl JitteredBackoff {
    pub(crate) fn new(policy: &RetryPolicy) -> Self {
        Self {
            initial_delay_secs: policy.initial_delay.as_secs_f64(),
            max_delay_secs: policy.max_delay.as_secs_f64(),
            exp_base: policy.exp_base,
            jitter: policy.jitter,
            retries_yielded: 0,
            max_retries: policy.attempts.saturating_sub(1),
        }
    }
}

impl Iterator for JitteredBackoff {
    type Item = Duration;

    fn next(&mut self) -> Option<Duration> {
        if self.retries_yielded >= self.max_retries {
            return None;
        }
        let exponent = f64::from(self.retries_yielded);
        self.retries_yielded += 1;

        let jitter = if self.jitter > 0.0 {
            rand::random_range(0.0..self.jitter)
        } else {
            0.0
        };
        let delay = self
            .initial_delay_secs
            .mul_add(self.exp_base.powf(exponent), jitter)
            .clamp(0.0, self.max_delay_secs);
        Some(Duration::from_secs_f64(delay))
    }
}

#[cfg(test)]
mod backoff_tests {
    use super::{JitteredBackoff, RetryPolicy};

    #[test]
    fn yields_one_delay_fewer_than_attempts() {
        let policy = RetryPolicy {
            attempts: 3,
            ..RetryPolicy::default()
        };
        assert_eq!(JitteredBackoff::new(&policy).count(), 2);
    }

    #[test]
    fn a_single_attempt_yields_no_delays() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.attempts, 1);
        assert_eq!(JitteredBackoff::new(&policy).count(), 0);
    }

    #[test]
    fn delays_grow_exponentially_within_the_configured_jitter_band() {
        // Python: min(initial * exp_base^(n) + uniform(0, jitter), max).
        // With initial=1, base=2, jitter=0.5 the nth delay must land in
        // [2^n, 2^n + 0.5).
        let policy = RetryPolicy {
            attempts: 5,
            initial_delay: std::time::Duration::from_secs(1),
            max_delay: std::time::Duration::from_secs(600),
            exp_base: 2.0,
            jitter: 0.5,
            ..RetryPolicy::default()
        };
        for (index, delay) in JitteredBackoff::new(&policy).enumerate() {
            let base = 2_f64.powi(i32::try_from(index).unwrap());
            let secs = delay.as_secs_f64();
            assert!(
                secs >= base && secs < base + 0.5,
                "delay {index} was {secs}, expected [{base}, {})",
                base + 0.5
            );
        }
    }

    #[test]
    fn delays_are_capped_at_max_delay() {
        let policy = RetryPolicy {
            attempts: 10,
            initial_delay: std::time::Duration::from_secs(1),
            max_delay: std::time::Duration::from_secs(5),
            exp_base: 2.0,
            jitter: 1.0,
            ..RetryPolicy::default()
        };
        for delay in JitteredBackoff::new(&policy) {
            assert!(delay.as_secs_f64() <= 5.0, "delay {delay:?} exceeded max");
        }
    }

    #[test]
    fn zero_jitter_is_deterministic() {
        let policy = RetryPolicy {
            attempts: 4,
            initial_delay: std::time::Duration::from_secs(1),
            exp_base: 2.0,
            jitter: 0.0,
            ..RetryPolicy::default()
        };
        let delays: Vec<f64> = JitteredBackoff::new(&policy)
            .map(|d| d.as_secs_f64())
            .collect();
        assert_eq!(delays, vec![1.0, 2.0, 4.0]);
    }
}
