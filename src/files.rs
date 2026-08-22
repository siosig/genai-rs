//! `client.files()`: file upload/get/list/delete/download. Mirrors Python's `files.py`.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use reqwest::Method;
use serde_json::{Map, Value};

use crate::client::Client;
use crate::converters::generated::files as conv;
use crate::error::Result;
use crate::pager::{PagedItem, Pager};
use crate::types::{
    DeleteFileConfig, DeleteFileResponse, DownloadFileConfig, File, GetFileConfig, HttpOptions,
    ListFilesConfig, ListFilesResponse, RegisterFilesConfig, RegisterFilesResponse,
    UploadFileConfig,
};

/// The source of bytes for [`Files::upload`]: a local filesystem path (read
/// fully into memory) or an already-in-memory buffer with an explicit MIME
/// type. Mirrors the `file: str | PathLike | IOBase` union Python's
/// `Files.upload` accepts.
pub enum UploadSource {
    /// A local filesystem path. Read fully into memory via
    /// `tokio::fs::read` before uploading.
    Path(PathBuf),
    /// Bytes already in memory.
    Bytes {
        /// The raw file bytes.
        data: Vec<u8>,
        /// The MIME type of `data`, used unless overridden by
        /// [`UploadFileConfig::mime_type`].
        mime_type: String,
    },
}

impl From<PathBuf> for UploadSource {
    fn from(path: PathBuf) -> Self {
        UploadSource::Path(path)
    }
}

impl From<&Path> for UploadSource {
    fn from(path: &Path) -> Self {
        UploadSource::Path(path.to_path_buf())
    }
}

impl From<&str> for UploadSource {
    fn from(path: &str) -> Self {
        UploadSource::Path(PathBuf::from(path))
    }
}

impl From<String> for UploadSource {
    fn from(path: String) -> Self {
        UploadSource::Path(PathBuf::from(path))
    }
}

/// Handle for `client.files()`. Cheap to construct; borrows nothing.
#[derive(Clone)]
pub struct Files {
    pub(crate) client: Client,
}

impl Files {
    /// Uploads `source` as a new [`File`] via the Gemini Developer API's
    /// resumable-upload protocol. Mirrors Python's `Files.upload`.
    ///
    /// The [`UploadSource::Path`] variant is read fully into memory before
    /// uploading; a true streaming-from-disk upload is a possible follow-up
    /// since the underlying resumable-upload primitive already accepts a
    /// `&[u8]` rather than an async reader.
    ///
    /// # Errors
    /// Returns [`crate::Error::Io`] if a `Path` source can't be read,
    /// [`crate::Error::Upload`] if the resumable-upload protocol fails, or
    /// [`crate::Error::Api`] for a non-2xx response.
    pub async fn upload(
        &self,
        source: impl Into<UploadSource>,
        config: Option<UploadFileConfig>,
    ) -> Result<File> {
        // A `Path` source is opened and streamed rather than read into a
        // `Vec`, so uploading a multi-gigabyte file costs one 8 MiB chunk of
        // memory instead of the whole file.
        let (data, mime_type) = match source.into() {
            UploadSource::Path(path) => {
                let mime_type = config
                    .as_ref()
                    .and_then(|c| c.mime_type.clone())
                    .unwrap_or_else(|| {
                        mime_guess::from_path(&path)
                            .first_or_octet_stream()
                            .to_string()
                    });
                (
                    crate::http::upload::UploadSourceData::open(&path).await?,
                    mime_type,
                )
            }
            UploadSource::Bytes { data, mime_type } => {
                let mime_type = config
                    .as_ref()
                    .and_then(|c| c.mime_type.clone())
                    .unwrap_or(mime_type);
                (
                    crate::http::upload::UploadSourceData::Bytes(data),
                    mime_type,
                )
            }
        };

        let mut file_obj = Map::new();
        if let Some(name) = config.as_ref().and_then(|c| c.name.clone()) {
            let name = if name.starts_with("files/") {
                name
            } else {
                format!("files/{name}")
            };
            file_obj.insert("name".to_owned(), Value::String(name));
        }
        if let Some(display_name) = config.as_ref().and_then(|c| c.display_name.clone()) {
            file_obj.insert("displayName".to_owned(), Value::String(display_name));
        }
        file_obj.insert("mimeType".to_owned(), Value::String(mime_type.clone()));
        file_obj.insert(
            "sizeBytes".to_owned(),
            Value::from(i64::try_from(data.len()).unwrap_or(i64::MAX)),
        );

        let params = serde_json::json!({ "file": Value::Object(file_obj) });
        let start_body = conv::create_file_parameters_to_mldev(&params, None, None)?;

        let body = crate::http::upload::resumable_upload(
            self.client.http(),
            "upload/v1beta/files",
            start_body,
            &mime_type,
            data,
        )
        .await?;

        // Mirrors Python's `Files.upload`, which builds the returned `File`
        // directly from `response.json['file']` rather than routing it
        // through `_CreateFileResponse_from_mldev` (that converter only
        // extracts the `sdk_http_response` wrapper, not the file payload).
        let wire: Value = serde_json::from_slice(&body)?;
        let file_value = wire.get("file").cloned().unwrap_or(wire);
        Ok(serde_json::from_value(file_value)?)
    }

    /// Retrieves a `File`'s metadata. Mirrors Python's `Files.get`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Validation`] if `name` is empty, or
    /// [`crate::Error::Api`] for a non-2xx response.
    pub async fn get(&self, name: &str, config: Option<GetFileConfig>) -> Result<File> {
        let file_id = resolve_url_file(conv::get_file_parameters_to_mldev, name)?;
        let path = format!("files/{file_id}");
        let http_options = config.and_then(|c| c.http_options);
        let response = self
            .client
            .http()
            .request(Method::GET, &path, None, None, http_options.as_ref())
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        Ok(serde_json::from_value(wire)?)
    }

    /// Lists `File`s owned by the requesting project. Mirrors Python's
    /// `Files.list`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn list(&self, config: Option<ListFilesConfig>) -> Result<Pager<File>> {
        let config_map = match config {
            Some(config) => serde_json::to_value(config)?
                .as_object()
                .cloned()
                .unwrap_or_default(),
            None => Map::new(),
        };
        let (files, next_page_token) =
            fetch_files_page(self.client.clone(), config_map.clone()).await?;
        let client = self.client.clone();
        let fetch = Arc::new(move |cfg: Map<String, Value>| {
            Box::pin(fetch_files_page(client.clone(), cfg))
                as Pin<Box<dyn Future<Output = Result<(Vec<File>, Option<String>)>> + Send>>
        });
        Ok(Pager::new(
            PagedItem::Files,
            files,
            config_map,
            next_page_token,
            fetch,
        ))
    }

    /// Deletes a remotely stored `File`. Mirrors Python's `Files.delete`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Validation`] if `name` is empty, or
    /// [`crate::Error::Api`] for a non-2xx response.
    pub async fn delete(
        &self,
        name: &str,
        config: Option<DeleteFileConfig>,
    ) -> Result<DeleteFileResponse> {
        let file_id = resolve_url_file(conv::delete_file_parameters_to_mldev, name)?;
        let path = format!("files/{file_id}");
        let http_options = config.and_then(|c| c.http_options);
        let response = self
            .client
            .http()
            .request(Method::DELETE, &path, None, None, http_options.as_ref())
            .await?;
        let wire: Value = if response.body.is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::from_slice(&response.body)?
        };
        let mldev = conv::delete_file_response_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }

    /// Downloads a `File`'s raw bytes (`GET {file}:download?alt=media`).
    /// Mirrors Python's `Files.download`. Only files with a `download_uri`
    /// (i.e. generated, not uploaded, files) can actually be downloaded by
    /// the service; `file` may be a bare id, a `files/...` name, or a full
    /// download URI.
    ///
    /// # Errors
    /// Returns [`crate::Error::Validation`] if `file` is empty, or
    /// [`crate::Error::Api`] for a non-2xx response.
    pub async fn download(&self, file: &str, config: Option<DownloadFileConfig>) -> Result<Bytes> {
        let file_id = crate::transformers::t_file_name(Value::String(file.to_owned()))?;
        let file_id = file_id.as_str().unwrap_or_default();
        let path = format!("files/{file_id}:download");
        let http_options = config.and_then(|c| c.http_options);
        self.client
            .http()
            .download(&path, Some("alt=media"), http_options.as_ref())
            .await
    }

    /// Registers Cloud Storage URIs as `File`s with the file service.
    /// Mirrors Python's internal `Files._register_files`.
    ///
    /// Deviation from Python: the public `Files.register_files` additionally
    /// attaches an OAuth bearer token derived from a
    /// `google.auth.credentials.Credentials` object, which this crate has no
    /// equivalent for (no Vertex/GCP auth support). Callers that need an
    /// `Authorization` header can set one via `config.http_options.headers`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn register_files(
        &self,
        uris: Vec<String>,
        config: Option<RegisterFilesConfig>,
    ) -> Result<RegisterFilesResponse> {
        let http_options = config.and_then(|c| c.http_options);
        let params = serde_json::json!({ "uris": uris });
        let body = conv::internal_register_files_parameters_to_mldev(&params, None, None)?;
        let response = self
            .client
            .http()
            .request(
                Method::POST,
                "files:register",
                None,
                Some(body),
                http_options.as_ref(),
            )
            .await?;
        let wire: Value = serde_json::from_slice(&response.body)?;
        let mldev = conv::register_files_response_from_mldev(&wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }
}

/// Runs a `*_to_mldev` converter (`get`/`delete`) that only ever sets
/// `_url.file`, and extracts that value. Shared by [`Files::get`] and
/// [`Files::delete`].
fn resolve_url_file(
    to_mldev: fn(&Value, Option<&mut Value>, Option<&Value>) -> Result<Value>,
    name: &str,
) -> Result<String> {
    let params = serde_json::json!({ "name": name });
    let mut request = to_mldev(&params, None, None)?;
    let request_obj = crate::converters::as_object_mut(&mut request);
    Ok(request_obj
        .remove("_url")
        .and_then(|url| url.get("file").cloned())
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("get/delete file converters always set _url.file")))
}

async fn fetch_files_page(
    client: Client,
    config: Map<String, Value>,
) -> Result<(Vec<File>, Option<String>)> {
    let http_options: Option<HttpOptions> = config
        .get("http_options")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?;
    let params = serde_json::json!({ "config": Value::Object(config) });
    let mut request = conv::list_files_parameters_to_mldev(&params, None, None)?;
    let request_obj = crate::converters::as_object_mut(&mut request);
    let query = request_obj.remove("_query");
    let query_string = query.as_ref().and_then(query_string_from_value);

    let response = client
        .http()
        .request(
            Method::GET,
            "files",
            query_string.as_deref(),
            None,
            http_options.as_ref(),
        )
        .await?;
    let wire: Value = serde_json::from_slice(&response.body)?;
    let mldev = conv::list_files_response_from_mldev(&wire, None, None)?;
    let list_response: ListFilesResponse = serde_json::from_value(mldev)?;
    Ok((
        list_response.files.unwrap_or_default(),
        list_response.next_page_token,
    ))
}

/// Builds a percent-encoded query string from a converter's `_query` object
/// (e.g. `{"pageSize": 10, "pageToken": "abc"}`), or `None` if it's empty.
fn query_string_from_value(query: &Value) -> Option<String> {
    let obj = query.as_object()?;
    if obj.is_empty() {
        return None;
    }
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in obj {
        match value {
            Value::String(s) => {
                serializer.append_pair(key, s);
            }
            other => {
                serializer.append_pair(key, &other.to_string());
            }
        }
    }
    Some(serializer.finish())
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::{Files, UploadSource};
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

    fn files(server: &MockServer) -> Files {
        Files {
            client: test_client(server.uri()),
        }
    }

    #[tokio::test]
    async fn get_requests_the_files_name_path() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/files/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "files/abc",
                "mimeType": "text/plain",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let file = files(&server).get("files/abc", None).await.unwrap();
        assert_eq!(file.name.as_deref(), Some("files/abc"));
        server.verify().await;
    }

    #[tokio::test]
    async fn upload_bytes_source_does_not_touch_the_filesystem() {
        let server = MockServer::start().await;
        let upload_url = format!("{}/upload-session/xyz", server.uri());
        Mock::given(method("POST"))
            .and(path("/upload/v1beta/files"))
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
                    .set_body_json(serde_json::json!({"file": {"name": "files/xyz", "mimeType": "text/plain"}})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let source = UploadSource::Bytes {
            data: b"hello".to_vec(),
            mime_type: "text/plain".to_owned(),
        };
        let file = files(&server).upload(source, None).await.unwrap();
        assert_eq!(file.name.as_deref(), Some("files/xyz"));
        server.verify().await;
    }
}
