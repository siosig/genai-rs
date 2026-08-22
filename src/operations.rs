//! `client.operations()`: long-running operation polling. Mirrors
//! Python's `operations.py` `Operations`.

use reqwest::Method;
use serde_json::Value;

use crate::{
    client::Client,
    converters::generated::{operations as conv, operations_converters as op_conv},
    error::{Error, Result},
};

/// A long-running operation that can be polled via
/// [`Operations::get`].
///
/// Implemented for every operation type this crate's methods return:
/// [`crate::types::GenerateVideosOperation`] (from
/// `models().generate_videos`), [`crate::types::ImportFileOperation`] (from
/// `file_search_stores().import_file`), and
/// [`crate::types::UploadToFileSearchStoreOperation`] (from
/// `file_search_stores().upload_to_file_search_store`). Mirrors Python's
/// `operations.get`, which is generic over `TypeVar('T', bound=types.Operation)`.
pub trait OperationLike: Sized {
    /// The operation's resource name (e.g. `operations/abc123`).
    fn name(&self) -> Option<&str>;

    /// Rebuilds this operation from a raw poll response body.
    ///
    /// Mirrors Python's `Operation.from_api_response` classmethod, which
    /// dispatches to the *type-specific* `_X_Operation_from_mldev`
    /// converter. That step is load-bearing, not cosmetic: the wire shape
    /// nests the payload under keys the Rust type doesn't name directly
    /// (e.g. a completed video operation arrives as
    /// `response.generateVideoResponse.generatedSamples[]`, which the
    /// converter remaps onto `response.generated_videos[]`), so
    /// deserializing the raw body would silently yield an operation with
    /// an empty result.
    ///
    /// # Errors
    /// Returns [`crate::Error::Json`] if `wire` doesn't match this
    /// operation's expected shape.
    fn from_api_response(wire: &Value) -> Result<Self>;
}

impl OperationLike for crate::types::GenerateVideosOperation {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn from_api_response(wire: &Value) -> Result<Self> {
        let mldev = op_conv::generate_videos_operation_from_mldev(wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }
}

impl OperationLike for crate::types::ImportFileOperation {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn from_api_response(wire: &Value) -> Result<Self> {
        let mldev = op_conv::import_file_operation_from_mldev(wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }
}

impl OperationLike for crate::types::UploadToFileSearchStoreOperation {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn from_api_response(wire: &Value) -> Result<Self> {
        let mldev = op_conv::upload_to_file_search_store_operation_from_mldev(wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }
}

/// Handle for `client.operations()`.
#[derive(Clone)]
pub struct Operations {
    pub(crate) client: Client,
}

impl Operations {
    /// Fetches the latest status of a long-running operation, returning
    /// an updated value of the same type. Mirrors Python's
    /// `Operations.get`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Validation`] if `operation` has no `name`,
    /// or [`crate::Error::Api`] for a non-2xx response.
    ///
    /// # Panics
    /// Panics if `get_operation_parameters_to_mldev` doesn't set
    /// `_url.operationName` -- a documented invariant of that generated
    /// converter, not a runtime condition a caller can trigger.
    pub async fn get<T>(&self, operation: &T) -> Result<T>
    where
        T: OperationLike,
    {
        let name = operation
            .name()
            .ok_or_else(|| Error::Validation("operation has no `name` to poll".to_owned()))?;
        let params = serde_json::json!({ "operation_name": name });
        let mut request = conv::get_operation_parameters_to_mldev(&params, None, None)?;
        let request_obj = crate::converters::as_object_mut(&mut request);
        let operation_name = request_obj
            .remove("_url")
            .and_then(|url| url.get("operationName").cloned())
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| {
                panic!("get_operation_parameters_to_mldev always sets _url.operationName")
            });

        let response = self
            .client
            .http()
            .request(Method::GET, &operation_name, None, None, None)
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        T::from_api_response(&wire)
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::Operations;
    use crate::{
        client::Client,
        types::{GenerateVideosOperation, HttpOptions},
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

    #[tokio::test]
    async fn get_polls_the_operation_by_name_and_returns_the_updated_value() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/operations/abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "operations/abc123",
                "done": true,
                "response": {"generateVideoResponse": {"generatedSamples": []}}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let ops = Operations {
            client: test_client(server.uri()),
        };
        let operation = GenerateVideosOperation {
            name: Some("operations/abc123".to_owned()),
            done: Some(false),
            ..Default::default()
        };
        let updated = ops.get(&operation).await.unwrap();
        assert_eq!(updated.done, Some(true));
        server.verify().await;
    }

    #[tokio::test]
    async fn get_rejects_an_operation_without_a_name() {
        let ops = Operations {
            client: test_client("http://127.0.0.1:1".to_owned()),
        };
        let operation = GenerateVideosOperation::default();
        let err = ops.get(&operation).await.unwrap_err();
        assert!(matches!(err, crate::error::Error::Validation(_)));
    }
}
