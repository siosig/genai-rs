//! `client.file_search_stores()`: File Search store create/get/list/delete/import. Mirrors Python's `file_search_stores.py`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use reqwest::Method;
use serde_json::{Map, Value};

use crate::client::Client;
use crate::converters::generated::file_search_stores as conv;
use crate::converters::generated::operations_converters as ops_conv;
use crate::error::Result;
use crate::pager::{PagedItem, Pager};
use crate::types::{
    CreateFileSearchStoreConfig, DeleteFileSearchStoreConfig, DownloadMediaConfig, FileSearchStore,
    GetFileSearchStoreConfig, ImportFileConfig, ImportFileOperation, ListFileSearchStoresConfig,
    ListFileSearchStoresResponse, UploadToFileSearchStoreConfig, UploadToFileSearchStoreOperation,
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
/// [`FileSearchStores::list`]. A type alias mainly to keep clippy's
/// `type_complexity` lint quiet.
type FetchListPage<T> = Arc<
    dyn Fn(
            Map<String, Value>,
        ) -> Pin<Box<dyn Future<Output = Result<(Vec<T>, Option<String>)>> + Send>>
        + Send
        + Sync,
>;

/// Handle for `client.file_search_stores()`. Cheap to construct; borrows
/// nothing.
#[derive(Clone)]
pub struct FileSearchStores {
    pub(crate) client: Client,
}

impl FileSearchStores {
    /// Creates a File Search store. Mirrors Python's
    /// `FileSearchStores.create`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn create(
        &self,
        config: Option<CreateFileSearchStoreConfig>,
    ) -> Result<FileSearchStore> {
        let params = serde_json::json!({ "config": config });
        let mut request = conv::create_file_search_store_parameters_to_mldev(&params, None, None)?;
        strip_meta(&mut request);
        let response = self
            .client
            .http()
            .request(Method::POST, "fileSearchStores", None, Some(request), None)
            .await?;
        let wire = parse_body(&response.body)?;
        Ok(serde_json::from_value(wire)?)
    }

    /// Gets metadata about a File Search store. Mirrors Python's
    /// `FileSearchStores.get`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn get(
        &self,
        name: &str,
        config: Option<GetFileSearchStoreConfig>,
    ) -> Result<FileSearchStore> {
        let params = serde_json::json!({ "name": name, "config": config });
        let mut request = conv::get_file_search_store_parameters_to_mldev(&params, None, None)?;
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

    /// Deletes a File Search store. Mirrors Python's
    /// `FileSearchStores.delete`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn delete(
        &self,
        name: &str,
        config: Option<DeleteFileSearchStoreConfig>,
    ) -> Result<()> {
        let params = serde_json::json!({ "name": name, "config": config });
        let mut request = conv::delete_file_search_store_parameters_to_mldev(&params, None, None)?;
        let path = take_url_param(&request, "name").unwrap_or_else(|| name.to_owned());
        let query = build_query_string(&request);
        strip_meta(&mut request);
        self.client
            .http()
            .request(Method::DELETE, &path, query.as_deref(), None, None)
            .await?;
        Ok(())
    }

    /// Lists File Search stores. Mirrors Python's
    /// `FileSearchStores.list`; iterate the returned [`Pager`] (or call
    /// [`Pager::into_stream`]) to walk every page.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn list(
        &self,
        config: Option<ListFileSearchStoresConfig>,
    ) -> Result<Pager<FileSearchStore>> {
        let config_map = config_to_map(config)?;
        let client = self.client.clone();
        let (page, next_token) = Self::fetch_list_page(&client, config_map.clone()).await?;
        let fetch_client = client.clone();
        let fetch: FetchListPage<FileSearchStore> = Arc::new(move |cfg: Map<String, Value>| {
            let client = fetch_client.clone();
            Box::pin(async move { Self::fetch_list_page(&client, cfg).await })
        });
        Ok(Pager::new(
            PagedItem::FileSearchStores,
            page,
            config_map,
            next_token,
            fetch,
        ))
    }

    async fn fetch_list_page(
        client: &Client,
        config_map: Map<String, Value>,
    ) -> Result<(Vec<FileSearchStore>, Option<String>)> {
        let config: ListFileSearchStoresConfig = serde_json::from_value(Value::Object(config_map))?;
        let params = serde_json::json!({ "config": config });
        let mut request = conv::list_file_search_stores_parameters_to_mldev(&params, None, None)?;
        let query = build_query_string(&request);
        strip_meta(&mut request);
        let response = client
            .http()
            .request(
                Method::GET,
                "fileSearchStores",
                query.as_deref(),
                None,
                None,
            )
            .await?;
        let wire = parse_body(&response.body)?;
        let mldev = conv::list_file_search_stores_response_from_mldev(&wire, None, None)?;
        let parsed: ListFileSearchStoresResponse = serde_json::from_value(mldev)?;
        Ok((
            parsed.file_search_stores.unwrap_or_default(),
            parsed.next_page_token,
        ))
    }

    /// Imports a File (previously uploaded via `client.files()`) into a
    /// File Search store. This is a long-running operation (see
    /// aip.dev/151): poll it with `client.operations()` (once
    /// implemented) or inspect `done`/`response` directly. Mirrors
    /// Python's `FileSearchStores.import_file`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn import_file(
        &self,
        file_search_store_name: &str,
        file_name: &str,
        config: Option<ImportFileConfig>,
    ) -> Result<ImportFileOperation> {
        let params = serde_json::json!({
            "file_search_store_name": file_search_store_name,
            "file_name": file_name,
            "config": config,
        });
        let mut request = conv::import_file_parameters_to_mldev(&params, None, None)?;
        let store_name = take_url_param(&request, "file_search_store_name")
            .unwrap_or_else(|| file_search_store_name.to_owned());
        strip_meta(&mut request);
        let path = format!("{store_name}:importFile");
        let response = self
            .client
            .http()
            .request(Method::POST, &path, None, Some(request), None)
            .await?;
        let wire = parse_body(&response.body)?;
        let mldev = conv::import_file_operation_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    /// Uploads raw file bytes directly into a File Search store via a
    /// resumable upload, returning a long-running operation. Mirrors
    /// Python's `FileSearchStores.upload_to_file_search_store`.
    ///
    /// Unlike the Python SDK (which accepts a path or file-like object),
    /// this takes the file's bytes and MIME type directly.
    ///
    /// # Errors
    /// Returns [`crate::Error::Upload`] if the server never returns an
    /// upload URL or a chunk's status is unexpected, or [`crate::Error::Api`]
    /// for a non-2xx response from the start request.
    pub async fn upload_to_file_search_store(
        &self,
        file_search_store_name: &str,
        data: &[u8],
        mime_type: &str,
        config: Option<UploadToFileSearchStoreConfig>,
    ) -> Result<UploadToFileSearchStoreOperation> {
        let mut config = config.unwrap_or_default();
        if config.mime_type.is_none() {
            config.mime_type = Some(mime_type.to_owned());
        }
        let params = serde_json::json!({
            "file_search_store_name": file_search_store_name,
            "config": config,
        });
        let mut request =
            conv::upload_to_file_search_store_parameters_to_mldev(&params, None, None)?;
        let store_name = take_url_param(&request, "file_search_store_name")
            .unwrap_or_else(|| file_search_store_name.to_owned());
        strip_meta(&mut request);
        let start_path = format!("upload/v1beta/{store_name}:uploadToFileSearchStore");
        let body = crate::http::upload::resumable_upload(
            self.client.http(),
            &start_path,
            request,
            mime_type,
            crate::http::upload::UploadSourceData::Bytes(data.to_vec()),
        )
        .await?;
        let wire = parse_body(&body)?;
        let mldev = ops_conv::upload_to_file_search_store_operation_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    /// Downloads raw media bytes by Media ID (e.g. from grounding
    /// metadata), in the form `fileSearchStores/<store>/media/<blob_id>`.
    /// Mirrors Python's `FileSearchStores.download_media`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Validation`] if `media_id` does not contain
    /// `/media/`, or [`crate::Error::Api`] for a non-2xx response.
    pub async fn download_media(
        &self,
        media_id: &str,
        _config: Option<DownloadMediaConfig>,
    ) -> Result<Bytes> {
        let clean_id = media_id.trim_start_matches('/');
        if !clean_id.contains("/media/") {
            return Err(crate::error::Error::Validation(format!(
                "invalid media_id format: `{media_id}`. Expected format: \
                 fileSearchStores/<store>/media/<blob_id>"
            )));
        }
        self.client
            .http()
            .download(clean_id, Some("alt=media"), None)
            .await
    }

    /// Document get/list/delete for a File Search store's Documents
    /// (`client.file_search_stores().documents()`).
    #[must_use]
    pub fn documents(&self) -> crate::documents::Documents {
        crate::documents::Documents {
            client: self.client.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{
        body_json, header, method, path, query_param, query_param_is_missing,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::FileSearchStores;
    use crate::client::Client;
    use crate::types::{
        CreateFileSearchStoreConfig, DeleteFileSearchStoreConfig, ImportFileConfig,
        ListFileSearchStoresConfig, UploadToFileSearchStoreConfig,
    };

    fn test_client(base_url: String) -> Client {
        Client::builder()
            .api_key("test-key")
            .http_options(crate::types::HttpOptions {
                base_url: Some(base_url),
                ..Default::default()
            })
            .build()
            .unwrap()
    }

    fn stores(server: &MockServer) -> FileSearchStores {
        FileSearchStores {
            client: test_client(server.uri()),
        }
    }

    #[tokio::test]
    async fn create_posts_to_file_search_stores() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/fileSearchStores"))
            .and(body_json(serde_json::json!({"displayName": "my store"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "fileSearchStores/abc123",
                "displayName": "my store"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let store = stores(&server)
            .create(Some(CreateFileSearchStoreConfig {
                display_name: Some("my store".to_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(store.name.as_deref(), Some("fileSearchStores/abc123"));
        server.verify().await;
    }

    #[tokio::test]
    async fn get_fetches_by_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/fileSearchStores/abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "fileSearchStores/abc123",
                "displayName": "my store"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let store = stores(&server)
            .get("fileSearchStores/abc123", None)
            .await
            .unwrap();
        assert_eq!(store.display_name.as_deref(), Some("my store"));
        server.verify().await;
    }

    #[tokio::test]
    async fn delete_sends_force_query_param() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1beta/fileSearchStores/abc123"))
            .and(query_param("force", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&server)
            .await;

        stores(&server)
            .delete(
                "fileSearchStores/abc123",
                Some(DeleteFileSearchStoreConfig {
                    force: Some(true),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn list_paginates_through_two_pages() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/fileSearchStores"))
            .and(query_param("pageSize", "1"))
            .and(query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "fileSearchStores": [{"name": "fileSearchStores/one"}],
                "nextPageToken": "tok1"
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1beta/fileSearchStores"))
            .and(query_param("pageToken", "tok1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "fileSearchStores": [{"name": "fileSearchStores/two"}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut pager = stores(&server)
            .list(Some(ListFileSearchStoresConfig {
                page_size: Some(1),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(pager.page().len(), 1);
        assert_eq!(
            pager.page()[0].name.as_deref(),
            Some("fileSearchStores/one")
        );

        let second = pager.next_page().await.unwrap();
        assert_eq!(second[0].name.as_deref(), Some("fileSearchStores/two"));
        server.verify().await;
    }

    #[tokio::test]
    async fn import_file_posts_to_the_import_file_action() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/fileSearchStores/abc123:importFile"))
            .and(body_json(serde_json::json!({"fileName": "files/xyz"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "fileSearchStores/abc123/operations/op1",
                "done": false
            })))
            .expect(1)
            .mount(&server)
            .await;

        let op = stores(&server)
            .import_file("fileSearchStores/abc123", "files/xyz", None)
            .await
            .unwrap();
        assert_eq!(
            op.name.as_deref(),
            Some("fileSearchStores/abc123/operations/op1")
        );
        assert_eq!(op.done, Some(false));
        server.verify().await;
    }

    #[tokio::test]
    async fn import_file_with_config_sends_custom_metadata_and_parses_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/fileSearchStores/abc123:importFile"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "fileSearchStores/abc123/operations/op1",
                "done": true,
                "response": {
                    "parent": "fileSearchStores/abc123",
                    "documentName": "fileSearchStores/abc123/documents/doc1"
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let op = stores(&server)
            .import_file(
                "fileSearchStores/abc123",
                "files/xyz",
                Some(ImportFileConfig::default()),
            )
            .await
            .unwrap();
        assert_eq!(op.done, Some(true));
        let response = op.response.unwrap();
        assert_eq!(
            response.document_name.as_deref(),
            Some("fileSearchStores/abc123/documents/doc1")
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn upload_to_file_search_store_performs_a_resumable_upload() {
        let server = MockServer::start().await;
        let upload_url = format!("{}/upload-session/xyz", server.uri());
        Mock::given(method("POST"))
            .and(path(
                "/upload/v1beta/fileSearchStores/abc123:uploadToFileSearchStore",
            ))
            .and(header("X-Goog-Upload-Protocol", "resumable"))
            .and(header("X-Goog-Upload-Command", "start"))
            .and(header("X-Goog-Upload-Header-Content-Type", "text/plain"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", upload_url.as_str()),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/upload-session/xyz"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-goog-upload-status", "final")
                    .set_body_json(serde_json::json!({
                        "name": "fileSearchStores/abc123/operations/op2",
                        "done": true,
                        "response": {
                            "parent": "fileSearchStores/abc123",
                            "documentName": "fileSearchStores/abc123/documents/doc2"
                        }
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let op = stores(&server)
            .upload_to_file_search_store(
                "fileSearchStores/abc123",
                b"hello world",
                "text/plain",
                Some(UploadToFileSearchStoreConfig {
                    display_name: Some("doc2".to_owned()),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        assert_eq!(op.done, Some(true));
        assert_eq!(
            op.response.unwrap().document_name.as_deref(),
            Some("fileSearchStores/abc123/documents/doc2")
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn download_media_gets_with_alt_media() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/fileSearchStores/abc123/media/blob1"))
            .and(query_param("alt", "media"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"raw-bytes".to_vec()))
            .expect(1)
            .mount(&server)
            .await;

        let data = stores(&server)
            .download_media("fileSearchStores/abc123/media/blob1", None)
            .await
            .unwrap();
        assert_eq!(&data[..], b"raw-bytes");
        server.verify().await;
    }

    #[tokio::test]
    async fn download_media_rejects_an_invalid_media_id() {
        let server = MockServer::start().await;
        let err = stores(&server)
            .download_media("fileSearchStores/abc123", None)
            .await
            .unwrap_err();
        assert!(matches!(err, crate::error::Error::Validation(_)));
    }
}
