//! `client.caches()`: context cache create/get/list/update/delete. Mirrors
//! Python's `caches.py` `Caches`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use reqwest::Method;
use serde_json::{Map, Value};

use crate::client::Client;
use crate::converters::generated::caches as conv;
use crate::error::Result;
use crate::pager::{PagedItem, Pager};
use crate::types::{
    CachedContent, CreateCachedContentConfig, DeleteCachedContentConfig,
    DeleteCachedContentResponse, GetCachedContentConfig, ListCachedContentsConfig,
    UpdateCachedContentConfig,
};

/// The page-fetch closure type backing a `caches().list(...)` [`Pager`].
type CachedContentPageFetch = Arc<
    dyn Fn(
            Map<String, Value>,
        )
            -> Pin<Box<dyn Future<Output = Result<(Vec<CachedContent>, Option<String>)>> + Send>>
        + Send
        + Sync,
>;

/// Handle for `client.caches()`. Cheap to construct; borrows nothing.
#[derive(Clone)]
pub struct Caches {
    pub(crate) client: Client,
}

impl Caches {
    /// Creates a cached-content resource from `contents`/`system_instruction`
    /// in `config`, so later [`crate::models::Models::generate_content`]
    /// calls can reference it via `GenerateContentConfig::cached_content`
    /// instead of resending that content. Mirrors Python's `Caches.create`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn create(
        &self,
        model: &str,
        config: Option<CreateCachedContentConfig>,
    ) -> Result<CachedContent> {
        let params = serde_json::json!({ "model": model, "config": config });
        let request = conv::create_cached_content_parameters_to_mldev(&params, None, None)?;
        let response = self
            .client
            .http()
            .request(Method::POST, "cachedContents", None, Some(request), None)
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        Ok(serde_json::from_value(wire)?)
    }

    /// Fetches a cached content's current configuration by resource name (a
    /// bare id or the full `cachedContents/{id}` name). Mirrors Python's
    /// `Caches.get`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn get(
        &self,
        name: &str,
        config: Option<GetCachedContentConfig>,
    ) -> Result<CachedContent> {
        let params = serde_json::json!({ "name": name, "config": config });
        let mut request = conv::get_cached_content_parameters_to_mldev(&params, None, None)?;
        let name_path = take_url_name(&mut request, "get_cached_content_parameters_to_mldev");
        let response = self
            .client
            .http()
            .request(Method::GET, &name_path, None, None, None)
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        Ok(serde_json::from_value(wire)?)
    }

    /// Lists cached contents, returning a [`Pager`] that fetches subsequent
    /// pages on demand. Mirrors Python's `Caches.list`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn list(
        &self,
        config: Option<ListCachedContentsConfig>,
    ) -> Result<Pager<CachedContent>> {
        let config_map = serde_json::to_value(&config)?
            .as_object()
            .cloned()
            .unwrap_or_default();
        let (page, next_page_token) = fetch_list_page(&self.client, config_map.clone()).await?;

        let client = self.client.clone();
        let fetch: CachedContentPageFetch = Arc::new(move |page_config: Map<String, Value>| {
            let client = client.clone();
            Box::pin(async move { fetch_list_page(&client, page_config).await })
        });

        Ok(Pager::new(
            PagedItem::CachedContents,
            page,
            config_map,
            next_page_token,
            fetch,
        ))
    }

    /// Updates a cached content's `ttl`/`expire_time`. Mirrors Python's
    /// `Caches.update`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn update(
        &self,
        name: &str,
        config: Option<UpdateCachedContentConfig>,
    ) -> Result<CachedContent> {
        let params = serde_json::json!({ "name": name, "config": config });
        let mut request = conv::update_cached_content_parameters_to_mldev(&params, None, None)?;
        let name_path = take_url_name(&mut request, "update_cached_content_parameters_to_mldev");
        let response = self
            .client
            .http()
            .request(Method::PATCH, &name_path, None, Some(request), None)
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        Ok(serde_json::from_value(wire)?)
    }

    /// Deletes a cached content by resource name. Mirrors Python's
    /// `Caches.delete`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn delete(
        &self,
        name: &str,
        config: Option<DeleteCachedContentConfig>,
    ) -> Result<DeleteCachedContentResponse> {
        let params = serde_json::json!({ "name": name, "config": config });
        let mut request = conv::delete_cached_content_parameters_to_mldev(&params, None, None)?;
        let name_path = take_url_name(&mut request, "delete_cached_content_parameters_to_mldev");
        let response = self
            .client
            .http()
            .request(Method::DELETE, &name_path, None, None, None)
            .await?;
        let wire: Value = if response.body.is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::from_slice(&response.body)?
        };
        let mldev = conv::delete_cached_content_response_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }
}

/// Removes and returns the `_url.name` path segment a `*_to_mldev`
/// converter sets on `request`.
///
/// # Panics
/// Panics if `request` isn't a JSON object, or if the named converter
/// didn't set `_url.name` — both are contract violations in the
/// (hand-audited) generated converters, not runtime conditions a caller
/// can trigger.
fn take_url_name(request: &mut Value, converter_name: &'static str) -> String {
    let request_obj = crate::converters::as_object_mut(request);
    request_obj
        .remove("_url")
        .and_then(|url| url.get("name").cloned())
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("{converter_name} always sets _url.name"))
}

/// Fetches one page of `cachedContents`, converting a `page_size`/
/// `page_token`-shaped config map into `_query` parameters. Shared by the
/// first page (in [`Caches::list`]) and the [`Pager`]'s fetch closure for
/// subsequent pages.
async fn fetch_list_page(
    client: &Client,
    config: Map<String, Value>,
) -> Result<(Vec<CachedContent>, Option<String>)> {
    let params = serde_json::json!({ "config": Value::Object(config) });
    let mut request = conv::list_cached_contents_parameters_to_mldev(&params, None, None)?;
    let request_obj = crate::converters::as_object_mut(&mut request);
    let query = request_obj.remove("_query");
    let query_string = query.as_ref().and_then(build_query_string);

    let response = client
        .http()
        .request(
            Method::GET,
            "cachedContents",
            query_string.as_deref(),
            None,
            None,
        )
        .await?;
    let wire: Value = serde_json::from_slice(&response.body)?;
    let mldev = conv::list_cached_contents_response_from_mldev(&wire, None, None)?;
    let items: Vec<CachedContent> = mldev
        .get("cached_contents")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    let next_page_token = mldev
        .get("next_page_token")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok((items, next_page_token))
}

/// Builds a `key=value&...` query string from a `_query`-shaped JSON
/// object (string/number/bool leaf values), percent-encoding via
/// [`url::form_urlencoded`]. Returns `None` for an absent/empty object.
fn build_query_string(query: &Value) -> Option<String> {
    let obj = query.as_object()?;
    if obj.is_empty() {
        return None;
    }
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in obj {
        let value = match value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        serializer.append_pair(key, &value);
    }
    Some(serializer.finish())
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::Caches;
    use crate::client::Client;
    use crate::types::HttpOptions;

    fn test_client(base_url: String) -> Client {
        Client::builder()
            .api_key("test-key")
            .http_options(HttpOptions {
                base_url: Some(base_url),
                ..Default::default()
            })
            .build()
            .unwrap()
    }

    fn caches(server: &MockServer) -> Caches {
        Caches {
            client: test_client(server.uri()),
        }
    }

    // Kept for parity with other test modules that construct a raw
    // SecretString directly; unused here but documents the pattern.
    #[allow(dead_code)]
    fn _unused(s: SecretString) {
        drop(s);
    }

    #[tokio::test]
    async fn create_posts_to_cached_contents_with_the_flattened_config_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/cachedContents"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "cachedContents/abc123",
                "model": "models/gemini-2.5-flash",
                "displayName": "test cache",
                "createTime": "2026-01-01T00:00:00Z",
                "expireTime": "2026-01-02T00:00:00Z",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let config = crate::types::CreateCachedContentConfig {
            display_name: Some("test cache".to_owned()),
            ttl: Some("86400s".to_owned()),
            contents: Some(vec![crate::types::Content {
                role: Some("user".to_owned()),
                parts: Some(vec![crate::types::Part {
                    text: Some("cache me".to_owned()),
                    ..Default::default()
                }]),
            }]),
            ..Default::default()
        };
        let cached = caches(&server)
            .create("gemini-2.5-flash", Some(config))
            .await
            .unwrap();
        assert_eq!(cached.name.as_deref(), Some("cachedContents/abc123"));
        assert_eq!(cached.display_name.as_deref(), Some("test cache"));
        assert_eq!(cached.expire_time.as_deref(), Some("2026-01-02T00:00:00Z"));
        server.verify().await;
    }

    #[tokio::test]
    async fn create_sends_ttl_and_contents_in_the_request_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/cachedContents"))
            .and(wiremock::matchers::body_json(serde_json::json!({
                "model": "models/gemini-2.5-flash",
                "ttl": "86400s",
                "displayName": "test cache",
                "contents": [{"role": "user", "parts": [{"text": "cache me"}]}],
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"name": "cachedContents/abc123"})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let config = crate::types::CreateCachedContentConfig {
            display_name: Some("test cache".to_owned()),
            ttl: Some("86400s".to_owned()),
            contents: Some(vec![crate::types::Content {
                role: Some("user".to_owned()),
                parts: Some(vec![crate::types::Part {
                    text: Some("cache me".to_owned()),
                    ..Default::default()
                }]),
            }]),
            ..Default::default()
        };
        caches(&server)
            .create("gemini-2.5-flash", Some(config))
            .await
            .unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn get_fetches_by_normalized_resource_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/cachedContents/abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "cachedContents/abc123",
                "model": "models/gemini-2.5-flash",
            })))
            .expect(1)
            .mount(&server)
            .await;

        // A bare id is normalized to `cachedContents/{id}` by `t_cached_content_name`.
        let cached = caches(&server).get("abc123", None).await.unwrap();
        assert_eq!(cached.name.as_deref(), Some("cachedContents/abc123"));
        server.verify().await;
    }

    #[tokio::test]
    async fn list_returns_a_pager_that_fetches_subsequent_pages() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/cachedContents"))
            .and(wiremock::matchers::query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "cachedContents": [{"name": "cachedContents/a"}],
                "nextPageToken": "tok1",
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1beta/cachedContents"))
            .and(wiremock::matchers::query_param("pageToken", "tok1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "cachedContents": [{"name": "cachedContents/b"}],
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut pager = caches(&server).list(None).await.unwrap();
        assert_eq!(pager.page().len(), 1);
        assert_eq!(pager.page()[0].name.as_deref(), Some("cachedContents/a"));

        let second = pager.next_page().await.unwrap();
        assert_eq!(second[0].name.as_deref(), Some("cachedContents/b"));

        let err = pager.next_page().await.unwrap_err();
        assert!(matches!(err, crate::error::Error::NoMorePages));
        server.verify().await;
    }

    #[tokio::test]
    async fn list_sends_page_size_as_a_query_parameter() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/cachedContents"))
            .and(wiremock::matchers::query_param("pageSize", "5"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"cachedContents": []})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let config = crate::types::ListCachedContentsConfig {
            page_size: Some(5),
            ..Default::default()
        };
        caches(&server).list(Some(config)).await.unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn update_patches_by_name_with_the_ttl_body() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/v1beta/cachedContents/abc123"))
            .and(wiremock::matchers::body_json(
                serde_json::json!({"ttl": "7600s"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "cachedContents/abc123",
                "expireTime": "2026-01-01T02:06:40Z",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let config = crate::types::UpdateCachedContentConfig {
            ttl: Some("7600s".to_owned()),
            ..Default::default()
        };
        let updated = caches(&server)
            .update("cachedContents/abc123", Some(config))
            .await
            .unwrap();
        assert_eq!(updated.expire_time.as_deref(), Some("2026-01-01T02:06:40Z"));
        server.verify().await;
    }

    #[tokio::test]
    async fn delete_removes_by_name_and_deserializes_the_empty_response() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1beta/cachedContents/abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_string(""))
            .expect(1)
            .mount(&server)
            .await;

        let deleted = caches(&server).delete("abc123", None).await.unwrap();
        assert_eq!(deleted.sdk_http_response, None);
        server.verify().await;
    }

    #[tokio::test]
    async fn delete_maps_a_client_error_to_api_error() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": {"code": 404, "message": "not found", "status": "NOT_FOUND"}
            })))
            .mount(&server)
            .await;

        let err = caches(&server).delete("abc123", None).await.unwrap_err();
        match err {
            crate::error::Error::Api(api_err) => assert_eq!(api_err.code, 404),
            other => panic!("expected Error::Api, got {other:?}"),
        }
    }
}
