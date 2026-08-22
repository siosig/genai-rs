//! `client.operations()`: long-running operation polling. Mirrors
//! Python's `operations.py` `Operations`.

use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::client::Client;
use crate::converters::generated::operations as conv;
use crate::error::{Error, Result};

/// A long-running operation type that carries a resource `name` (e.g.
/// [`crate::types::GenerateVideosOperation`]). Implemented for every
/// generated operation-shaped type this crate exposes.
pub trait OperationLike {
    /// The operation's resource name (e.g. `operations/abc123`).
    fn name(&self) -> Option<&str>;
}

impl OperationLike for crate::types::GenerateVideosOperation {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
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
        T: OperationLike + Serialize + DeserializeOwned,
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
        Ok(serde_json::from_value(wire)?)
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::Operations;
    use crate::client::Client;
    use crate::types::{GenerateVideosOperation, HttpOptions};

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
                "response": {"generatedVideos": []}
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
