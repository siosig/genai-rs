//! Maps an upstream `test_method` (a dotted client path such as
//! `models.generate_content`) to the matching call on this crate.
//!
//! Corpus `parameters` are the Python method's keyword arguments, dumped
//! from pydantic (camelCase JSON values). Each arm deserializes the fields it
//! needs into this crate's typed inputs; where Python accepts a looser shape
//! (a bare string for `contents`, a list of requests for `src`, separate
//! `prompt`/`image`/`video` arguments) the helpers below apply the same
//! normalization the Python `_transformers` module does.

use gemini_genai::{
    Client,
    types::{
        BatchJobSource, Content, Contents, EmbeddingsBatchJobSource, GenerateVideosSource, Part,
        TuningDataset,
    },
};
use serde::de::DeserializeOwned;
use serde_json::Value;

/// How a dispatched call ended.
pub enum Outcome {
    /// The call returned `Ok`.
    Succeeded,
    /// The crate returned an error (its `Display` text).
    Failed(String),
    /// The harness could not run the case (unknown method, bad parameters).
    Harness(String),
}

/// An early exit from an arm: either the crate failed or the harness did.
enum Halt {
    Failed(String),
    Harness(String),
}

impl From<gemini_genai::Error> for Halt {
    fn from(error: gemini_genai::Error) -> Self {
        Self::Failed(error.to_string())
    }
}

type Run = Result<(), Halt>;

const MODEL: &str = "model";
const CONFIG: &str = "config";
const NAME: &str = "name";
const PARENT: &str = "parent";
const CONTENTS: &str = "contents";
const PROMPT: &str = "prompt";
const IMAGE: &str = "image";
const VIDEO: &str = "video";
const SOURCE: &str = "source";
const SRC: &str = "src";
const URIS: &str = "uris";
const BASE_MODEL: &str = "base_model";
const TRAINING_DATASET: &str = "training_dataset";
const SYSTEM_INSTRUCTION_KEYS: [&str; 2] = ["systemInstruction", "system_instruction"];
const SPEECH_CONFIG_KEYS: [&str; 2] = ["speechConfig", "speech_config"];
const DEST: &str = "dest";
const HTTP_OPTIONS_KEYS: [&str; 2] = ["httpOptions", "http_options"];
const BASE_URL_KEYS: [&str; 2] = ["baseUrl", "base_url"];
const TEXT_KEY: &str = "text";
const USER_ROLE: &str = "user";
const PARTS_KEY: &str = "parts";
const ROLE_KEY: &str = "role";
const GCS_PREFIX: &str = "gs://";
const BIGQUERY_PREFIX: &str = "bq://";
const FILES_PREFIX: &str = "files/";
const FORMAT_JSONL: &str = "jsonl";
const FORMAT_BIGQUERY: &str = "bigquery";

const AUTH_TOKENS_CREATE: &str = "auth_tokens.create";
const BATCHES_CANCEL: &str = "batches.cancel";
const BATCHES_CREATE: &str = "batches.create";
const BATCHES_CREATE_EMBEDDINGS: &str = "batches.create_embeddings";
const BATCHES_DELETE: &str = "batches.delete";
const BATCHES_GET: &str = "batches.get";
const BATCHES_LIST: &str = "batches.list";
const CACHES_CREATE: &str = "caches.create";
const CACHES_DELETE: &str = "caches.delete";
const CACHES_GET: &str = "caches.get";
const CACHES_LIST: &str = "caches.list";
const CACHES_UPDATE: &str = "caches.update";
const FILE_SEARCH_STORES_CREATE: &str = "file_search_stores.create";
const FILE_SEARCH_STORES_DELETE: &str = "file_search_stores.delete";
const FILE_SEARCH_STORES_GET: &str = "file_search_stores.get";
const FILE_SEARCH_STORES_LIST: &str = "file_search_stores.list";
const DOCUMENTS_DELETE: &str = "file_search_stores.documents.delete";
const DOCUMENTS_GET: &str = "file_search_stores.documents.get";
const DOCUMENTS_LIST: &str = "file_search_stores.documents.list";
const FILES_REGISTER_FILES: &str = "files._register_files";
const FILES_DELETE: &str = "files.delete";
const FILES_GET: &str = "files.get";
const FILES_LIST: &str = "files.list";
const MODELS_COMPUTE_TOKENS: &str = "models.compute_tokens";
const MODELS_COUNT_TOKENS: &str = "models.count_tokens";
const MODELS_DELETE: &str = "models.delete";
const MODELS_EMBED_CONTENT: &str = "models.embed_content";
const MODELS_GENERATE_CONTENT: &str = "models.generate_content";
const MODELS_GENERATE_IMAGES: &str = "models.generate_images";
const MODELS_GENERATE_VIDEOS: &str = "models.generate_videos";
const MODELS_GET: &str = "models.get";
const MODELS_LIST: &str = "models.list";
const TUNINGS_GET: &str = "tunings.get";
const TUNINGS_LIST: &str = "tunings.list";
const TUNINGS_TUNE: &str = "tunings.tune";

/// Every `test_method` this module can run.
pub const HANDLED_METHODS: &[&str] = &[
    AUTH_TOKENS_CREATE,
    BATCHES_CANCEL,
    BATCHES_CREATE,
    BATCHES_CREATE_EMBEDDINGS,
    BATCHES_DELETE,
    BATCHES_GET,
    BATCHES_LIST,
    CACHES_CREATE,
    CACHES_DELETE,
    CACHES_GET,
    CACHES_LIST,
    CACHES_UPDATE,
    FILE_SEARCH_STORES_CREATE,
    FILE_SEARCH_STORES_DELETE,
    FILE_SEARCH_STORES_GET,
    FILE_SEARCH_STORES_LIST,
    DOCUMENTS_DELETE,
    DOCUMENTS_GET,
    DOCUMENTS_LIST,
    FILES_REGISTER_FILES,
    FILES_DELETE,
    FILES_GET,
    FILES_LIST,
    MODELS_COMPUTE_TOKENS,
    MODELS_COUNT_TOKENS,
    MODELS_DELETE,
    MODELS_EMBED_CONTENT,
    MODELS_GENERATE_CONTENT,
    MODELS_GENERATE_IMAGES,
    MODELS_GENERATE_VIDEOS,
    MODELS_GET,
    MODELS_LIST,
    TUNINGS_GET,
    TUNINGS_LIST,
    TUNINGS_TUNE,
];

/// Rewrites the shorthand forms Python's pydantic models accept for a field
/// (a bare string for a system instruction, a voice name for a speech
/// config) into the object form this crate's typed fields take, matching
/// what `t_content` / `t_speech_config` produce.
fn lift_unions(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, child) in object.iter_mut() {
                if SYSTEM_INSTRUCTION_KEYS.contains(&key.as_str())
                    && let Some(text) = child.as_str()
                {
                    *child = serde_json::json!({
                        PARTS_KEY: [{ TEXT_KEY: text }],
                        ROLE_KEY: USER_ROLE,
                    });
                } else if SPEECH_CONFIG_KEYS.contains(&key.as_str())
                    && let Some(voice) = child.as_str()
                {
                    *child = serde_json::json!({
                        "voiceConfig": { "prebuiltVoiceConfig": { "voiceName": voice } },
                    });
                } else {
                    lift_unions(child);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(lift_unions),
        _ => {}
    }
}

/// Drops `baseUrl` from every per-request `httpOptions`. The corpus records
/// request paths relative to the origin (Python's capture transport ignored
/// the host), and here the mock server is the origin, so a real host name in
/// the case must not redirect the request.
fn drop_base_urls(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, child) in object.iter_mut() {
                if HTTP_OPTIONS_KEYS.contains(&key.as_str())
                    && let Some(options) = child.as_object_mut()
                {
                    for base_url_key in BASE_URL_KEYS {
                        options.remove(base_url_key);
                    }
                }
                drop_base_urls(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(drop_base_urls),
        _ => {}
    }
}

/// Mirrors Python's `t_batch_job_destination` for a bare-string `config.dest`.
fn lift_batch_destination(config: &mut Value) -> Result<(), Halt> {
    let Some(dest) = config.get_mut(DEST) else {
        return Ok(());
    };
    let Some(uri) = dest.as_str().map(str::to_owned) else {
        return Ok(());
    };
    *dest = if uri.starts_with(GCS_PREFIX) {
        serde_json::json!({ "format": FORMAT_JSONL, "gcsUri": uri })
    } else if uri.starts_with(BIGQUERY_PREFIX) {
        serde_json::json!({ "format": FORMAT_BIGQUERY, "bigqueryUri": uri })
    } else {
        return Err(Halt::Failed(format!("Unsupported destination: {uri}")));
    };
    Ok(())
}

/// Deserializes `params[key]` (a missing key reads as `null`).
fn field<T: DeserializeOwned>(params: &Value, key: &str) -> Result<T, Halt> {
    serde_json::from_value(params.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|error| Halt::Harness(format!("parameter `{key}`: {error}")))
}

fn parse<T: DeserializeOwned>(value: &Value, what: &str) -> Result<T, Halt> {
    serde_json::from_value(value.clone()).map_err(|error| Halt::Harness(format!("{what}: {error}")))
}

/// A JSON object is a `Content` when it carries `parts` or `role`, else a `Part`
/// (the distinction Python's `t_contents` draws with pydantic validation).
fn is_content(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.contains_key(PARTS_KEY) || object.contains_key(ROLE_KEY))
}

fn part_from(value: &Value) -> Result<Part, Halt> {
    match value.as_str() {
        Some(text) => Ok(Part::from_text(text)),
        None => parse(value, "part"),
    }
}

/// Mirrors Python's `t_contents`: consecutive parts (or strings) are grouped
/// into one turn, whole `Content` items stand on their own.
fn contents_from(params: &Value) -> Result<Contents, Halt> {
    let value = params.get(CONTENTS).unwrap_or(&Value::Null);
    let Some(items) = value.as_array() else {
        return if is_content(value) {
            parse::<Content>(value, "contents").map(Contents::from)
        } else {
            part_from(value).map(Contents::from)
        };
    };
    let mut turns: Vec<Content> = Vec::new();
    let mut pending: Vec<Part> = Vec::new();
    for item in items {
        if is_content(item) {
            if !pending.is_empty() {
                turns.push(Content::from(std::mem::take(&mut pending)));
            }
            turns.push(parse(item, "content")?);
        } else {
            pending.push(part_from(item)?);
        }
    }
    if !pending.is_empty() {
        turns.push(Content::from(pending));
    }
    Ok(Contents::from(turns))
}

/// Mirrors Python's `t_batch_job_source` for the shapes the corpus uses.
fn batch_source_from(params: &Value) -> Result<BatchJobSource, Halt> {
    let value = params.get(SRC).unwrap_or(&Value::Null);
    match value {
        Value::Array(_) => Ok(BatchJobSource {
            inlined_requests: Some(parse(value, SRC)?),
            ..Default::default()
        }),
        Value::String(uri) if uri.starts_with(GCS_PREFIX) => Ok(BatchJobSource {
            format: Some(FORMAT_JSONL.to_owned()),
            gcs_uri: Some(vec![uri.clone()]),
            ..Default::default()
        }),
        Value::String(uri) if uri.starts_with(BIGQUERY_PREFIX) => Ok(BatchJobSource {
            format: Some(FORMAT_BIGQUERY.to_owned()),
            bigquery_uri: Some(uri.clone()),
            ..Default::default()
        }),
        Value::String(uri) if uri.starts_with(FILES_PREFIX) => Ok(BatchJobSource {
            file_name: Some(uri.clone()),
            ..Default::default()
        }),
        Value::String(uri) => Err(Halt::Failed(format!("Unsupported source: {uri}"))),
        _ => parse(value, SRC),
    }
}

/// Python's `generate_videos` takes either `source` or the deprecated
/// `prompt`/`image`/`video`; both end up as one `GenerateVideosSource`.
fn video_source_from(params: &Value) -> Result<GenerateVideosSource, Halt> {
    let source: Option<GenerateVideosSource> = field(params, SOURCE)?;
    match source {
        Some(source) => Ok(source),
        None => Ok(GenerateVideosSource {
            prompt: field(params, PROMPT)?,
            image: field(params, IMAGE)?,
            video: field(params, VIDEO)?,
        }),
    }
}

/// Runs a `auth_tokens` corpus case; `Harness` if `test_method` is not one of its methods.
async fn run_auth_tokens(client: &Client, test_method: &str, p: &Value) -> Run {
    match test_method {
        AUTH_TOKENS_CREATE => {
            client.auth_tokens().create(field(p, CONFIG)?).await?;
        }
        other => return Err(Halt::Harness(format!("no dispatch arm for {other}"))),
    }
    Ok(())
}

/// Runs a `batches` corpus case; `Harness` if `test_method` is not one of its methods.
async fn run_batches(client: &Client, test_method: &str, p: &Value) -> Run {
    match test_method {
        BATCHES_CANCEL => {
            let name: String = field(p, NAME)?;
            client.batches().cancel(&name, field(p, CONFIG)?).await?;
        }
        BATCHES_CREATE => {
            let model: String = field(p, MODEL)?;
            let source = batch_source_from(p)?;
            client
                .batches()
                .create(&model, source, field(p, CONFIG)?)
                .await?;
        }
        BATCHES_CREATE_EMBEDDINGS => {
            let model: String = field(p, MODEL)?;
            let source: EmbeddingsBatchJobSource = field(p, SRC)?;
            client
                .batches()
                .create_embeddings(&model, source, field(p, CONFIG)?)
                .await?;
        }
        BATCHES_DELETE => {
            let name: String = field(p, NAME)?;
            client.batches().delete(&name, field(p, CONFIG)?).await?;
        }
        BATCHES_GET => {
            let name: String = field(p, NAME)?;
            client.batches().get(&name, field(p, CONFIG)?).await?;
        }
        BATCHES_LIST => {
            client.batches().list(field(p, CONFIG)?).await?;
        }
        other => return Err(Halt::Harness(format!("no dispatch arm for {other}"))),
    }
    Ok(())
}

/// Runs a `caches` corpus case; `Harness` if `test_method` is not one of its methods.
async fn run_caches(client: &Client, test_method: &str, p: &Value) -> Run {
    match test_method {
        CACHES_CREATE => {
            let model: String = field(p, MODEL)?;
            client.caches().create(&model, field(p, CONFIG)?).await?;
        }
        CACHES_DELETE => {
            let name: String = field(p, NAME)?;
            client.caches().delete(&name, field(p, CONFIG)?).await?;
        }
        CACHES_GET => {
            let name: String = field(p, NAME)?;
            client.caches().get(&name, field(p, CONFIG)?).await?;
        }
        CACHES_LIST => {
            client.caches().list(field(p, CONFIG)?).await?;
        }
        CACHES_UPDATE => {
            let name: String = field(p, NAME)?;
            client.caches().update(&name, field(p, CONFIG)?).await?;
        }
        other => return Err(Halt::Harness(format!("no dispatch arm for {other}"))),
    }
    Ok(())
}

/// Runs a `documents` corpus case; `Harness` if `test_method` is not one of its methods.
async fn run_documents(client: &Client, test_method: &str, p: &Value) -> Run {
    match test_method {
        DOCUMENTS_DELETE => {
            let name: String = field(p, NAME)?;
            client
                .file_search_stores()
                .documents()
                .delete(&name, field(p, CONFIG)?)
                .await?;
        }
        DOCUMENTS_GET => {
            let name: String = field(p, NAME)?;
            client
                .file_search_stores()
                .documents()
                .get(&name, field(p, CONFIG)?)
                .await?;
        }
        DOCUMENTS_LIST => {
            let parent: String = field(p, PARENT)?;
            client
                .file_search_stores()
                .documents()
                .list(&parent, field(p, CONFIG)?)
                .await?;
        }
        other => return Err(Halt::Harness(format!("no dispatch arm for {other}"))),
    }
    Ok(())
}

/// Runs a `file_search_stores` corpus case; `Harness` if `test_method` is not one of its methods.
async fn run_file_search_stores(client: &Client, test_method: &str, p: &Value) -> Run {
    match test_method {
        FILE_SEARCH_STORES_CREATE => {
            client
                .file_search_stores()
                .create(field(p, CONFIG)?)
                .await?;
        }
        FILE_SEARCH_STORES_DELETE => {
            let name: String = field(p, NAME)?;
            client
                .file_search_stores()
                .delete(&name, field(p, CONFIG)?)
                .await?;
        }
        FILE_SEARCH_STORES_GET => {
            let name: String = field(p, NAME)?;
            client
                .file_search_stores()
                .get(&name, field(p, CONFIG)?)
                .await?;
        }
        FILE_SEARCH_STORES_LIST => {
            client.file_search_stores().list(field(p, CONFIG)?).await?;
        }
        other => return Err(Halt::Harness(format!("no dispatch arm for {other}"))),
    }
    Ok(())
}

/// Runs a `files` corpus case; `Harness` if `test_method` is not one of its methods.
async fn run_files(client: &Client, test_method: &str, p: &Value) -> Run {
    match test_method {
        FILES_REGISTER_FILES => {
            client
                .files()
                .register_files(field(p, URIS)?, field(p, CONFIG)?)
                .await?;
        }
        FILES_DELETE => {
            let name: String = field(p, NAME)?;
            client.files().delete(&name, field(p, CONFIG)?).await?;
        }
        FILES_GET => {
            let name: String = field(p, NAME)?;
            client.files().get(&name, field(p, CONFIG)?).await?;
        }
        FILES_LIST => {
            client.files().list(field(p, CONFIG)?).await?;
        }
        other => return Err(Halt::Harness(format!("no dispatch arm for {other}"))),
    }
    Ok(())
}

/// Runs a `models` corpus case; `Harness` if `test_method` is not one of its methods.
#[expect(
    deprecated,
    reason = "upstream still tests generate_images, so the corpus must exercise the deprecated method"
)]
async fn run_models(client: &Client, test_method: &str, p: &Value) -> Run {
    match test_method {
        MODELS_COMPUTE_TOKENS => {
            let model: String = field(p, MODEL)?;
            let contents = contents_from(p)?;
            client
                .models()
                .compute_tokens(&model, contents, field(p, CONFIG)?)
                .await?;
        }
        MODELS_COUNT_TOKENS => {
            let model: String = field(p, MODEL)?;
            let contents = contents_from(p)?;
            client
                .models()
                .count_tokens(&model, contents, field(p, CONFIG)?)
                .await?;
        }
        MODELS_DELETE => {
            let model: String = field(p, MODEL)?;
            client.models().delete(&model, field(p, CONFIG)?).await?;
        }
        MODELS_EMBED_CONTENT => {
            let model: String = field(p, MODEL)?;
            let contents = contents_from(p)?;
            client
                .models()
                .embed_content(&model, contents, field(p, CONFIG)?)
                .await?;
        }
        MODELS_GENERATE_CONTENT => {
            let model: String = field(p, MODEL)?;
            let contents = contents_from(p)?;
            client
                .models()
                .generate_content(&model, contents, field(p, CONFIG)?)
                .await?;
        }
        MODELS_GENERATE_IMAGES => {
            let model: String = field(p, MODEL)?;
            let prompt: String = field(p, PROMPT)?;
            client
                .models()
                .generate_images(&model, &prompt, field(p, CONFIG)?)
                .await?;
        }
        MODELS_GENERATE_VIDEOS => {
            let model: String = field(p, MODEL)?;
            let source = video_source_from(p)?;
            client
                .models()
                .generate_videos(&model, source, field(p, CONFIG)?)
                .await?;
        }
        MODELS_GET => {
            let model: String = field(p, MODEL)?;
            client.models().get(&model, field(p, CONFIG)?).await?;
        }
        MODELS_LIST => {
            client.models().list(field(p, CONFIG)?).await?;
        }
        other => return Err(Halt::Harness(format!("no dispatch arm for {other}"))),
    }
    Ok(())
}

/// Runs a `tunings` corpus case; `Harness` if `test_method` is not one of its methods.
async fn run_tunings(client: &Client, test_method: &str, p: &Value) -> Run {
    match test_method {
        TUNINGS_GET => {
            let name: String = field(p, NAME)?;
            client.tunings().get(&name, field(p, CONFIG)?).await?;
        }
        TUNINGS_LIST => {
            client.tunings().list(field(p, CONFIG)?).await?;
        }
        TUNINGS_TUNE => {
            let base_model: String = field(p, BASE_MODEL)?;
            let dataset: TuningDataset = field(p, TRAINING_DATASET)?;
            client
                .tunings()
                .tune(&base_model, dataset, field(p, CONFIG)?)
                .await?;
        }
        other => return Err(Halt::Harness(format!("no dispatch arm for {other}"))),
    }
    Ok(())
}

async fn run(client: &Client, test_method: &str, params: &Value) -> Run {
    let mut normalized = params.clone();
    lift_unions(&mut normalized);
    drop_base_urls(&mut normalized);
    if test_method == BATCHES_CREATE
        && let Some(config) = normalized.get_mut(CONFIG)
    {
        lift_batch_destination(config)?;
    }
    let p = &normalized;
    // `file_search_stores.documents.*` must be tried before `file_search_stores.*`.
    if test_method.starts_with("auth_tokens.") {
        run_auth_tokens(client, test_method, p).await
    } else if test_method.starts_with("batches.") {
        run_batches(client, test_method, p).await
    } else if test_method.starts_with("caches.") {
        run_caches(client, test_method, p).await
    } else if test_method.starts_with("file_search_stores.documents.") {
        run_documents(client, test_method, p).await
    } else if test_method.starts_with("file_search_stores.") {
        run_file_search_stores(client, test_method, p).await
    } else if test_method.starts_with("files.") {
        run_files(client, test_method, p).await
    } else if test_method.starts_with("models.") {
        run_models(client, test_method, p).await
    } else if test_method.starts_with("tunings.") {
        Box::pin(run_tunings(client, test_method, p)).await
    } else {
        Err(Halt::Harness(format!("no dispatch arm for {test_method}")))
    }
}

/// Calls `test_method` on `client` with the JSON `params` of one corpus case.
pub async fn call(client: &Client, test_method: &str, params: Value) -> Outcome {
    match Box::pin(run(client, test_method, &params)).await {
        Ok(()) => Outcome::Succeeded,
        Err(Halt::Failed(message)) => Outcome::Failed(message),
        Err(Halt::Harness(message)) => Outcome::Harness(message),
    }
}
