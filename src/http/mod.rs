//! The HTTP transport layer: request building, retries, SSE streaming, and
//! (from the `files`/`file_search_stores` modules) resumable uploads.
//! Mirrors Python's `_api_client.py`.

pub(crate) mod headers;
pub(crate) mod retry;
pub(crate) mod sse;
pub(crate) mod upload;

use std::collections::HashMap;
use std::time::Duration;

use backon::{ExponentialBuilder, Retryable};
use bytes::Bytes;
use futures_core::Stream;
use reqwest::{Method, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

use crate::error::{ApiError, Error};
use crate::types::HttpOptions;
use headers::{API_CLIENT_HEADER, API_KEY_HEADER, SERVER_TIMEOUT_HEADER, USER_AGENT_HEADER};
use retry::RetryPolicy;

/// The default Gemini Developer API base URL.
pub(crate) const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/";
/// The default Gemini Developer API version path segment.
pub(crate) const DEFAULT_API_VERSION: &str = "v1beta";

/// A buffered HTTP response.
#[derive(Debug, Clone)]
pub(crate) struct HttpResponse {
    pub status: StatusCode,
    pub headers: HashMap<String, String>,
    pub body: Bytes,
}

impl HttpResponse {
    fn header_map(headers: &reqwest::header::HeaderMap) -> HashMap<String, String> {
        headers
            .iter()
            .filter_map(|(name, value)| value.to_str().ok().map(|v| (name.as_str().to_owned(), v.to_owned())))
            .collect()
    }

}

/// The resolved (client-level) HTTP configuration, plus the `reqwest`
/// client used to issue requests.
#[derive(Debug)]
pub(crate) struct HttpClient {
    inner: reqwest::Client,
    api_key: SecretString,
    base_url: String,
    api_version: String,
    default_headers: HashMap<String, String>,
    default_timeout: Option<i64>,
    default_extra_body: Option<serde_json::Map<String, Value>>,
    default_retry: RetryPolicy,
}

impl HttpClient {
    /// Builds an [`HttpClient`] from a resolved API key and the client's
    /// default [`HttpOptions`].
    pub(crate) fn new(api_key: SecretString, options: &HttpOptions) -> Result<Self, Error> {
        let mut default_headers = options.headers.clone().unwrap_or_default();
        headers::apply_sdk_headers(&mut default_headers);

        let base_url = options
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        let base_url = if base_url.ends_with('/') {
            base_url
        } else {
            format!("{base_url}/")
        };

        Ok(Self {
            inner: reqwest::Client::builder().build()?,
            api_key,
            base_url,
            api_version: options.api_version.clone().unwrap_or_else(|| DEFAULT_API_VERSION.to_owned()),
            default_headers,
            default_timeout: options.timeout,
            default_extra_body: options.extra_body.clone(),
            default_retry: RetryPolicy::from_options(options.retry_options.as_ref()),
        })
    }

    /// Builds the full request URL for a `path` such as
    /// `{model}:generateContent`, with the client's `api_version` inserted
    /// and an optional query string appended.
    pub(crate) fn build_url(&self, path: &str, query: Option<&str>) -> String {
        let version_segment = if self.api_version.is_empty() {
            String::new()
        } else {
            format!("{}/", self.api_version)
        };
        let mut url = format!("{}{version_segment}{path}", self.base_url);
        if let Some(query) = query {
            if !query.is_empty() {
                url.push('?');
                url.push_str(query);
            }
        }
        url
    }

    fn merged_headers(&self, per_request: Option<&HttpOptions>) -> HashMap<String, String> {
        let mut headers = self.default_headers.clone();
        if let Some(extra) = per_request.and_then(|o| o.headers.as_ref()) {
            headers.extend(extra.clone());
        }
        headers.insert(API_KEY_HEADER.to_owned(), self.api_key.expose_secret().to_owned());
        let timeout = per_request.and_then(|o| o.timeout).or(self.default_timeout);
        if let Some(seconds) = headers::server_timeout_seconds(timeout) {
            headers.insert(SERVER_TIMEOUT_HEADER.to_owned(), seconds);
        }
        headers
    }

    fn merged_body(&self, body: Value, per_request: Option<&HttpOptions>) -> Value {
        let extra = per_request
            .and_then(|o| o.extra_body.as_ref())
            .or(self.default_extra_body.as_ref());
        let Some(extra) = extra else {
            return body;
        };
        let mut merged = body;
        if let (Value::Object(base), extra) = (&mut merged, extra) {
            for (key, value) in extra {
                base.insert(key.clone(), value.clone());
            }
        }
        merged
    }

    fn retry_policy(&self, per_request: Option<&HttpOptions>) -> RetryPolicy {
        per_request
            .and_then(|o| o.retry_options.as_ref())
            .map_or_else(|| self.default_retry.clone(), |r| RetryPolicy::from_options(Some(r)))
    }

    fn timeout(&self, per_request: Option<&HttpOptions>) -> Option<Duration> {
        per_request
            .and_then(|o| o.timeout)
            .or(self.default_timeout)
            .map(|ms| Duration::from_millis(ms.max(0).unsigned_abs()))
    }

    /// Sends a single request attempt. Non-2xx responses are converted to
    /// [`Error::Api`] immediately so the retry predicate in
    /// [`Self::request`] can decide, from the status code alone, whether
    /// to retry.
    async fn send_once(
        &self,
        method: &Method,
        url: &str,
        headers: &HashMap<String, String>,
        body: Option<&Value>,
        timeout: Option<Duration>,
    ) -> Result<HttpResponse, Error> {
        let mut request = self.inner.request(method.clone(), url);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        let response = request.send().await?;
        let status = response.status();
        let resp_headers = HttpResponse::header_map(response.headers());
        let body = response.bytes().await?;
        if !status.is_success() {
            let api_err = ApiError::from_response(status.as_u16(), status.canonical_reason().unwrap_or("Unknown"), resp_headers, &body);
            return Err(Error::Api(api_err));
        }
        Ok(HttpResponse {
            status,
            headers: resp_headers,
            body,
        })
    }

    /// Sends a single buffered request, retrying according to the
    /// resolved [`RetryPolicy`], and raises an [`Error::Api`] for non-2xx
    /// responses.
    pub(crate) async fn request(
        &self,
        method: Method,
        path: &str,
        query: Option<&str>,
        body: Option<Value>,
        per_request: Option<&HttpOptions>,
    ) -> Result<HttpResponse, Error> {
        let url = self.build_url(path, query);
        let headers = self.merged_headers(per_request);
        let body = body.map(|b| self.merged_body(b, per_request));
        let policy = self.retry_policy(per_request);
        let timeout = self.timeout(per_request);

        let backoff = ExponentialBuilder::default()
            .with_min_delay(policy.initial_delay)
            .with_max_delay(policy.max_delay)
            .with_factor(policy.exp_base as f32)
            .with_jitter()
            .with_max_times(policy.attempts.saturating_sub(1) as usize);

        (|| self.send_once(&method, &url, &headers, body.as_ref(), timeout))
            .retry(backoff)
            .when(|err: &Error| match err {
                Error::Http(_) => true,
                Error::Api(api_err) => policy.should_retry_status(api_err.code),
                _ => false,
            })
            .await
    }

    /// Sends a request and returns a stream of decoded SSE payloads
    /// (`?alt=sse`). Non-2xx initial responses raise [`Error::Api`].
    pub(crate) async fn request_stream(
        &self,
        method: Method,
        path: &str,
        query: Option<&str>,
        body: Option<Value>,
        per_request: Option<&HttpOptions>,
    ) -> Result<impl Stream<Item = Result<Value, Error>> + Send + Unpin + use<>, Error> {
        let url = self.build_url(path, query);
        let headers = self.merged_headers(per_request);
        let body = body.map(|b| self.merged_body(b, per_request));

        let mut request = self.inner.request(method, &url);
        for (name, value) in &headers {
            request = request.header(name, value);
        }
        if let Some(body) = &body {
            request = request.json(body);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let resp_headers = HttpResponse::header_map(response.headers());
            let body = response.bytes().await?;
            let api_err = ApiError::from_response(
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown"),
                resp_headers,
                &body,
            );
            return Err(Error::Api(api_err));
        }
        Ok(Box::pin(sse::parse_sse(response.bytes_stream())))
    }

    /// Downloads a resource's raw bytes (e.g. `GET {file}:download?alt=media`).
    pub(crate) async fn download(
        &self,
        path: &str,
        query: Option<&str>,
        per_request: Option<&HttpOptions>,
    ) -> Result<Bytes, Error> {
        let response = self.request(Method::GET, path, query, None, per_request).await?;
        Ok(response.body)
    }

    pub(crate) fn reqwest_client(&self) -> &reqwest::Client {
        &self.inner
    }

    pub(crate) fn api_key(&self) -> &SecretString {
        &self.api_key
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) fn api_version(&self) -> &str {
        &self.api_version
    }
}

