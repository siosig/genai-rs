//! `client.models()`: text generation, embeddings, token counting, and
//! model listing/management. Mirrors Python's `models.py` `Models`.

use std::pin::Pin;

use futures_core::Stream;
use futures_util::StreamExt;
use reqwest::Method;
use serde_json::{Map, Value};

use crate::client::Client;
use crate::converters::generated::models as conv;
use crate::error::Result;
use crate::types::{Contents, GenerateContentConfig, GenerateContentResponse};

/// A stream of incremental [`GenerateContentResponse`] chunks, returned by
/// [`Models::generate_content_stream`].
pub struct GenerateContentStream {
    inner: Pin<Box<dyn Stream<Item = Result<GenerateContentResponse>> + Send>>,
}

impl Stream for GenerateContentStream {
    type Item = Result<GenerateContentResponse>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

/// Handle for `client.models()`. Cheap to construct; borrows nothing.
#[derive(Clone)]
pub struct Models {
    pub(crate) client: Client,
}

impl Models {
    /// Generates content from a model in a single request. Mirrors
    /// Python's `Models.generate_content`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response, or
    /// [`crate::Error::UnsupportedByBackend`] if `config` sets a field
    /// only the Vertex AI backend supports.
    pub async fn generate_content(
        &self,
        model: &str,
        contents: impl Into<Contents>,
        config: Option<GenerateContentConfig>,
    ) -> Result<GenerateContentResponse> {
        let (path, body) = Self::build_generate_content_request(
            model,
            contents,
            config.as_ref(),
            "generateContent",
        )?;
        let response = self
            .client
            .http()
            .request(Method::POST, &path, None, Some(body), None)
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        let mldev = conv::generate_content_response_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    fn build_generate_content_request(
        model: &str,
        contents: impl Into<Contents>,
        config: Option<&GenerateContentConfig>,
        method_suffix: &str,
    ) -> Result<(String, Value)> {
        let params = serde_json::json!({
            "model": model,
            "contents": Vec::from(contents.into()),
            "config": config,
        });
        let mut request = conv::generate_content_parameters_to_mldev(&params, None, None)?;
        let request_obj = crate::converters::as_object_mut(&mut request);
        let model_url = request_obj
            .remove("_url")
            .and_then(|url| url.get("model").cloned())
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| {
                panic!("generate_content_parameters_to_mldev always sets _url.model")
            });
        request_obj.remove("_query");
        Ok((format!("{model_url}:{method_suffix}"), request))
    }

    /// Generates content, streaming incremental [`GenerateContentResponse`]
    /// chunks as they arrive. Mirrors Python's
    /// `Models.generate_content_stream`.
    ///
    /// # Errors
    /// See [`Self::generate_content`]. A mid-stream failure yields one
    /// `Err` item and then ends the stream.
    pub async fn generate_content_stream(
        &self,
        model: &str,
        contents: impl Into<Contents>,
        config: Option<GenerateContentConfig>,
    ) -> Result<GenerateContentStream> {
        let (path, body) = Self::build_generate_content_request(
            model,
            contents,
            config.as_ref(),
            "streamGenerateContent",
        )?;
        let raw = self
            .client
            .http()
            .request_stream(Method::POST, &path, Some("alt=sse"), Some(body), None)
            .await?;
        let mapped = raw.map(|item| {
            let wire = item?;
            let mldev = conv::generate_content_response_from_mldev(&wire, None, None)?;
            Ok(serde_json::from_value(mldev)?)
        });
        Ok(GenerateContentStream {
            inner: Box::pin(mapped),
        })
    }

    /// Calculates embeddings for the given contents. Mirrors Python's
    /// `Models.embed_content` (`POST {model}:batchEmbedContents`).
    ///
    /// # Errors
    /// See [`Self::generate_content`].
    pub async fn embed_content(
        &self,
        model: &str,
        contents: impl Into<Contents>,
        config: Option<crate::types::EmbedContentConfig>,
    ) -> Result<crate::types::EmbedContentResponse> {
        let params = serde_json::json!({
            "model": model,
            "contents": Vec::from(contents.into()),
            "config": config,
        });
        let mut request = conv::embed_content_parameters_private_to_mldev(&params, None, None)?;
        let request_obj = crate::converters::as_object_mut(&mut request);
        let model_url = extract_url_model(request_obj, "embed_content_parameters_private_to_mldev");
        let response = self
            .client
            .http()
            .request(
                Method::POST,
                &format!("{model_url}:batchEmbedContents"),
                None,
                Some(request),
                None,
            )
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        let mldev = conv::embed_content_response_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    /// Counts the number of tokens in the given content. Mirrors Python's
    /// `Models.count_tokens` (`POST {model}:countTokens`).
    ///
    /// # Errors
    /// See [`Self::generate_content`].
    pub async fn count_tokens(
        &self,
        model: &str,
        contents: impl Into<Contents>,
        config: Option<crate::types::CountTokensConfig>,
    ) -> Result<crate::types::CountTokensResponse> {
        let params = serde_json::json!({
            "model": model,
            "contents": Vec::from(contents.into()),
            "config": config,
        });
        let mut request = conv::count_tokens_parameters_to_mldev(&params, None, None)?;
        let request_obj = crate::converters::as_object_mut(&mut request);
        let model_url = extract_url_model(request_obj, "count_tokens_parameters_to_mldev");
        let response = self
            .client
            .http()
            .request(
                Method::POST,
                &format!("{model_url}:countTokens"),
                None,
                Some(request),
                None,
            )
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        let mldev = conv::count_tokens_response_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    /// **Not supported by the Gemini Developer API** (Vertex AI only, per
    /// Python's `Models.compute_tokens`, which raises unconditionally on a
    /// non-Vertex client).
    ///
    /// # Errors
    /// Always returns [`crate::Error::UnsupportedBackend`].
    #[expect(
        clippy::unused_async,
        reason = "kept async for signature parity with this resource's other methods, even though the Gemini Developer API doesn't support this operation and this method never awaits"
    )]
    pub async fn compute_tokens(
        &self,
        _model: &str,
        _contents: impl Into<Contents>,
        _config: Option<serde_json::Value>,
    ) -> Result<crate::types::ComputeTokensResponse> {
        Err(crate::error::Error::UnsupportedBackend(
            "models.compute_tokens",
        ))
    }

    /// Fetches a model's metadata. Mirrors Python's `Models.get`
    /// (`GET {name}`).
    ///
    /// # Errors
    /// See [`Self::generate_content`].
    pub async fn get(
        &self,
        model: &str,
        config: Option<crate::types::GetModelConfig>,
    ) -> Result<crate::types::Model> {
        let params = serde_json::json!({ "model": model, "config": config });
        let mut request = conv::get_model_parameters_to_mldev(&params, None, None)?;
        let request_obj = crate::converters::as_object_mut(&mut request);
        let name = extract_url_field(request_obj, "name", "get_model_parameters_to_mldev");
        let response = self
            .client
            .http()
            .request(Method::GET, &name, None, None, None)
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        let mldev = conv::model_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    /// Lists available models (or tuned models, if `config.query_base` is
    /// `Some(false)`; defaults to `true` -- base models -- exactly like
    /// Python's `Models.list`). Mirrors `GET models`/`GET tunedModels`.
    ///
    /// # Errors
    /// See [`Self::generate_content`].
    pub async fn list(
        &self,
        config: Option<crate::types::ListModelsConfig>,
    ) -> Result<crate::pager::Pager<crate::types::Model>> {
        let mut config = config.unwrap_or_default();
        if config.query_base.is_none() {
            config.query_base = Some(true);
        }
        let config_value = serde_json::to_value(&config)?;
        let (page, next_page_token) = fetch_models_page(&self.client, &config_value).await?;
        let config_map = match config_value {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        let client = self.client.clone();
        Ok(crate::pager::Pager::new(
            crate::pager::PagedItem::Models,
            page,
            config_map,
            next_page_token,
            std::sync::Arc::new(move |next_config: Map<String, Value>| {
                let client = client.clone();
                Box::pin(
                    async move { fetch_models_page(&client, &Value::Object(next_config)).await },
                )
            }),
        ))
    }

    /// Updates a tuned model's metadata. Mirrors Python's `Models.update`
    /// (`PATCH {name}`).
    ///
    /// # Errors
    /// See [`Self::generate_content`].
    pub async fn update(
        &self,
        model: &str,
        config: crate::types::UpdateModelConfig,
    ) -> Result<crate::types::Model> {
        let params = serde_json::json!({ "model": model, "config": config });
        let mut request = conv::update_model_parameters_to_mldev(&params, None, None)?;
        let request_obj = crate::converters::as_object_mut(&mut request);
        let name = extract_url_field(request_obj, "name", "update_model_parameters_to_mldev");
        let response = self
            .client
            .http()
            .request(Method::PATCH, &name, None, Some(request), None)
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        let mldev = conv::model_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    /// Deletes a tuned model. Mirrors Python's `Models.delete`
    /// (`DELETE {name}`).
    ///
    /// # Errors
    /// See [`Self::generate_content`].
    pub async fn delete(
        &self,
        model: &str,
        config: Option<crate::types::DeleteModelConfig>,
    ) -> Result<crate::types::DeleteModelResponse> {
        let params = serde_json::json!({ "model": model, "config": config });
        let mut request = conv::delete_model_parameters_to_mldev(&params, None, None)?;
        let request_obj = crate::converters::as_object_mut(&mut request);
        let name = extract_url_field(request_obj, "name", "delete_model_parameters_to_mldev");
        let response = self
            .client
            .http()
            .request(Method::DELETE, &name, None, None, None)
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        let mldev = conv::delete_model_response_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    /// Generates images from a text prompt. Mirrors Python's
    /// `Models.generate_images` (`POST {model}:predict`).
    ///
    /// # Deprecated
    /// Matches the Python SDK: superseded by `generate_content` with an
    /// image-capable model. Not removed before 2027-01-01.
    ///
    /// # Errors
    /// See [`Self::generate_content`].
    #[deprecated(
        note = "use generate_content with an image-capable model instead; see https://ai.google.dev/gemini-api/docs/deprecations#imagen-models"
    )]
    pub async fn generate_images(
        &self,
        model: &str,
        prompt: &str,
        config: Option<crate::types::GenerateImagesConfig>,
    ) -> Result<crate::types::GenerateImagesResponse> {
        let params = serde_json::json!({ "model": model, "prompt": prompt, "config": config });
        let mut request = conv::generate_images_parameters_to_mldev(&params, None, None)?;
        let request_obj = crate::converters::as_object_mut(&mut request);
        let model_url = extract_url_model(request_obj, "generate_images_parameters_to_mldev");
        let response = self
            .client
            .http()
            .request(
                Method::POST,
                &format!("{model_url}:predict"),
                None,
                Some(request),
                None,
            )
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        let mldev = conv::generate_images_response_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    /// Starts generating videos from a prompt/image/video source,
    /// returning an in-progress [`crate::types::GenerateVideosOperation`]
    /// (poll it via `client.operations().get(&operation)`). Mirrors
    /// Python's `Models.generate_videos` (`POST {model}:predictLongRunning`).
    ///
    /// # Errors
    /// See [`Self::generate_content`].
    pub async fn generate_videos(
        &self,
        model: &str,
        source: crate::types::GenerateVideosSource,
        config: Option<crate::types::GenerateVideosConfig>,
    ) -> Result<crate::types::GenerateVideosOperation> {
        let params = serde_json::json!({ "model": model, "source": source, "config": config });
        let mut request = conv::generate_videos_parameters_to_mldev(&params, None, None)?;
        let request_obj = crate::converters::as_object_mut(&mut request);
        let model_url = extract_url_model(request_obj, "generate_videos_parameters_to_mldev");
        let response = self
            .client
            .http()
            .request(
                Method::POST,
                &format!("{model_url}:predictLongRunning"),
                None,
                Some(request),
                None,
            )
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        let mldev = conv::generate_videos_operation_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }
}

/// Fetches one page of `models().list(...)`, given a request config
/// already shaped as `{"page_size", "page_token", "filter", "query_base"}`.
/// Shared between [`Models::list`] and the returned [`crate::pager::Pager`]'s
/// fetch closure (which calls back into this for every subsequent page).
async fn fetch_models_page(
    client: &Client,
    config_value: &Value,
) -> Result<(Vec<crate::types::Model>, Option<String>)> {
    let params = serde_json::json!({ "config": config_value });
    let mut request = conv::list_models_parameters_to_mldev(&params, None, None)?;
    let request_obj = crate::converters::as_object_mut(&mut request);
    let models_url =
        extract_url_field(request_obj, "models_url", "list_models_parameters_to_mldev");
    let query = request_obj
        .remove("_query")
        .and_then(|q| q.as_object().cloned())
        .map(|q| {
            q.into_iter()
                .filter_map(|(k, v)| v.as_str().map(|s| format!("{k}={}", urlencoding_light(s))))
                .collect::<Vec<_>>()
                .join("&")
        });
    let response = client
        .http()
        .request(Method::GET, &models_url, query.as_deref(), None, None)
        .await?;
    let wire: Value = serde_json::from_slice(&response.body)?;
    let mldev = conv::list_models_response_from_mldev(&wire, None, None)?;
    let page: Vec<crate::types::Model> = mldev
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(serde_json::from_value)
        .collect::<std::result::Result<_, _>>()?;
    let next_page_token = mldev
        .get("next_page_token")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok((page, next_page_token))
}

fn extract_url_model(request_obj: &mut Map<String, Value>, converter_name: &'static str) -> String {
    extract_url_field(request_obj, "model", converter_name)
}

fn extract_url_field(
    request_obj: &mut Map<String, Value>,
    key: &str,
    converter_name: &'static str,
) -> String {
    request_obj
        .remove("_url")
        .and_then(|url| url.get(key).cloned())
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("{converter_name} always sets _url.{key}"))
}

/// Minimal query-string value encoding (percent-encodes spaces and `&`/`=`,
/// which is sufficient for the filter/page-token values this module sends;
/// full RFC 3986 encoding is not required here since Gemini API query
/// values are simple identifiers/tokens).
fn urlencoding_light(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::Models;
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

    fn models(server: &MockServer) -> Models {
        Models {
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
    async fn generate_content_posts_to_the_model_generate_content_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/models/gemini-2.5-flash:generateContent"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "candidates": [{
                    "content": {"role": "model", "parts": [{"text": "hello"}]},
                    "finishReason": "STOP"
                }],
                "usageMetadata": {"promptTokenCount": 3, "totalTokenCount": 8}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let response = models(&server)
            .generate_content("gemini-2.5-flash", "hi", None)
            .await
            .unwrap();
        assert_eq!(response.text().as_deref(), Some("hello"));
        assert_eq!(response.usage_metadata.unwrap().total_token_count, Some(8));
        assert_eq!(
            response.candidates.unwrap()[0].finish_reason,
            Some(crate::types::FinishReason::Stop)
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn generate_content_sends_generation_config_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/models/gemini-2.5-flash:generateContent"))
            .and(body_json(serde_json::json!({
                "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
                "generationConfig": {"temperature": 0.2, "maxOutputTokens": 16}
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"candidates": []})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let config = crate::types::GenerateContentConfig {
            temperature: Some(0.2),
            max_output_tokens: Some(16),
            ..Default::default()
        };
        models(&server)
            .generate_content("gemini-2.5-flash", "hi", Some(config))
            .await
            .unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn generate_content_maps_a_client_error_to_api_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": {"code": 401, "message": "bad key", "status": "UNAUTHENTICATED"}
            })))
            .mount(&server)
            .await;

        let err = models(&server)
            .generate_content("gemini-2.5-flash", "hi", None)
            .await
            .unwrap_err();
        match err {
            crate::error::Error::Api(api_err) => {
                assert_eq!(api_err.code, 401);
                assert!(api_err.is_client_error());
            }
            other => panic!("expected Error::Api, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn generate_content_deserializes_unknown_response_fields_without_failing() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "candidates": [],
                "totallyNewFieldFromTheFuture": {"anything": true}
            })))
            .mount(&server)
            .await;

        let response = models(&server)
            .generate_content("gemini-2.5-flash", "hi", None)
            .await
            .unwrap();
        assert_eq!(response.candidates, Some(vec![]));
    }

    #[tokio::test]
    async fn generate_content_stream_yields_chunks_in_order() {
        use futures_util::StreamExt;

        let server = MockServer::start().await;
        let sse_body = concat!(
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hel\"}]}}]}\n\n",
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"lo\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"totalTokenCount\":5}}\n\n",
        );
        Mock::given(method("POST"))
            .and(path(
                "/v1beta/models/gemini-2.5-flash:streamGenerateContent",
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(sse_body)
                    .insert_header("content-type", "text/event-stream"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut stream = models(&server)
            .generate_content_stream("gemini-2.5-flash", "hi", None)
            .await
            .unwrap();

        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(first.text().as_deref(), Some("Hel"));
        let second = stream.next().await.unwrap().unwrap();
        assert_eq!(second.text().as_deref(), Some("lo"));
        assert_eq!(second.usage_metadata.unwrap().total_token_count, Some(5));
        assert!(stream.next().await.is_none());
        server.verify().await;
    }

    #[tokio::test]
    async fn embed_content_posts_to_batch_embed_contents() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/models/text-embedding-004:batchEmbedContents"))
            .and(body_json(serde_json::json!({
                "requests": [
                    {"model": "models/text-embedding-004", "content": {"role": "user", "parts": [{"text": "a"}]}},
                    {"model": "models/text-embedding-004", "content": {"role": "user", "parts": [{"text": "b"}]}}
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "embeddings": [{"values": [0.1, 0.2]}, {"values": [0.3, 0.4]}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let response = models(&server)
            .embed_content(
                "text-embedding-004",
                vec![
                    crate::types::Content::from("a"),
                    crate::types::Content::from("b"),
                ],
                None,
            )
            .await
            .unwrap();
        let embeddings = response.embeddings.unwrap();
        assert_eq!(embeddings.len(), 2);
        assert_eq!(embeddings[0].values.as_deref(), Some([0.1, 0.2].as_slice()));
        server.verify().await;
    }

    #[tokio::test]
    async fn count_tokens_posts_and_parses_total() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/models/gemini-2.5-flash:countTokens"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"totalTokens": 5})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let response = models(&server)
            .count_tokens("gemini-2.5-flash", "hi", None)
            .await
            .unwrap();
        assert_eq!(response.total_tokens, Some(5));
        server.verify().await;
    }

    #[tokio::test]
    async fn compute_tokens_is_unsupported_on_the_gemini_api_backend() {
        let server = MockServer::start().await;
        let err = models(&server)
            .compute_tokens("gemini-2.5-flash", "hi", None)
            .await
            .unwrap_err();
        assert!(matches!(err, crate::error::Error::UnsupportedBackend(_)));
    }

    #[tokio::test]
    async fn get_fetches_a_model_by_resource_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/models/gemini-2.5-flash"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "models/gemini-2.5-flash",
                "displayName": "Gemini 2.5 Flash"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let model = models(&server).get("gemini-2.5-flash", None).await.unwrap();
        assert_eq!(model.name.as_deref(), Some("models/gemini-2.5-flash"));
        assert_eq!(model.display_name.as_deref(), Some("Gemini 2.5 Flash"));
        server.verify().await;
    }

    #[tokio::test]
    async fn list_defaults_query_base_to_true_and_pages() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/models"))
            .and(wiremock::matchers::query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": [{"name": "models/a"}],
                "nextPageToken": "tok1"
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1beta/models"))
            .and(wiremock::matchers::query_param("pageToken", "tok1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": [{"name": "models/b"}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut pager = models(&server).list(None).await.unwrap();
        assert_eq!(pager.page().len(), 1);
        assert_eq!(pager.page()[0].name.as_deref(), Some("models/a"));
        let second = pager.next_page().await.unwrap();
        assert_eq!(second[0].name.as_deref(), Some("models/b"));
        server.verify().await;
    }

    #[tokio::test]
    async fn list_uses_tuned_models_collection_when_query_base_is_false() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/tunedModels"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"models": []})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let config = crate::types::ListModelsConfig {
            query_base: Some(false),
            ..Default::default()
        };
        models(&server).list(Some(config)).await.unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn update_patches_a_tuned_model() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/v1beta/tunedModels/my-model"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"name": "tunedModels/my-model"})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let config = crate::types::UpdateModelConfig {
            display_name: Some("New Name".to_owned()),
            ..Default::default()
        };
        let model = models(&server)
            .update("tunedModels/my-model", config)
            .await
            .unwrap();
        assert_eq!(model.name.as_deref(), Some("tunedModels/my-model"));
        server.verify().await;
    }

    #[tokio::test]
    async fn delete_removes_a_tuned_model() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1beta/tunedModels/my-model"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&server)
            .await;

        models(&server)
            .delete("tunedModels/my-model", None)
            .await
            .unwrap();
        server.verify().await;
    }

    #[tokio::test]
    #[allow(
        deprecated,
        reason = "testing the deprecated generate_images method itself"
    )]
    async fn generate_images_posts_prompt_to_predict() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/models/imagen-3.0-generate-002:predict"))
            .and(body_json(
                serde_json::json!({"instances": [{"prompt": "a cat"}]}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "predictions": [{"bytesBase64Encoded": "aGVsbG8=", "mimeType": "image/png"}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let response = models(&server)
            .generate_images("imagen-3.0-generate-002", "a cat", None)
            .await
            .unwrap();
        assert_eq!(response.generated_images.unwrap().len(), 1);
        server.verify().await;
    }

    #[tokio::test]
    async fn generate_videos_posts_to_predict_long_running_and_parses_operation() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/v1beta/models/veo-2.0-generate-001:predictLongRunning",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "operations/abc123",
                "done": false
            })))
            .expect(1)
            .mount(&server)
            .await;

        let source = crate::types::GenerateVideosSource {
            prompt: Some("a neon hologram of a cat".to_owned()),
            ..Default::default()
        };
        let operation = models(&server)
            .generate_videos("veo-2.0-generate-001", source, None)
            .await
            .unwrap();
        assert_eq!(operation.name.as_deref(), Some("operations/abc123"));
        assert_eq!(operation.done, Some(false));
        server.verify().await;
    }
}
