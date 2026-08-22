//! `client.file_search_stores().documents()`: Document get/list/delete. Mirrors Python's `documents.py`.

use std::{future::Future, pin::Pin, sync::Arc};

use reqwest::Method;
use serde_json::{Map, Value};

use crate::{
    client::Client,
    converters::generated::documents as conv,
    error::Result,
    pager::{PagedItem, Pager},
    types::{
        DeleteDocumentConfig, Document, GetDocumentConfig, ListDocumentsConfig,
        ListDocumentsResponse,
    },
};

/// Parses a (possibly empty) response body as JSON, mirroring Python's
/// `{} if not response.body else json.loads(response.body)`.
fn parse_body(body: &[u8]) -> Result<Value> {
    if body.is_empty() {
        Ok(Value::Object(Map::new()))
    } else {
        Ok(serde_json::from_slice(body)?)
    }
}

/// Reads a string field out of a converted request's `_url` object.
fn take_url_param(request: &Value, key: &str) -> Option<String> {
    request
        .get("_url")
        .and_then(|url| url.get(key))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Removes the `_url`/`_query` bookkeeping keys a generated converter
/// leaves on its output, so the remainder can be sent as the request body.
fn strip_meta(request: &mut Value) {
    if let Some(obj) = request.as_object_mut() {
        obj.remove("_url");
        obj.remove("_query");
    }
}

/// Builds a URL-encoded query string from a converted request's `_query`
/// object, mirroring Python's `urlencode(query_params)`.
fn build_query_string(request: &Value) -> Option<String> {
    let query_obj = request.get("_query")?.as_object()?;
    if query_obj.is_empty() {
        return None;
    }
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in query_obj {
        let value_str = match value {
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            other => other.to_string(),
        };
        serializer.append_pair(key, &value_str);
    }
    Some(serializer.finish())
}

/// Converts a `Serialize`-able config into the `Map<String, Value>` a
/// [`Pager`] uses to track/replay its request config, mirroring Python's
/// `dict(config)` handling in `pagers.py`.
fn config_to_map<C: serde::Serialize>(config: Option<C>) -> Result<Map<String, Value>> {
    match config {
        None => Ok(Map::new()),
        Some(config) => match serde_json::to_value(config)? {
            Value::Object(map) => Ok(map),
            _ => Ok(Map::new()),
        },
    }
}

/// The boxed-closure type used to fetch subsequent pages in
/// [`Documents::list`]. A type alias mainly to keep clippy's
/// `type_complexity` lint quiet.
type FetchListPage<T> = Arc<
    dyn Fn(
            Map<String, Value>,
        ) -> Pin<Box<dyn Future<Output = Result<(Vec<T>, Option<String>)>> + Send>>
        + Send
        + Sync,
>;

/// Handle for `client.file_search_stores().documents()`. Cheap to
/// construct; borrows nothing.
#[derive(Clone)]
pub struct Documents {
    pub(crate) client: Client,
}

impl Documents {
    /// Gets metadata about a Document. Mirrors Python's `Documents.get`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn get(&self, name: &str, config: Option<GetDocumentConfig>) -> Result<Document> {
        let params = serde_json::json!({ "name": name, "config": config });
        let mut request = conv::get_document_parameters_to_mldev(&params, None, None)?;
        let path = take_url_param(&request, "name").unwrap_or_else(|| name.to_owned());
        strip_meta(&mut request);
        let response = self
            .client
            .http()
            .request(Method::GET, &path, None, None, None)
            .await?;
        let wire = parse_body(&response.body)?;
        Ok(serde_json::from_value(wire)?)
    }

    /// Deletes a Document. Mirrors Python's `Documents.delete`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn delete(&self, name: &str, config: Option<DeleteDocumentConfig>) -> Result<()> {
        let params = serde_json::json!({ "name": name, "config": config });
        let mut request = conv::delete_document_parameters_to_mldev(&params, None, None)?;
        let path = take_url_param(&request, "name").unwrap_or_else(|| name.to_owned());
        let query = build_query_string(&request);
        strip_meta(&mut request);
        self.client
            .http()
            .request(Method::DELETE, &path, query.as_deref(), None, None)
            .await?;
        Ok(())
    }

    /// Lists the Documents in a File Search store. Mirrors Python's
    /// `Documents.list`; iterate the returned [`Pager`] (or call
    /// [`Pager::into_stream`]) to walk every page.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn list(
        &self,
        parent: &str,
        config: Option<ListDocumentsConfig>,
    ) -> Result<Pager<Document>> {
        let config_map = config_to_map(config)?;
        let client = self.client.clone();
        let parent_owned = parent.to_owned();
        let (page, next_token) =
            Self::fetch_list_page(&client, &parent_owned, config_map.clone()).await?;
        let fetch_client = client.clone();
        let fetch: FetchListPage<Document> = Arc::new(move |cfg: Map<String, Value>| {
            let client = fetch_client.clone();
            let parent = parent_owned.clone();
            Box::pin(async move { Self::fetch_list_page(&client, &parent, cfg).await })
        });
        Ok(Pager::new(
            PagedItem::Documents,
            page,
            config_map,
            next_token,
            fetch,
        ))
    }

    async fn fetch_list_page(
        client: &Client,
        parent: &str,
        config_map: Map<String, Value>,
    ) -> Result<(Vec<Document>, Option<String>)> {
        let config: ListDocumentsConfig = serde_json::from_value(Value::Object(config_map))?;
        let params = serde_json::json!({ "parent": parent, "config": config });
        let mut request = conv::list_documents_parameters_to_mldev(&params, None, None)?;
        let path = take_url_param(&request, "parent").unwrap_or_else(|| parent.to_owned());
        let query = build_query_string(&request);
        strip_meta(&mut request);
        let response = client
            .http()
            .request(
                Method::GET,
                &format!("{path}/documents"),
                query.as_deref(),
                None,
                None,
            )
            .await?;
        let wire = parse_body(&response.body)?;
        let mldev = conv::list_documents_response_from_mldev(&wire, None, None)?;
        let parsed: ListDocumentsResponse = serde_json::from_value(mldev)?;
        Ok((parsed.documents.unwrap_or_default(), parsed.next_page_token))
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, query_param},
    };

    use super::Documents;
    use crate::{
        client::Client,
        types::{DeleteDocumentConfig, HttpOptions, ListDocumentsConfig},
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

    fn documents(server: &MockServer) -> Documents {
        Documents {
            client: test_client(server.uri()),
        }
    }

    #[tokio::test]
    async fn get_fetches_by_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/fileSearchStores/abc123/documents/doc1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "fileSearchStores/abc123/documents/doc1",
                "displayName": "doc one",
                "state": "STATE_ACTIVE"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let doc = documents(&server)
            .get("fileSearchStores/abc123/documents/doc1", None)
            .await
            .unwrap();
        assert_eq!(doc.display_name.as_deref(), Some("doc one"));
        server.verify().await;
    }

    #[tokio::test]
    async fn delete_sends_force_query_param() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1beta/fileSearchStores/abc123/documents/doc1"))
            .and(query_param("force", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&server)
            .await;

        documents(&server)
            .delete(
                "fileSearchStores/abc123/documents/doc1",
                Some(DeleteDocumentConfig {
                    force: Some(true),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn list_fetches_documents_under_the_parent_store() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/fileSearchStores/abc123/documents"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "documents": [
                    {"name": "fileSearchStores/abc123/documents/doc1"},
                    {"name": "fileSearchStores/abc123/documents/doc2"}
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let pager = documents(&server)
            .list(
                "fileSearchStores/abc123",
                Some(ListDocumentsConfig::default()),
            )
            .await
            .unwrap();
        assert_eq!(pager.page().len(), 2);
        assert_eq!(
            pager.page()[1].name.as_deref(),
            Some("fileSearchStores/abc123/documents/doc2")
        );
        server.verify().await;
    }
}
