//! Resumable file upload (`files.upload`,
//! `file_search_stores.upload_to_file_search_store`), mirroring Python's
//! `_api_client.py` `_upload_fd`/`_async_upload_fd`. See
//! `specs/001-port-genai-rust/contracts/wire-protocol.md` "Resumable Upload".

use bytes::Bytes;
use secrecy::ExposeSecret;

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
    data: &[u8],
) -> Result<Bytes> {
    let total_size = data.len() as u64;
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

    let mut offset: usize = 0;
    if data.is_empty() {
        return finalize_empty_upload(client, &upload_url).await;
    }
    while offset < data.len() {
        let end = (offset + CHUNK_SIZE).min(data.len());
        let chunk = &data[offset..end];
        let is_final = end >= data.len();
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
            .body(chunk.to_vec())
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
    unreachable!("loop always returns before exhausting a non-empty `data`")
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
    use wiremock::matchers::{header, headers, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::resumable_upload;
    use crate::http::HttpClient;
    use crate::types::HttpOptions;

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
            &data,
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
            &data,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, crate::error::Error::Upload(_)));
    }
}
