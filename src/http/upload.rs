//! Resumable file upload (`files.upload`,
//! `file_search_stores.upload_to_file_search_store`), mirroring Python's
//! `_api_client.py` `_upload_fd`/`_async_upload_fd`. See
//! `specs/001-port-genai-rust/contracts/wire-protocol.md` "Resumable Upload".

use bytes::Bytes;
use secrecy::ExposeSecret;
use tokio::io::AsyncReadExt;

use super::HttpClient;
use crate::error::{ApiError, Error, Result};

/// Matches the Python SDK's chunk size (`CHUNK_SIZE = 8 * 1024 * 1024`).
const CHUNK_SIZE: usize = 8 * 1024 * 1024;

const UPLOAD_PROTOCOL_HEADER: &str = "X-Goog-Upload-Protocol";
const UPLOAD_COMMAND_HEADER: &str = "X-Goog-Upload-Command";
const UPLOAD_OFFSET_HEADER: &str = "X-Goog-Upload-Offset";
const UPLOAD_URL_HEADER: &str = "X-Goog-Upload-URL";
const UPLOAD_STATUS_HEADER: &str = "x-goog-upload-status";
const UPLOAD_HEADER_CONTENT_LENGTH: &str = "X-Goog-Upload-Header-Content-Length";
const UPLOAD_HEADER_CONTENT_TYPE: &str = "X-Goog-Upload-Header-Content-Type";

/// Where an upload's bytes come from.
///
/// [`Self::File`] exists so a large upload never has to be resident in
/// memory: chunks are read from disk one [`CHUNK_SIZE`] block at a time, as
/// they are sent, keeping peak usage at roughly one chunk regardless of
/// file size (`spec.md`'s "several hundred MB" edge case, and `plan.md`'s
/// "memory ceiling is one 8 MiB chunk plus constants"). [`Self::Bytes`] is
/// for callers whose data is already in memory.
pub(crate) enum UploadSourceData {
    /// Bytes already held in memory.
    Bytes(Vec<u8>),
    /// An opened file and its length, streamed chunk by chunk.
    File(tokio::fs::File, u64),
}

impl UploadSourceData {
    /// Opens `path`, returning a streaming source and the file's length.
    ///
    /// # Errors
    /// Returns [`Error::Io`] if the file can't be opened or its metadata
    /// can't be read.
    pub(crate) async fn open(path: &std::path::Path) -> Result<Self> {
        let file = tokio::fs::File::open(path).await?;
        let len = file.metadata().await?.len();
        Ok(Self::File(file, len))
    }

    /// The total number of bytes to be uploaded, needed up front for the
    /// `X-Goog-Upload-Header-Content-Length` header.
    pub(crate) fn len(&self) -> u64 {
        match self {
            Self::Bytes(data) => u64::try_from(data.len()).unwrap_or(u64::MAX),
            Self::File(_, len) => *len,
        }
    }

    /// Reads the next chunk into `buf` (clearing it first), returning how
    /// many bytes were read. A short read is retried until the buffer holds
    /// [`CHUNK_SIZE`] bytes or the source is exhausted, so chunk boundaries
    /// stay aligned with the offsets reported to the server.
    async fn next_chunk(&mut self, offset: u64, buf: &mut Vec<u8>) -> Result<usize> {
        buf.clear();
        match self {
            Self::Bytes(data) => {
                let start = usize::try_from(offset)
                    .unwrap_or(usize::MAX)
                    .min(data.len());
                let end = start.saturating_add(CHUNK_SIZE).min(data.len());
                buf.extend_from_slice(&data[start..end]);
                Ok(buf.len())
            }
            Self::File(file, _) => {
                buf.resize(CHUNK_SIZE, 0);
                let mut filled = 0;
                while filled < CHUNK_SIZE {
                    let read = file.read(&mut buf[filled..]).await?;
                    if read == 0 {
                        break;
                    }
                    filled += read;
                }
                buf.truncate(filled);
                Ok(filled)
            }
        }
    }
}

/// Starts a resumable upload session at `start_path` (e.g. `upload/v1beta/files`),
/// sends `data` to the server in [`CHUNK_SIZE`]-sized chunks, and returns
/// the server's final JSON response body (e.g. `{"file": {...}}`).
///
/// # Errors
/// Returns [`Error::Upload`] if the server never returns an upload URL, or
/// if any chunk's `x-goog-upload-status` response header is not `active`
/// (mid-upload) or `final` (last chunk). Returns [`Error::Api`] for a
/// non-2xx response from the start request.
pub(crate) async fn resumable_upload(
    client: &HttpClient,
    start_path: &str,
    start_body: serde_json::Value,
    mime_type: &str,
    mut source: UploadSourceData,
) -> Result<Bytes> {
    let total_size = source.len();
    let start_url = format!("{}{start_path}", client.base_url());
    let response = client
        .reqwest_client()
        .post(&start_url)
        .header("x-goog-api-key", client.api_key().expose_secret())
        .header(UPLOAD_PROTOCOL_HEADER, "resumable")
        .header(UPLOAD_COMMAND_HEADER, "start")
        .header(UPLOAD_HEADER_CONTENT_LENGTH, total_size.to_string())
        .header(UPLOAD_HEADER_CONTENT_TYPE, mime_type)
        .json(&start_body)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let reason = status.canonical_reason().unwrap_or("Unknown").to_owned();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|v| (k.as_str().to_owned(), v.to_owned()))
            })
            .collect();
        let body = response.bytes().await.unwrap_or_default();
        return Err(Error::Api(Box::new(ApiError::from_response(
            status.as_u16(),
            &reason,
            headers,
            &body,
        ))));
    }

    let upload_url = response
        .headers()
        .get(UPLOAD_URL_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Upload("server did not return an upload URL".to_owned()))?
        .to_owned();

    if total_size == 0 {
        return finalize_empty_upload(client, &upload_url).await;
    }

    // One reusable buffer: this is the memory ceiling for the whole upload.
    let mut chunk =
        Vec::with_capacity(CHUNK_SIZE.min(usize::try_from(total_size).unwrap_or(CHUNK_SIZE)));
    let mut offset: u64 = 0;
    while offset < total_size {
        let read = source.next_chunk(offset, &mut chunk).await?;
        if read == 0 {
            return Err(Error::Upload(format!(
                "upload source ended after {offset} bytes, but {total_size} were expected"
            )));
        }
        let end = offset.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        let is_final = end >= total_size;
        let command = if is_final {
            "upload, finalize"
        } else {
            "upload"
        };

        let response = client
            .reqwest_client()
            .post(&upload_url)
            .header(UPLOAD_COMMAND_HEADER, command)
            .header(UPLOAD_OFFSET_HEADER, offset.to_string())
            .body(chunk.clone())
            .send()
            .await?;

        let upload_status = response
            .headers()
            .get(UPLOAD_STATUS_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        offset = end;

        if is_final {
            if upload_status.as_deref() != Some("final") {
                return Err(Error::Upload(format!(
                    "expected final upload status `final`, got {upload_status:?}"
                )));
            }
            return Ok(response.bytes().await?);
        }
        if upload_status.as_deref() != Some("active") {
            return Err(Error::Upload(format!(
                "expected upload status `active`, got {upload_status:?}"
            )));
        }
    }
    Err(Error::Upload(
        "upload loop ended without the server reporting a final chunk".to_owned(),
    ))
}

async fn finalize_empty_upload(client: &HttpClient, upload_url: &str) -> Result<Bytes> {
    let response = client
        .reqwest_client()
        .post(upload_url)
        .header(UPLOAD_COMMAND_HEADER, "upload, finalize")
        .header(UPLOAD_OFFSET_HEADER, "0")
        .body(Vec::new())
        .send()
        .await?;
    let upload_status = response
        .headers()
        .get(UPLOAD_STATUS_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if upload_status.as_deref() != Some("final") {
        return Err(Error::Upload(format!(
            "expected final upload status `final`, got {upload_status:?}"
        )));
    }
    Ok(response.bytes().await?)
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, headers, method, path},
    };

    use super::{UploadSourceData, resumable_upload};
    use crate::{http::HttpClient, types::HttpOptions};

    fn client_for(server: &MockServer) -> HttpClient {
        let options = HttpOptions {
            base_url: Some(server.uri()),
            api_version: Some(String::new()),
            ..Default::default()
        };
        HttpClient::new(SecretString::from("test-key".to_owned()), &options).unwrap()
    }

    #[tokio::test]
    async fn uploads_small_data_in_a_single_finalized_chunk() {
        let server = MockServer::start().await;
        let upload_url = format!("{}/upload-session/abc", server.uri());
        Mock::given(method("POST"))
            .and(path("/upload/v1beta/files"))
            .and(header("X-Goog-Upload-Protocol", "resumable"))
            .and(header("X-Goog-Upload-Command", "start"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", upload_url.as_str()),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/upload-session/abc"))
            // wiremock's `header()` matcher splits the *actual* request
            // header value on commas before comparing (RFC 7230 list
            // convention), so a literal `"upload, finalize"` value must be
            // matched via `headers()` with the pre-split parts.
            .and(headers("X-Goog-Upload-Command", vec!["upload", "finalize"]))
            .and(header("X-Goog-Upload-Offset", "0"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-goog-upload-status", "final")
                    .set_body_json(serde_json::json!({"file": {"name": "files/abc"}})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server);
        let data = b"hello world".to_vec();
        let body = resumable_upload(
            &client,
            "upload/v1beta/files",
            serde_json::json!({}),
            "text/plain",
            UploadSourceData::Bytes(data),
        )
        .await
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["file"]["name"], "files/abc");
    }

    #[tokio::test]
    async fn errors_when_a_chunk_status_is_not_active_or_final() {
        let server = MockServer::start().await;
        let upload_url = format!("{}/upload-session/bad", server.uri());
        Mock::given(method("POST"))
            .and(path("/upload/v1beta/files"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("X-Goog-Upload-URL", upload_url.as_str()),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/upload-session/bad"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("x-goog-upload-status", "cancelled"),
            )
            .mount(&server)
            .await;

        let client = client_for(&server);
        let data = b"x".to_vec();
        let err = resumable_upload(
            &client,
            "upload/v1beta/files",
            serde_json::json!({}),
            "text/plain",
            UploadSourceData::Bytes(data),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, crate::error::Error::Upload(_)));
    }
}

#[cfg(test)]
mod streaming_tests {
    use secrecy::SecretString;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, headers, method, path},
    };

    use super::{CHUNK_SIZE, UploadSourceData, resumable_upload};
    use crate::{http::HttpClient, types::HttpOptions};

    fn client_for(server: &MockServer) -> HttpClient {
        let options = HttpOptions {
            base_url: Some(server.uri()),
            api_version: Some(String::new()),
            ..Default::default()
        };
        // `unwrap` is allowed here: `clippy.toml` sets `allow-unwrap-in-tests`,
        // which covers everything inside a `#[cfg(test)]` module (unlike the
        // integration tests in `tests/`, which are separate crates and do need
        // an explicit `#[expect]`).
        HttpClient::new(SecretString::from("test-key".to_owned()), &options).unwrap()
    }

    /// A file larger than one chunk is split across requests *without* ever
    /// being read into memory whole -- `UploadSourceData::File` pulls one
    /// `CHUNK_SIZE` block at a time. Chunk boundaries and offsets must match
    /// what the in-memory path produces.
    #[tokio::test]
    async fn a_file_source_streams_in_chunks_with_correct_offsets() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/upload/v1beta/files"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Goog-Upload-URL", format!("{}/u", server.uri()).as_str()),
            )
            .expect(1)
            .mount(&server)
            .await;
        // First chunk: offset 0, still active.
        Mock::given(method("POST"))
            .and(path("/u"))
            .and(header("X-Goog-Upload-Offset", "0"))
            .and(header("X-Goog-Upload-Command", "upload"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("x-goog-upload-status", "active"),
            )
            .expect(1)
            .mount(&server)
            .await;
        // Second chunk: offset exactly one chunk in, and finalizes.
        Mock::given(method("POST"))
            .and(path("/u"))
            .and(header(
                "X-Goog-Upload-Offset",
                CHUNK_SIZE.to_string().as_str(),
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-goog-upload-status", "final")
                    .set_body_json(serde_json::json!({"file": {"name": "files/big"}})),
            )
            .expect(1)
            .mount(&server)
            .await;

        // 9 MiB on disk -> 8 MiB chunk + 1 MiB chunk.
        let mut file_path = std::env::temp_dir();
        file_path.push(format!("genai-rs-stream-{}", uuid::Uuid::new_v4()));
        tokio::fs::write(&file_path, vec![7_u8; CHUNK_SIZE + 1024 * 1024])
            .await
            .expect("writing the temp file");

        let source = UploadSourceData::open(&file_path)
            .await
            .expect("opening the temp file");
        assert_eq!(source.len(), (CHUNK_SIZE + 1024 * 1024) as u64);

        let client = client_for(&server);
        let body = resumable_upload(
            &client,
            "upload/v1beta/files",
            serde_json::json!({}),
            "application/octet-stream",
            source,
        )
        .await
        .expect("streaming upload");

        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("parsing the final response");
        assert_eq!(parsed["file"]["name"], "files/big");
        server.verify().await;
        tokio::fs::remove_file(&file_path).await.ok();
    }

    /// An empty file still completes the protocol via the single
    /// finalize-only request, rather than sending a zero-length chunk loop.
    #[tokio::test]
    async fn an_empty_file_source_finalizes_immediately() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/upload/v1beta/files"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Goog-Upload-URL", format!("{}/u", server.uri()).as_str()),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/u"))
            // `header()` compares against the comma-split actual value, so a
            // literal "upload, finalize" never matches; `headers()` takes the
            // pre-split form.
            .and(headers("X-Goog-Upload-Command", vec!["upload", "finalize"]))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-goog-upload-status", "final")
                    .set_body_json(serde_json::json!({"file": {"name": "files/empty"}})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut file_path = std::env::temp_dir();
        file_path.push(format!("genai-rs-empty-{}", uuid::Uuid::new_v4()));
        tokio::fs::write(&file_path, Vec::new())
            .await
            .expect("writing the empty temp file");

        let source = UploadSourceData::open(&file_path)
            .await
            .expect("opening the empty temp file");
        assert_eq!(source.len(), 0);

        let client = client_for(&server);
        let body = resumable_upload(
            &client,
            "upload/v1beta/files",
            serde_json::json!({}),
            "application/octet-stream",
            source,
        )
        .await
        .expect("empty upload");
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("parsing the final response");
        assert_eq!(parsed["file"]["name"], "files/empty");
        server.verify().await;
        tokio::fs::remove_file(&file_path).await.ok();
    }
}
