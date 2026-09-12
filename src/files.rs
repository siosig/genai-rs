//! `client.files()`: file upload/get/list/delete/download. Mirrors Python's `files.py`.

use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
};

use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt;
use reqwest::Method;
use serde_json::{Map, Value};
use tokio::io::AsyncWriteExt;

use crate::{
    client::Client,
    converters::generated::files as conv,
    error::{Error, Result},
    pager::{PagedItem, Pager},
    types::{
        DeleteFileConfig, DeleteFileResponse, DownloadFileConfig, File, GeneratedVideo,
        GetFileConfig, HttpOptions, ListFilesConfig, ListFilesResponse, RegisterFilesConfig,
        RegisterFilesResponse, UploadFileConfig, Video,
    },
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

/// Default chunk size for [`Files::download_to_path`]'s writes: matches
/// Python's `download_file`'s `chunk_size` default (1 MiB). See that
/// method's docs for what this does and does not control.
const DOWNLOAD_CHUNK_SIZE: usize = 1024 * 1024;

/// A stream of raw byte chunks, returned by [`Files::download_stream`].
///
/// A concrete (rather than `impl Stream`-returning) type, matching
/// [`crate::models::GenerateContentStream`]'s shape: `download_stream`
/// takes `file: impl Into<FileSource>`, an argument-position `impl Trait`,
/// and Rust 2024's opaque-type capture rules would otherwise force that
/// parameter's anonymous type into the returned stream's hidden type --
/// which then can't be proven `'static`, which
/// [`crate::blocking::BlockingStream`] (used by the generated blocking
/// wrapper) requires. Boxing sidesteps that: `dyn Stream + Send` has no
/// dependency on the caller's argument type at all.
pub struct FileDownloadStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>,
}

impl Stream for FileDownloadStream {
    type Item = Result<Bytes>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

/// What to download via [`Files::download_stream`]/[`Files::download_to_path`]:
/// a bare identifier (name, `files/...` path, or download URI -- resolved
/// the same way [`Files::download`] resolves `file: &str`, with no
/// up-front validity check) or the file's own metadata, which lets this
/// crate check *before* sending anything that it actually has a
/// `download_uri` (uploaded files don't, and can't be downloaded). Mirrors
/// the `file: str | File | Video | GeneratedVideo` union Python's
/// `Files.download` accepts.
pub enum FileSource {
    /// A bare identifier. Not pre-validated -- an undownloadable file only
    /// fails once the server responds (spec 003-upstream-2-23-sync FR-022:
    /// this is a deliberate difference from the `File`/`Video`/
    /// `GeneratedVideo` variants, not an oversight).
    Name(String),
    /// A fetched [`File`]. Rejected up front if `download_uri` is `None`.
    /// Boxed: `File` is by far the largest of this enum's variants (it has
    /// every field the resource can return), and boxing it keeps
    /// [`FileSource`] itself cheap to move.
    File(Box<File>),
    /// A [`Video`], identified by its `uri`.
    Video(Video),
    /// A [`GeneratedVideo`], identified by its `video.uri`.
    GeneratedVideo(GeneratedVideo),
}

impl From<&str> for FileSource {
    fn from(name: &str) -> Self {
        FileSource::Name(name.to_owned())
    }
}

impl From<String> for FileSource {
    fn from(name: String) -> Self {
        FileSource::Name(name)
    }
}

impl From<File> for FileSource {
    fn from(file: File) -> Self {
        FileSource::File(Box::new(file))
    }
}

impl From<&File> for FileSource {
    fn from(file: &File) -> Self {
        FileSource::File(Box::new(file.clone()))
    }
}

impl From<Video> for FileSource {
    fn from(video: Video) -> Self {
        FileSource::Video(video)
    }
}

impl From<&Video> for FileSource {
    fn from(video: &Video) -> Self {
        FileSource::Video(video.clone())
    }
}

impl From<GeneratedVideo> for FileSource {
    fn from(video: GeneratedVideo) -> Self {
        FileSource::GeneratedVideo(video)
    }
}

impl From<&GeneratedVideo> for FileSource {
    fn from(video: &GeneratedVideo) -> Self {
        FileSource::GeneratedVideo(video.clone())
    }
}

impl FileSource {
    /// Resolves to the identifier [`Files::download_stream`] sends to the
    /// server. Ports the object-handling branches of Python's
    /// `_transformers.t_file_name` (`File.name`, `Video.uri`,
    /// `GeneratedVideo.video.uri`) plus, for the file-object case, the
    /// `download_uri is None` check Python's `Files.download` itself makes
    /// (see contracts/download-api.md's "Differences from upstream", #3).
    fn resolve(self) -> Result<String> {
        let raw = match self {
            FileSource::Name(name) => Value::String(name),
            FileSource::File(file) => {
                if file.download_uri.is_none() {
                    return Err(Error::Validation(
                        "only generated files can be downloaded; uploaded files can't be \
                         downloaded -- check `File::download_uri` (or `source`) first"
                            .to_owned(),
                    ));
                }
                Value::String(file.name.or(file.uri).unwrap_or_default())
            }
            FileSource::Video(video) => Value::String(video.uri.unwrap_or_default()),
            FileSource::GeneratedVideo(generated) => {
                Value::String(generated.video.and_then(|v| v.uri).unwrap_or_default())
            }
        };
        let name = crate::transformers::t_file_name(raw)?;
        Ok(name.as_str().unwrap_or_default().to_owned())
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

    /// Downloads a file's data as a stream of chunks, instead of buffering
    /// the whole body into memory. Mirrors Python's `Files.download(...,
    /// destination=<writable stream>)`, minus the writable-stream case --
    /// consume this [`Stream`] with [`futures_util::StreamExt`] to write it
    /// wherever you like, or use [`Self::download_to_path`] for the common
    /// case of writing to a local path.
    ///
    /// Unlike [`Self::download`], `file` also accepts a [`File`],
    /// [`crate::types::Video`], or [`crate::types::GeneratedVideo`]
    /// (anything [`Into<FileSource>`]) -- passing one of those lets this
    /// method check *before* sending anything that the file actually has a
    /// `download_uri` (uploaded files don't, and can't be downloaded).
    /// Passing a bare name/URI string skips that check, same as
    /// [`Self::download`] does today: a string carries no `download_uri` to
    /// check.
    ///
    /// This crate never sets `video_bytes` on a passed-in `Video`/
    /// `GeneratedVideo` the way Python's in-memory `download` does --
    /// there's no single buffer here to hang it on.
    ///
    /// # Errors
    /// Returns [`crate::Error::Validation`] if `file` resolves to an empty
    /// identifier, or a [`FileSource::File`] has no `download_uri`;
    /// [`crate::Error::Api`] for a non-2xx response; or
    /// [`crate::Error::Http`]/[`crate::Error::Stream`] if the connection
    /// fails partway through.
    pub async fn download_stream(
        &self,
        file: impl Into<FileSource>,
        config: Option<DownloadFileConfig>,
    ) -> Result<FileDownloadStream> {
        let file_id = file.into().resolve()?;
        let path = format!("files/{file_id}:download");
        let http_options = config.and_then(|c| c.http_options);
        let inner = self
            .client
            .http()
            .download_stream(&path, Some("alt=media"), http_options.as_ref())
            .await?;
        Ok(FileDownloadStream {
            inner: Box::pin(inner),
        })
    }

    /// Downloads a file directly to a local path, streaming it in
    /// `DOWNLOAD_CHUNK_SIZE`-sized writes rather than holding the whole
    /// file in memory (built on [`Self::download_stream`], which this
    /// consumes). `DOWNLOAD_CHUNK_SIZE` (1 MiB) matches Python's
    /// `download_file`'s `chunk_size` default, but governs only how many
    /// bytes this method batches per write -- unlike Python's
    /// `iter_content(chunk_size=...)`, it does not control how the
    /// underlying network stream is split (that follows TCP/TLS framing;
    /// see [`Self::download_stream`]).
    ///
    /// `destination` is created or truncated, matching Python's
    /// `open(destination, 'wb')`. If the download fails partway through,
    /// the partial file is removed (best-effort) before the error is
    /// returned, so callers never see an incomplete file left behind as if
    /// it had succeeded.
    ///
    /// # Errors
    /// See [`Self::download_stream`], plus [`crate::Error::Io`] if
    /// `destination` can't be created or written to.
    pub async fn download_to_path(
        &self,
        file: impl Into<FileSource>,
        destination: impl AsRef<Path>,
        config: Option<DownloadFileConfig>,
    ) -> Result<()> {
        let stream = self.download_stream(file, config).await?;
        write_stream_to_path(stream, destination.as_ref()).await
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

/// Writes `stream` to `destination` in [`DOWNLOAD_CHUNK_SIZE`]-sized
/// writes, creating/truncating the file. If `stream` yields an `Err` (or
/// the write itself fails) partway through, the partial file is removed
/// (best-effort) before the error is returned -- see
/// [`Files::download_to_path`]'s docs. Factored out of that method (rather
/// than inlined) so its cleanup behaviour can be exercised directly
/// against a synthetic stream in tests, without needing a real partial-body
/// HTTP response (which wiremock has no reliable way to produce -- see
/// `tests/files.rs`'s truncated-body tests for why).
async fn write_stream_to_path(
    mut stream: impl Stream<Item = Result<Bytes>> + Unpin,
    destination: &Path,
) -> Result<()> {
    let mut out = tokio::fs::File::create(destination).await?;

    let result: Result<()> = async {
        let mut buffer = Vec::with_capacity(DOWNLOAD_CHUNK_SIZE);
        while let Some(chunk) = stream.next().await {
            buffer.extend_from_slice(&chunk?);
            if buffer.len() >= DOWNLOAD_CHUNK_SIZE {
                out.write_all(&buffer).await?;
                buffer.clear();
            }
        }
        if !buffer.is_empty() {
            out.write_all(&buffer).await?;
        }
        out.flush().await?;
        Ok(())
    }
    .await;

    if result.is_err() {
        drop(out);
        let _ = tokio::fs::remove_file(destination).await;
    }
    result
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
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::{Files, UploadSource};
    use crate::{client::Client, types::HttpOptions};

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

    /// A genuine mid-stream failure (unlike `tests/files.rs`'s
    /// Content-Length-mismatch tests, which -- as their own comments
    /// explain -- actually fail at the connection level, before any bytes
    /// are ever written): the first item succeeds and is written to disk,
    /// the second is an `Err`. Exercises `write_stream_to_path`'s cleanup
    /// directly against a synthetic stream, sidestepping wiremock/hyper's
    /// unwillingness to serve a body shorter than its own Content-Length.
    #[tokio::test]
    async fn write_stream_to_path_removes_a_partially_written_file_on_a_later_error() {
        let synthetic = futures_util::stream::iter(vec![
            Ok(bytes::Bytes::from_static(
                b"this much gets written before it fails",
            )),
            Err(crate::Error::Validation(
                "synthetic mid-stream failure".to_owned(),
            )),
        ]);

        let mut destination = std::env::temp_dir();
        destination.push(format!(
            "gemini-genai-files-unit-truncated-{}",
            uuid::Uuid::new_v4()
        ));

        let result = super::write_stream_to_path(synthetic, &destination).await;

        assert!(result.is_err(), "the stream's Err must propagate");
        assert!(
            !destination.exists(),
            "a file that had already been partially written must not survive a later stream error"
        );
    }

    /// The success path's counterpart to the test above: multiple `Ok`
    /// items concatenate correctly.
    #[tokio::test]
    async fn write_stream_to_path_concatenates_multiple_items() {
        let synthetic = futures_util::stream::iter(vec![
            Ok(bytes::Bytes::from_static(b"first-")),
            Ok(bytes::Bytes::from_static(b"second-")),
            Ok(bytes::Bytes::from_static(b"third")),
        ]);

        let mut destination = std::env::temp_dir();
        destination.push(format!(
            "gemini-genai-files-unit-concat-{}",
            uuid::Uuid::new_v4()
        ));

        super::write_stream_to_path(synthetic, &destination)
            .await
            .unwrap();

        let written = tokio::fs::read(&destination).await.unwrap();
        assert_eq!(written, b"first-second-third");
        tokio::fs::remove_file(&destination).await.ok();
    }
}
