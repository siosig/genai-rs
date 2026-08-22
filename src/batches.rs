//! `client.batches()`: batch job create/get/list/cancel/delete. Mirrors Python's `batches.py`.

use reqwest::Method;
use serde_json::{Map, Value};

use crate::client::Client;
use crate::converters::generated::batches as conv;
use crate::error::Result;
use crate::pager::{PagedItem, Pager};
use crate::types::{
    BatchJob, BatchJobSource, CancelBatchJobConfig, CreateBatchJobConfig,
    CreateEmbeddingsBatchJobConfig, DeleteBatchJobConfig, DeleteResourceJob,
    EmbeddingsBatchJobSource, GetBatchJobConfig, HttpOptions, ListBatchJobsConfig,
    ListBatchJobsResponse,
};

/// The boxed future returned while fetching one `batches().list(...)` page.
type BatchJobsPageFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<(Vec<BatchJob>, Option<String>)>> + Send>,
>;

/// Parses a response body as JSON, treating an empty body as `{}` (mirrors
/// Python's `{} if not response.body else json.loads(response.body)`).
fn response_json(body: &[u8]) -> Result<Value> {
    if body.is_empty() {
        Ok(Value::Object(Map::new()))
    } else {
        Ok(serde_json::from_slice(body)?)
    }
}

/// Builds a `key=value&...` query string from a converter's `_query`
/// object, or `None` if it has no entries.
fn build_query_string(query: &Map<String, Value>) -> Option<String> {
    if query.is_empty() {
        return None;
    }
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in query {
        let rendered = match value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        serializer.append_pair(key, &rendered);
    }
    Some(serializer.finish())
}

/// Removes and returns a `_url.{field}` string set by a `to_mldev`
/// converter.
fn take_url_field(request: &mut Value, field: &str, converter: &str) -> String {
    crate::converters::as_object_mut(request)
        .remove("_url")
        .and_then(|url| url.get(field).cloned())
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("{converter} always sets _url.{field}"))
}

/// Removes and returns a `_query` object set by a `to_mldev` converter, as
/// a `key=value` query string.
fn take_query_string(request: &mut Value) -> Option<String> {
    crate::converters::as_object_mut(request)
        .remove("_query")
        .and_then(|q| q.as_object().cloned())
        .and_then(|m| build_query_string(&m))
}

/// Handle for `client.batches()`. Cheap to construct; borrows nothing.
#[derive(Clone)]
pub struct Batches {
    pub(crate) client: Client,
}

impl Batches {
    /// Creates a batch job. Mirrors Python's `Batches.create`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Validation`] if `src` sets neither, or
    /// both, of `inlined_requests`/`file_name`;
    /// [`crate::Error::UnsupportedByBackend`] if `config` sets a
    /// Vertex-only field; or [`crate::Error::Api`] for a non-2xx response.
    pub async fn create(
        &self,
        model: &str,
        src: impl Into<BatchJobSource>,
        config: Option<CreateBatchJobConfig>,
    ) -> Result<BatchJob> {
        let http_options = config.as_ref().and_then(|c| c.http_options.clone());
        let params = serde_json::json!({
            "model": model,
            "src": src.into(),
            "config": config,
        });
        let mut request = conv::create_batch_job_parameters_to_mldev(&params, None, None)?;
        let model_url = take_url_field(
            &mut request,
            "model",
            "create_batch_job_parameters_to_mldev",
        );
        let path = format!("{model_url}:batchGenerateContent");
        let response = self
            .client
            .http()
            .request(
                Method::POST,
                &path,
                None,
                Some(request),
                http_options.as_ref(),
            )
            .await?;
        let wire = response_json(&response.body)?;
        let mldev = conv::batch_job_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    /// **Experimental.** Creates an embeddings batch job. Mirrors Python's
    /// `Batches.create_embeddings`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn create_embeddings(
        &self,
        model: &str,
        src: EmbeddingsBatchJobSource,
        config: Option<CreateEmbeddingsBatchJobConfig>,
    ) -> Result<BatchJob> {
        let http_options = config.as_ref().and_then(|c| c.http_options.clone());
        let params = serde_json::json!({
            "model": model,
            "src": src,
            "config": config,
        });
        let mut request =
            conv::create_embeddings_batch_job_parameters_to_mldev(&params, None, None)?;
        let model_url = take_url_field(
            &mut request,
            "model",
            "create_embeddings_batch_job_parameters_to_mldev",
        );
        let path = format!("{model_url}:asyncBatchEmbedContent");
        let response = self
            .client
            .http()
            .request(
                Method::POST,
                &path,
                None,
                Some(request),
                http_options.as_ref(),
            )
            .await?;
        let wire = response_json(&response.body)?;
        let mldev = conv::batch_job_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    /// Gets a batch job's current status. Mirrors Python's `Batches.get`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Validation`] if `name` is not a
    /// `batches/{id}` resource name, or [`crate::Error::Api`] for a
    /// non-2xx response.
    pub async fn get(&self, name: &str, config: Option<GetBatchJobConfig>) -> Result<BatchJob> {
        let http_options = config.as_ref().and_then(|c| c.http_options.clone());
        let params = serde_json::json!({ "name": name, "config": config });
        let mut request = conv::get_batch_job_parameters_to_mldev(&params, None, None)?;
        let name_id = take_url_field(&mut request, "name", "get_batch_job_parameters_to_mldev");
        let path = format!("batches/{name_id}");
        let response = self
            .client
            .http()
            .request(Method::GET, &path, None, None, http_options.as_ref())
            .await?;
        let wire = response_json(&response.body)?;
        let mldev = conv::batch_job_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    /// Cancels a running or pending batch job. Mirrors Python's
    /// `Batches.cancel`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Validation`] if `name` is not a
    /// `batches/{id}` resource name, or [`crate::Error::Api`] for a
    /// non-2xx response.
    pub async fn cancel(&self, name: &str, config: Option<CancelBatchJobConfig>) -> Result<()> {
        let http_options = config.as_ref().and_then(|c| c.http_options.clone());
        let params = serde_json::json!({ "name": name, "config": config });
        let mut request = conv::cancel_batch_job_parameters_to_mldev(&params, None, None)?;
        let name_id = take_url_field(&mut request, "name", "cancel_batch_job_parameters_to_mldev");
        let path = format!("batches/{name_id}:cancel");
        self.client
            .http()
            .request(
                Method::POST,
                &path,
                None,
                Some(request),
                http_options.as_ref(),
            )
            .await?;
        Ok(())
    }

    /// Deletes a batch job. Mirrors Python's `Batches.delete`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Validation`] if `name` is not a
    /// `batches/{id}` resource name, or [`crate::Error::Api`] for a
    /// non-2xx response.
    pub async fn delete(
        &self,
        name: &str,
        config: Option<DeleteBatchJobConfig>,
    ) -> Result<DeleteResourceJob> {
        let http_options = config.as_ref().and_then(|c| c.http_options.clone());
        let params = serde_json::json!({ "name": name, "config": config });
        let mut request = conv::delete_batch_job_parameters_to_mldev(&params, None, None)?;
        let name_id = take_url_field(&mut request, "name", "delete_batch_job_parameters_to_mldev");
        let path = format!("batches/{name_id}");
        let response = self
            .client
            .http()
            .request(
                Method::DELETE,
                &path,
                None,
                Some(request),
                http_options.as_ref(),
            )
            .await?;
        let wire = response_json(&response.body)?;
        let mldev = conv::delete_resource_job_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    /// Lists batch jobs, oldest request first. Mirrors Python's
    /// `Batches.list`.
    ///
    /// # Errors
    /// Returns [`crate::Error::UnsupportedByBackend`] if `config` sets the
    /// Vertex-only `filter` field, or [`crate::Error::Api`] for a non-2xx
    /// response.
    pub async fn list(&self, config: Option<ListBatchJobsConfig>) -> Result<Pager<BatchJob>> {
        let config_map = match config {
            Some(config) => match serde_json::to_value(config)? {
                Value::Object(map) => map,
                _ => Map::new(),
            },
            None => Map::new(),
        };
        let (page, next_page_token) = self.fetch_batch_jobs_page(&config_map).await?;

        let client = self.client.clone();
        let fetch = std::sync::Arc::new(move |updated_config: Map<String, Value>| {
            let batches = Batches {
                client: client.clone(),
            };
            let fut: BatchJobsPageFuture =
                Box::pin(async move { batches.fetch_batch_jobs_page(&updated_config).await });
            fut
        });

        Ok(Pager::new(
            PagedItem::BatchJobs,
            page,
            config_map,
            next_page_token,
            fetch,
        ))
    }

    /// Fetches a single page of `batches.list`, given the `config` fields
    /// (`snake_case`, as produced by serializing [`ListBatchJobsConfig`])
    /// with `page_token` already updated for the page being requested.
    async fn fetch_batch_jobs_page(
        &self,
        config: &Map<String, Value>,
    ) -> Result<(Vec<BatchJob>, Option<String>)> {
        let http_options = config
            .get("http_options")
            .and_then(|v| serde_json::from_value::<HttpOptions>(v.clone()).ok());
        let params = serde_json::json!({ "config": Value::Object(config.clone()) });
        let mut request = conv::list_batch_jobs_parameters_to_mldev(&params, None, None)?;
        let query_string = take_query_string(&mut request);
        let response = self
            .client
            .http()
            .request(
                Method::GET,
                "batches",
                query_string.as_deref(),
                None,
                http_options.as_ref(),
            )
            .await?;
        let wire = response_json(&response.body)?;
        let mldev = conv::list_batch_jobs_response_from_mldev(&wire, None, None)?;
        let parsed: ListBatchJobsResponse = serde_json::from_value(mldev)?;
        Ok((
            parsed.batch_jobs.unwrap_or_default(),
            parsed.next_page_token,
        ))
    }
}
