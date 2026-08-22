//! The HTTP transport layer: request building, retries, SSE streaming, and
//! (from the `files`/`file_search_stores` modules) resumable uploads.
//! Mirrors Python's `_api_client.py`.

pub(crate) mod headers;
pub(crate) mod retry;
pub(crate) mod sse;
pub(crate) mod upload;

use std::collections::HashMap;
use std::time::Duration;

use backon::Retryable;
use bytes::Bytes;
use futures_core::Stream;
use reqwest::{Method, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value};

use crate::error::{ApiError, Error};
use crate::types::HttpOptions;
use headers::{API_KEY_HEADER, SERVER_TIMEOUT_HEADER};
use retry::RetryPolicy;

/// The default Gemini Developer API base URL.
pub(crate) const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/";
/// The default Gemini Developer API version path segment.
pub(crate) const DEFAULT_API_VERSION: &str = "v1beta";

/// A buffered HTTP response.
#[expect(
    dead_code,
    reason = "status/headers are read by tests only today; kept for future response-header inspection (e.g. rate-limit headers) without changing this type's shape"
)]
#[derive(Debug, Clone)]
pub(crate) struct HttpResponse {
    pub status: StatusCode,
    pub headers: HashMap<String, String>,
    pub body: Bytes,
}

/// Normalizes a JSON key for case- and separator-insensitive matching, so
/// `topP`, `top_p` and `TOPP` all collide. Ports Python's
/// `_common._normalize_key_for_matching`.
fn normalize_key_for_matching(key: &str) -> String {
    key.replace('_', "").to_lowercase()
}

/// Recursively merges `update` into `target`, matching Python's
/// `_common.recursive_dict_update`.
///
/// Two behaviours here are load-bearing rather than incidental, and both
/// come from the Python original:
///
/// - **Nested objects merge instead of replacing.** `extra_body` is how a
///   caller reaches a request field this crate's typed config doesn't model
///   yet, and those fields usually sit *inside* an existing object (e.g. a
///   new knob under `generationConfig`). A shallow insert would drop every
///   sibling the converters had already written.
/// - **Keys are aligned to the casing already present in the request.** The
///   body a converter produced uses wire casing (`generationConfig`), but a
///   caller writing `extra_body` by hand may reasonably use the Rust field
///   spelling (`generation_config`). Python reconciles the two before
///   merging (`_common.align_key_case`); without that, the two spellings
///   would both be sent and the server would see a duplicate.
///
/// Lists are replaced wholesale, not concatenated -- also matching Python,
/// which treats an `extra_body` list as the authoritative value.
fn recursive_body_update(target: &mut Map<String, Value>, update: &Map<String, Value>) {
    // Existing keys, indexed by their normalized form, so an update written
    // in the other casing still lands on the same field.
    let existing: HashMap<String, String> = target
        .keys()
        .map(|key| (normalize_key_for_matching(key), key.clone()))
        .collect();

    for (key, value) in update {
        let aligned = existing
            .get(&normalize_key_for_matching(key))
            .cloned()
            .unwrap_or_else(|| key.clone());

        match (target.get_mut(&aligned), value) {
            (Some(Value::Object(nested_target)), Value::Object(nested_update)) => {
                recursive_body_update(nested_target, nested_update);
            }
            _ => {
                target.insert(aligned, value.clone());
            }
        }
    }
}

impl HttpResponse {
    fn header_map(headers: &reqwest::header::HeaderMap) -> HashMap<String, String> {
        headers
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|v| (name.as_str().to_owned(), v.to_owned()))
            })
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
            api_version: options
                .api_version
                .clone()
                .unwrap_or_else(|| DEFAULT_API_VERSION.to_owned()),
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
        headers.insert(
            API_KEY_HEADER.to_owned(),
            self.api_key.expose_secret().to_owned(),
        );
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
        if let Value::Object(base) = &mut merged {
            recursive_body_update(base, extra);
        }
        merged
    }

    fn retry_policy(&self, per_request: Option<&HttpOptions>) -> RetryPolicy {
        per_request
            .and_then(|o| o.retry_options.as_ref())
            .map_or_else(
                || self.default_retry.clone(),
                |r| RetryPolicy::from_options(Some(r)),
            )
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
            let api_err = ApiError::from_response(
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown"),
                resp_headers,
                &body,
            );
            return Err(Error::Api(Box::new(api_err)));
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

        let backoff = retry::JitteredBackoff::new(&policy);

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
            return Err(Error::Api(Box::new(api_err)));
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
        let response = self
            .request(Method::GET, path, query, None, per_request)
            .await?;
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

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use serde_json::{Value, json};
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::{DEFAULT_BASE_URL, HttpClient, recursive_body_update};
    use crate::error::Error;
    use crate::types::{HttpOptions, HttpRetryOptions};

    /// P8 of `specs/002-oss-release-compliance/contracts/protected-identifiers.md`.
    ///
    /// The host belongs to the upstream API, not to this crate; the rest of
    /// that contract is guarded from `tests/protected_identifiers.rs`, but
    /// `DEFAULT_BASE_URL` is `pub(crate)` so it has to be checked from inside.
    #[test]
    fn default_base_url_points_at_the_gemini_developer_api() {
        assert!(
            DEFAULT_BASE_URL.contains("generativelanguage.googleapis.com"),
            "the default base URL must stay on the upstream API host, got {DEFAULT_BASE_URL}"
        );
    }

    fn client_for(server: &MockServer, options: HttpOptions) -> HttpClient {
        let mut options = options;
        options.base_url = Some(server.uri());
        options.api_version = Some(String::new());
        HttpClient::new(SecretString::from("test-key".to_owned()), &options).unwrap()
    }

    #[tokio::test]
    async fn retries_retryable_status_until_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(2)
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .expect(1)
            .mount(&server)
            .await;

        let options = HttpOptions {
            retry_options: Some(HttpRetryOptions {
                attempts: Some(3),
                initial_delay: Some(0.001),
                max_delay: Some(0.001),
                ..Default::default()
            }),
            ..Default::default()
        };
        let client = client_for(&server, options);
        let response = client
            .request(reqwest::Method::GET, "x", None, None, None)
            .await
            .unwrap();
        assert_eq!(response.status, reqwest::StatusCode::OK);
        server.verify().await;
    }

    #[tokio::test]
    async fn does_not_retry_without_retry_options() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server, HttpOptions::default());
        let err = client
            .request(reqwest::Method::GET, "x", None, None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Api(api_err) if api_err.code == 503));
        server.verify().await;
    }

    #[tokio::test]
    async fn does_not_retry_non_retryable_status() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(400))
            .expect(1)
            .mount(&server)
            .await;

        let options = HttpOptions {
            retry_options: Some(HttpRetryOptions {
                attempts: Some(3),
                initial_delay: Some(0.001),
                max_delay: Some(0.001),
                ..Default::default()
            }),
            ..Default::default()
        };
        let client = client_for(&server, options);
        let err = client
            .request(reqwest::Method::GET, "x", None, None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Api(api_err) if api_err.code == 400));
        server.verify().await;
    }

    #[tokio::test]
    async fn sends_api_key_and_sdk_identification_headers() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .and(header("x-goog-api-key", "test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server, HttpOptions::default());
        client
            .request(reqwest::Method::GET, "x", None, None, None)
            .await
            .unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn per_request_timeout_header_reflects_configured_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .and(header("X-Server-Timeout", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server, HttpOptions::default());
        let per_request = HttpOptions {
            timeout: Some(1500),
            ..Default::default()
        };
        client
            .request(reqwest::Method::GET, "x", None, None, Some(&per_request))
            .await
            .unwrap();
        server.verify().await;
    }

    #[test]
    fn recursive_body_update_merges_nested_objects_and_aligns_key_case() {
        let mut target = json!({
            "model": "models/x",
            "generationConfig": {"temperature": 0.5, "maxOutputTokens": 1024},
        });
        let update = json!({
            "extraField": "value",
            // Rust field casing; must land on the existing wire-cased keys
            // rather than adding a second, duplicate spelling.
            "generation_config": {"top_p": 0.9, "max_output_tokens": 2048},
        });
        let Value::Object(target_map) = &mut target else {
            unreachable!()
        };
        let Value::Object(update_map) = &update else {
            unreachable!()
        };
        recursive_body_update(target_map, update_map);

        assert_eq!(
            target,
            json!({
                "model": "models/x",
                "generationConfig": {
                    // Sibling survived the nested override.
                    "temperature": 0.5,
                    // Aligned onto the existing wire-cased key and overrode it.
                    "maxOutputTokens": 2048,
                    // No `topP` existed to align onto, so the caller's own
                    // spelling is kept verbatim -- confirmed byte-identical to
                    // Python's `_common.recursive_dict_update` for this input.
                    "top_p": 0.9,
                },
                "extraField": "value",
            })
        );
    }

    #[test]
    fn recursive_body_update_replaces_lists_wholesale() {
        // Matches Python, which treats an `extra_body` list as authoritative
        // rather than concatenating.
        let mut target = json!({"stopSequences": ["a", "b"]});
        let update = json!({"stopSequences": ["z"]});
        let Value::Object(target_map) = &mut target else {
            unreachable!()
        };
        let Value::Object(update_map) = &update else {
            unreachable!()
        };
        recursive_body_update(target_map, update_map);
        assert_eq!(target, json!({"stopSequences": ["z"]}));
    }

    #[tokio::test]
    async fn extra_body_is_deep_merged_into_request_body() {
        let server = MockServer::start().await;
        // The previous version of this test only counted requests, so it
        // passed even while `merged_body` was doing a shallow insert that
        // dropped every sibling of an overridden nested object. Assert the
        // exact body instead.
        Mock::given(method("POST"))
            .and(path("/x"))
            .and(body_json(json!({
                // Untouched top-level field survives.
                "model": "models/x",
                // Nested object is *merged*: `temperature` is still there,
                // `topP` was added, `maxOutputTokens` was overridden.
                "generationConfig": {
                    "temperature": 0.5,
                    "maxOutputTokens": 2048,
                    "top_p": 0.9,
                },
                // A brand-new top-level field is added.
                "extraField": "value",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;

        let mut extra = serde_json::Map::new();
        extra.insert("extraField".to_owned(), json!("value"));
        // Written in Rust field casing (`generation_config` / `top_p`) to
        // prove key alignment against the request's wire casing, matching
        // Python's `_common.align_key_case`.
        extra.insert(
            "generation_config".to_owned(),
            json!({"top_p": 0.9, "max_output_tokens": 2048}),
        );
        let options = HttpOptions {
            extra_body: Some(extra),
            ..Default::default()
        };
        let client = client_for(&server, options);
        client
            .request(
                reqwest::Method::POST,
                "x",
                None,
                Some(json!({
                    "model": "models/x",
                    "generationConfig": {"temperature": 0.5, "maxOutputTokens": 1024},
                })),
                None,
            )
            .await
            .unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn build_url_inserts_api_version_between_base_and_path() {
        let server = MockServer::start().await;
        let mut options = HttpOptions {
            base_url: Some(server.uri()),
            ..Default::default()
        };
        options.api_version = None; // default v1beta
        let client = HttpClient::new(SecretString::from("k".to_owned()), &options).unwrap();
        assert_eq!(
            client.build_url("models/x:generateContent", None),
            format!("{}/v1beta/models/x:generateContent", server.uri())
        );
    }
}
