//! Environment resources (`client.environments()`): create, get, list and delete environments, and list their files. Mirrors Python's `environments.py`.
//!
//! The items are generated into `crate::gaos` (see `tools/codegen/gen_gaos.py`) and
//! re-exported here under the names the upstream module exports.

pub use crate::gaos::exports::environments::*;

use std::path::PathBuf;

use bytes::Bytes;
use serde_json::Value;

use crate::{
    api_client::upload::{UploadSourceData, resumable_upload_put},
    errors::{Error, Result},
    gaos::wire,
};

/// MIME type used for an upload whose type is neither given nor guessable from the file name.
const DEFAULT_UPLOAD_MIME_TYPE: &str = "application/octet-stream";
/// Prefix of a full environment resource name.
const ENVIRONMENT_PREFIX: &str = "environments/";

/// The bytes of an [`EnvironmentFiles::upload`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentFileSource {
    /// A file on the local filesystem, streamed in chunks.
    Path(PathBuf),
    /// Bytes already in memory (Python: `bytes` or an `io.BytesIO`).
    Bytes(Vec<u8>),
}

/// Options of an [`EnvironmentFiles::upload`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvironmentFileUploadConfig {
    /// MIME type of the file. Defaults to a guess from the file name for a
    /// [`EnvironmentFileSource::Path`], otherwise `application/octet-stream`.
    pub mime_type: Option<String>,
    /// Whether to overwrite the destination file if it already exists.
    pub overwrite: Option<bool>,
    /// Treat the uploaded file as a tar/tar.gz archive and unpack it into the destination path.
    pub extract: Option<bool>,
}

/// Files of an environment workspace (`client.environments().files()`): list,
/// upload and download. Mirrors Python's `client.environments.files`.
#[derive(Debug, Clone)]
pub struct EnvironmentFiles {
    environments: Environments,
}

impl Environments {
    /// The files of an environment workspace: list, upload and download.
    #[must_use]
    pub fn files(&self) -> EnvironmentFiles {
        EnvironmentFiles {
            environments: self.clone(),
        }
    }
}

impl EnvironmentFiles {
    /// Lists directory contents or files inside an environment workspace.
    ///
    /// # Errors
    /// See [`Environments::files_list`].
    pub async fn list(
        &self,
        environment: &str,
        path: &str,
        params: &GetEnvironmentFilesRequest,
    ) -> Result<GetEnvironmentFilesResponse> {
        self.environments
            .files_list(environment, path, params)
            .await
    }

    /// Downloads the bytes of a file in an environment workspace
    /// (`GET environments/{environment}/files/{path}?alt=media`). `environment`
    /// may be a bare id or a full `environments/{id}` name and a leading `/` of
    /// `path` is dropped.
    ///
    /// # Errors
    /// Returns [`Error::Api`] for a non-2xx response and [`Error::Http`] for
    /// transport failures.
    pub async fn download(&self, environment: &str, path: &str) -> Result<Bytes> {
        let environment = if environment.starts_with(ENVIRONMENT_PREFIX) {
            environment.to_owned()
        } else {
            format!("{ENVIRONMENT_PREFIX}{environment}")
        };
        let url_path = format!(
            "{}/files/{}",
            wire::path_segment(&environment),
            wire::path_segment(path.trim_start_matches('/'))
        );
        self.environments
            .client
            .http()
            .download(
                &url_path,
                Some("alt=media"),
                self.environments.http_options.as_ref(),
            )
            .await
    }

    /// Uploads a file (or, with `extract`, an archive) into an environment
    /// workspace over the resumable-upload protocol: a `PUT
    /// /upload/{api_version}/environments/{environment}/files/{path}` handshake
    /// returns the upload URL, then the bytes are sent there.
    ///
    /// The response is normalized like Python: a `files` list, a bare file, or a
    /// `{"file": {...}}` wrapper all become a [`GetEnvironmentFilesResponse`]; any
    /// other JSON object yields an empty one.
    ///
    /// # Errors
    /// Returns [`Error::Validation`] for an empty `environment`, [`Error::Io`] if
    /// the source file cannot be read, [`Error::Upload`] if the handshake returns
    /// no upload URL or a chunk is rejected, and [`Error::Api`] for a non-2xx
    /// handshake response.
    pub async fn upload(
        &self,
        environment: &str,
        path: &str,
        source: EnvironmentFileSource,
        config: &EnvironmentFileUploadConfig,
    ) -> Result<GetEnvironmentFilesResponse> {
        let environment = environment
            .strip_prefix(ENVIRONMENT_PREFIX)
            .unwrap_or(environment);
        if environment.is_empty() {
            return Err(Error::Validation(
                "environment or environment_id is required.".to_owned(),
            ));
        }
        let (data, guessed_mime) = match source {
            EnvironmentFileSource::Bytes(data) => (UploadSourceData::Bytes(data), None),
            EnvironmentFileSource::Path(path) => {
                let guessed = mime_guess::from_path(&path).first().map(|m| m.to_string());
                (UploadSourceData::open(&path).await?, guessed)
            }
        };
        let mime_type = config
            .mime_type
            .clone()
            .or(guessed_mime)
            .unwrap_or_else(|| DEFAULT_UPLOAD_MIME_TYPE.to_owned());

        let options = self.environments.http_options.as_ref();
        let http = self.environments.client.http();
        let api_version = options
            .and_then(|o| o.api_version.as_deref())
            .unwrap_or_else(|| http.api_version())
            .trim_start_matches('/');
        let mut query = wire::Query::default();
        query.push("extract", config.extract.as_ref());
        query.push("overwrite", config.overwrite.as_ref());
        let query = query.finish().map(|q| format!("?{q}")).unwrap_or_default();
        let start_path = format!(
            "upload/{}environments/{}/files/{}{query}",
            if api_version.is_empty() {
                String::new()
            } else {
                format!("{api_version}/")
            },
            wire::path_segment(environment),
            wire::path_segment(path.trim_start_matches('/'))
        );
        let body = resumable_upload_put(http, &start_path, &mime_type, data, options).await?;
        decode_upload_response(&body)
    }
}

/// Mirrors the response handling of Python's `environments.files.upload`.
fn decode_upload_response(body: &[u8]) -> Result<GetEnvironmentFilesResponse> {
    if body.is_empty() {
        return Ok(GetEnvironmentFilesResponse::default());
    }
    let json: Value = serde_json::from_slice(body)?;
    let object = json.as_object();
    if object.is_some_and(|o| o.get("files").is_some_and(Value::is_array)) {
        Ok(serde_json::from_value(json)?)
    } else if object.is_some_and(|o| o.contains_key("name") || o.contains_key("path")) {
        Ok(GetEnvironmentFilesResponse {
            files: Some(vec![serde_json::from_value(json)?]),
            ..Default::default()
        })
    } else if let Some(file) = object.and_then(|o| o.get("file")).filter(|f| f.is_object()) {
        Ok(GetEnvironmentFilesResponse {
            files: Some(vec![serde_json::from_value(file.clone())?]),
            ..Default::default()
        })
    } else {
        Ok(GetEnvironmentFilesResponse::default())
    }
}
