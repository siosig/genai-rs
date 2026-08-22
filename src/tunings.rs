//! `client.tunings()`: fine-tuning job create/get/list/cancel. Mirrors
//! Python's `tunings.py` `Tunings`.
//!
//! `Tunings::list` (and Python's `Tunings.validate_reward`, which this
//! crate does not expose at all) are Vertex-AI-only in the upstream SDK:
//! `Tunings._list` and `Tunings.validate_reward` both raise `ValueError`
//! before building any request unless `api_client.vertexai` is set, and
//! there is no `_to_mldev`/`_from_mldev` converter pair generated for
//! either of them. Since this crate only implements the Gemini Developer
//! API backend, `list` always returns
//! [`crate::Error::UnsupportedByBackend`] and `validate_reward` is
//! omitted entirely; see the method docs on [`Tunings::list`] for details.

use reqwest::Method;
use serde_json::Value;

use crate::{
    client::Client,
    converters::generated::tunings as conv,
    error::{Backend, Error, Result},
    pager::Pager,
    types::{
        CancelTuningJobConfig, CancelTuningJobResponse, CreateTuningJobConfig, GetTuningJobConfig,
        JobState, ListTuningJobsConfig, TuningDataset, TuningJob, TuningOperation,
    },
};

/// Handle for `client.tunings()`. Cheap to construct; borrows nothing.
#[derive(Clone)]
pub struct Tunings {
    pub(crate) client: Client,
}

/// Removes the `_url`/`_query` bookkeeping keys a generated `_to_mldev`
/// converter leaves in its output, returning the string value at
/// `_url.<key>`. What remains in `request` is a plain request body.
///
/// # Errors
/// Returns [`Error::Validation`] if `request` has no `_url.<key>` string
/// value; this indicates a converter/caller mismatch (a programming
/// error in this crate), never a bad response from the API.
fn take_url_field(request: &mut Value, key: &str, converter_name: &'static str) -> Result<String> {
    let missing = || {
        Error::Validation(format!(
            "{converter_name} did not set _url.{key} as expected"
        ))
    };
    let request_obj = request.as_object_mut().ok_or_else(missing)?;
    let url = request_obj.remove("_url");
    request_obj.remove("_query");
    url.and_then(|url| url.get(key).cloned())
        .and_then(|v| v.as_str().map(str::to_owned))
        .ok_or_else(missing)
}

impl Tunings {
    /// Creates a fine-tuning job. Mirrors Python's `Tunings.tune`
    /// (Gemini Developer API branch, `_tune_mldev`).
    ///
    /// The Gemini Developer API responds to a create request with a
    /// long-running `TuningOperation`, not a full `TuningJob`. As in the
    /// Python SDK, this method synthesizes a stub [`TuningJob`] (`name`
    /// plus `state: JOB_STATE_QUEUED`) from that operation rather than
    /// inventing job details the backend didn't send; `name` is taken
    /// from `operation.metadata["tunedModel"]` if present, otherwise from
    /// `operation.name` truncated before `/operations/`. Poll
    /// [`Self::get`] with the returned `name` for the job's actual
    /// current state.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response,
    /// [`crate::Error::UnsupportedByBackend`] if `config` or
    /// `training_dataset` sets a field only the Vertex AI backend
    /// supports (e.g. `training_dataset.gcs_uri`,
    /// `config.tuned_model_display_name` combined with fields like
    /// `description`), or [`crate::Error::Validation`] if the backend
    /// returns an operation with neither `metadata.tunedModel` nor a
    /// `name` to fall back on.
    pub async fn tune(
        &self,
        base_model: &str,
        training_dataset: TuningDataset,
        config: Option<CreateTuningJobConfig>,
    ) -> Result<TuningJob> {
        let http_options = config.as_ref().and_then(|c| c.http_options.clone());
        let params = serde_json::json!({
            "base_model": base_model,
            "training_dataset": training_dataset,
            "config": config,
        });
        let request = conv::create_tuning_job_parameters_private_to_mldev(&params, None, None)?;

        let response = self
            .client
            .http()
            .request(
                Method::POST,
                "tunedModels",
                None,
                Some(request),
                http_options.as_ref(),
            )
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        let mldev = conv::tuning_operation_from_mldev(&wire, None, None)?;
        let operation: TuningOperation = serde_json::from_value(mldev)?;

        let tuned_model_name = operation
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("tunedModel"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let name = if let Some(name) = tuned_model_name {
            name
        } else {
            let operation_name = operation
                .name
                .ok_or_else(|| Error::Validation("operation name is required".to_owned()))?;
            operation_name
                .split_once("/operations/")
                .map_or(operation_name.clone(), |(before, _)| before.to_owned())
        };

        Ok(TuningJob {
            name: Some(name),
            state: Some(JobState::JobStateQueued),
            ..Default::default()
        })
    }

    /// Fetches the latest status of a tuning job. Mirrors Python's
    /// `Tunings.get`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response, or
    /// [`crate::Error::Validation`] if the request converter did not set
    /// the URL name field (a crate-internal invariant violation, not a
    /// caller mistake).
    pub async fn get(&self, name: &str, config: Option<GetTuningJobConfig>) -> Result<TuningJob> {
        let http_options = config.as_ref().and_then(|c| c.http_options.clone());
        let params = serde_json::json!({ "name": name });
        let mut request = conv::get_tuning_job_parameters_to_mldev(&params, None, None)?;
        let path = take_url_field(&mut request, "name", "get_tuning_job_parameters_to_mldev")?;

        let response = self
            .client
            .http()
            .request(Method::GET, &path, None, None, http_options.as_ref())
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        let mldev = conv::tuning_job_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    /// Lists `TuningJob` objects. Mirrors Python's `Tunings.list`.
    ///
    /// # Errors
    /// Always returns [`crate::Error::UnsupportedByBackend`]. Listing
    /// tuning jobs is implemented only for the Vertex AI backend in the
    /// upstream Python SDK: `Tunings._list` raises `ValueError` unless
    /// `api_client.vertexai` is set, before ever building a request, and
    /// there is no `_ListTuningJobsParameters_to_mldev` /
    /// `_ListTuningJobsResponse_from_mldev` converter pair (confirmed by
    /// grepping the generated `converters::generated::tunings` module,
    /// which has no `list_tuning_jobs_*` functions at all). Since this
    /// crate implements only the Gemini Developer API backend, there is
    /// no request this method could faithfully send; it fails fast
    /// instead of fabricating one.
    #[expect(
        clippy::unused_async,
        reason = "kept async for signature parity with this resource's other methods, even though the Gemini Developer API doesn't support this operation and this method never awaits"
    )]
    pub async fn list(&self, _config: Option<ListTuningJobsConfig>) -> Result<Pager<TuningJob>> {
        Err(Error::UnsupportedByBackend {
            field: "tunings().list",
            backend: Backend::VertexAi,
        })
    }

    /// Cancels a tuning job. Mirrors Python's `Tunings.cancel`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response, or
    /// [`crate::Error::Validation`] if the request converter did not set
    /// the URL name field (a crate-internal invariant violation, not a
    /// caller mistake).
    pub async fn cancel(
        &self,
        name: &str,
        config: Option<CancelTuningJobConfig>,
    ) -> Result<CancelTuningJobResponse> {
        let http_options = config.as_ref().and_then(|c| c.http_options.clone());
        let params = serde_json::json!({ "name": name });
        let mut request = conv::cancel_tuning_job_parameters_to_mldev(&params, None, None)?;
        let name = take_url_field(
            &mut request,
            "name",
            "cancel_tuning_job_parameters_to_mldev",
        )?;
        let path = format!("{name}:cancel");

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
        let wire: Value = serde_json::from_slice(&response.body)?;
        let mldev = conv::cancel_tuning_job_response_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, method, path},
    };

    use super::Tunings;
    use crate::{
        client::Client,
        error::{Backend, Error},
        types::{CreateTuningJobConfig, HttpOptions, JobState, TuningDataset, TuningExample},
    };

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

    fn tunings(server: &MockServer) -> Tunings {
        Tunings {
            client: test_client(server.uri()),
        }
    }

    fn dataset() -> TuningDataset {
        TuningDataset {
            examples: Some(vec![TuningExample {
                text_input: Some("2 + 2".to_owned()),
                output: Some("4".to_owned()),
            }]),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn tune_posts_to_tuned_models_and_synthesizes_a_queued_job() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/tunedModels"))
            .and(body_json(
                serde_json::json!({"baseModel": "models/gemini-1.5-flash-001"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "tunedModels/abc123/operations/999",
                "metadata": {"tunedModel": "tunedModels/abc123"},
                "done": false
            })))
            .expect(1)
            .mount(&server)
            .await;

        let job = tunings(&server)
            .tune("models/gemini-1.5-flash-001", dataset(), None)
            .await
            .unwrap();
        assert_eq!(job.name.as_deref(), Some("tunedModels/abc123"));
        assert_eq!(job.state, Some(JobState::JobStateQueued));
        server.verify().await;
    }

    #[tokio::test]
    async fn tune_falls_back_to_the_operation_name_when_metadata_has_no_tuned_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/tunedModels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "tunedModels/xyz789/operations/1",
                "done": false
            })))
            .expect(1)
            .mount(&server)
            .await;

        let job = tunings(&server)
            .tune("models/gemini-1.5-flash-001", dataset(), None)
            .await
            .unwrap();
        assert_eq!(job.name.as_deref(), Some("tunedModels/xyz789"));
        assert_eq!(job.state, Some(JobState::JobStateQueued));
        server.verify().await;
    }

    #[tokio::test]
    async fn tune_rejects_a_vertex_only_config_field() {
        let server = MockServer::start().await;
        let config = CreateTuningJobConfig {
            description: Some("vertex only".to_owned()),
            ..Default::default()
        };
        let err = tunings(&server)
            .tune("models/gemini-1.5-flash-001", dataset(), Some(config))
            .await
            .unwrap_err();
        match err {
            Error::UnsupportedByBackend { field, backend } => {
                assert_eq!(field, "description");
                assert_eq!(backend, Backend::VertexAi);
            }
            other => panic!("expected Error::UnsupportedByBackend, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn tune_rejects_a_vertex_only_training_dataset_field() {
        let server = MockServer::start().await;
        let dataset = TuningDataset {
            gcs_uri: Some("gs://bucket/file.jsonl".to_owned()),
            ..Default::default()
        };
        let err = tunings(&server)
            .tune("models/gemini-1.5-flash-001", dataset, None)
            .await
            .unwrap_err();
        match err {
            Error::UnsupportedByBackend { field, backend } => {
                assert_eq!(field, "gcs_uri");
                assert_eq!(backend, Backend::VertexAi);
            }
            other => panic!("expected Error::UnsupportedByBackend, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_fetches_the_job_by_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/tunedModels/abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "tunedModels/abc123",
                "state": "JOB_STATE_RUNNING",
                "baseModel": "models/gemini-1.5-flash-001"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let job = tunings(&server)
            .get("tunedModels/abc123", None)
            .await
            .unwrap();
        assert_eq!(job.name.as_deref(), Some("tunedModels/abc123"));
        assert_eq!(job.state, Some(JobState::JobStateRunning));
        assert_eq!(
            job.base_model.as_deref(),
            Some("models/gemini-1.5-flash-001")
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn get_maps_a_client_error_to_api_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": {"code": 404, "message": "not found", "status": "NOT_FOUND"}
            })))
            .mount(&server)
            .await;

        let err = tunings(&server)
            .get("tunedModels/missing", None)
            .await
            .unwrap_err();
        match err {
            Error::Api(api_err) => assert_eq!(api_err.code, 404),
            other => panic!("expected Error::Api, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cancel_posts_to_the_cancel_suffix() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/tunedModels/abc123:cancel"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&server)
            .await;

        tunings(&server)
            .cancel("tunedModels/abc123", None)
            .await
            .unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn list_is_unsupported_by_the_gemini_developer_api_backend() {
        let server = MockServer::start().await;
        let err = tunings(&server).list(None).await.unwrap_err();
        match err {
            Error::UnsupportedByBackend { field, backend } => {
                assert_eq!(field, "tunings().list");
                assert_eq!(backend, Backend::VertexAi);
            }
            other => panic!("expected Error::UnsupportedByBackend, got {other:?}"),
        }
    }
}
