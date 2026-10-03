//! Hand-written `t_*` transformers used by the generated converters
//! (`crate::converters::generated`), mirroring Python's `_transformers.py`
//! (the `_base_transformers.py` ones live in [`crate::base_transformers`]).
//!
//! **Gemini Developer API only**: the Python originals branch on
//! `client.vertexai`; only the non-Vertex (`mldev`) branch is ported here
//! (see `research.md` R-02/R-05). Several transformers are simpler than
//! their Python counterparts because the coercion Python does dynamically
//! at runtime (`str` → `Part`, a Python class → JSON Schema, raw `bytes` →
//! base64) is instead done by this crate's Rust type system and `serde`
//! *before* a value ever reaches these functions (see `types::conversions`
//! and the `#[serde_as(as = "Option<Base64>")]` fields in
//! `types::generated`): by the time a `Value` arrives here it is already
//! shaped correctly, so most of these are validation/normalization, not
//! full coercion.

#![expect(
    clippy::needless_pass_by_value,
    clippy::missing_errors_doc,
    reason = "every `t_*` transformer intentionally shares Python's uniform `fn(Value) -> Result<Value>` shape, even where a given transformer currently has no failure path or doesn't need ownership of its argument; the older ones describe their failures in prose rather than an `# Errors` section"
)]

use reqwest::Method;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};

use crate::{
    api_client::HttpClient,
    errors::{Error, Result},
};

/// Prepends a resource-name collection prefix if missing and doing so
/// would not violate the given collection hierarchy depth. Mirrors the
/// `mldev` (non-Vertex) branch of `_resource_name`.
#[must_use]
pub fn resource_name(name: &str, collection: &str, hierarchy_depth: usize) -> String {
    let collection_prefix = format!("{collection}/");
    let prefixed = format!("{collection_prefix}{name}");
    let should_prepend = !name.starts_with(&collection_prefix)
        && prefixed.matches('/').count() + 1 == hierarchy_depth;
    if should_prepend {
        prefixed
    } else {
        name.to_owned()
    }
}

/// Normalizes a model resource name (mirrors the `mldev` branch of
/// Python's `t_model`).
pub fn t_model(value: Value) -> Result<Value> {
    let model = as_str(&value, "model")?;
    if model.is_empty() {
        return Err(Error::Validation("model is required".to_owned()));
    }
    if model.contains("..") || model.contains('?') || model.contains('&') {
        return Err(Error::Validation("invalid model parameter".to_owned()));
    }
    if model.starts_with("models/") || model.starts_with("tunedModels/") {
        return Ok(value);
    }
    Ok(Value::String(format!("models/{model}")))
}

/// `"models"` or `"tunedModels"` depending on whether the caller wants
/// base (non-tuned) models. Mirrors the `mldev` branch of `t_models_url`.
pub fn t_models_url(base_models: Value) -> Result<Value> {
    let base_models = base_models.as_bool().unwrap_or(false);
    Ok(Value::String(if base_models {
        "models".to_owned()
    } else {
        "tunedModels".to_owned()
    }))
}

/// Extracts the model list from a `models.list` response, trying
/// `models`, then `tunedModels`, then `publisherModels`. Mirrors
/// `t_extract_models`.
pub fn t_extract_models(value: Value) -> Result<Value> {
    let Some(obj) = value.as_object() else {
        return Ok(Value::Array(vec![]));
    };
    for key in ["models", "tunedModels", "publisherModels"] {
        if let Some(list) = obj.get(key) {
            return Ok(list.clone());
        }
    }
    Ok(Value::Array(vec![]))
}

/// mldev has no project/location prefixing for cache model names, so this
/// is exactly [`t_model`] (mirrors the `mldev` branch of `t_caches_model`).
pub fn t_caches_model(value: Value) -> Result<Value> {
    t_model(value)
}

/// Rejects a falsy (Python `not x`) value; used by the "is required"
/// checks of the `t_function_response*` / `t_blob` transformers.
fn is_falsy(value: &Value) -> bool {
    !is_truthy(value)
}

/// Short Python-style name of a JSON value's kind, used in the
/// "unsupported type" messages (Python prints `type(x)`).
fn kind_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Returns `true` if `value` is a JSON object that validates as `T` the way
/// a Python `pydantic` model with `extra='forbid'` would: it must
/// deserialize into `T`, and every non-null top-level key (either
/// `snake_case` or the camelCase wire alias) must be a field of `T`.
/// Only the top level is checked for unknown keys; nested typed fields are
/// checked by `serde` alone.
fn conforms_to<T: Serialize + DeserializeOwned>(value: &Value) -> bool {
    let Some(input) = value.as_object() else {
        return false;
    };
    serde_json::from_value::<T>(value.clone())
        .ok()
        .and_then(|typed| serde_json::to_value(typed).ok())
        .and_then(|out| match out {
            Value::Object(fields) => Some(fields),
            _ => None,
        })
        .is_some_and(|fields| {
            input
                .iter()
                .all(|(key, v)| v.is_null() || fields.contains_key(&camel_to_snake(key)))
        })
}

/// `inlineData` -> `inline_data`; already-snake keys are unchanged.
fn camel_to_snake(key: &str) -> String {
    key.chars()
        .fold(String::with_capacity(key.len()), |mut out, c| {
            if c.is_ascii_uppercase() {
                out.push('_');
                out.push(c.to_ascii_lowercase());
            } else {
                out.push(c);
            }
            out
        })
}

/// `inlined_requests` -> `inlinedRequests`.
fn snake_to_camel(key: &str) -> String {
    key.split('_')
        .enumerate()
        .map(|(i, word)| {
            if i == 0 {
                word.to_owned()
            } else {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                    .unwrap_or_default()
            }
        })
        .collect()
}

/// The value of `snake` (or its camelCase alias) in `obj`, if present and
/// not `null`.
fn field<'a>(obj: &'a Map<String, Value>, snake: &str) -> Option<&'a Value> {
    obj.get(snake)
        .or_else(|| obj.get(&snake_to_camel(snake)))
        .filter(|v| !v.is_null())
}

/// Keys that only a `File` resource has; `File` is told apart from a `Part`
/// or `FileData` object by any of these being present (Python tells them
/// apart by class).
const FILE_ONLY_KEYS: [&str; 11] = [
    "name",
    "size_bytes",
    "sizeBytes",
    "create_time",
    "expiration_time",
    "update_time",
    "sha256_hash",
    "uri",
    "download_uri",
    "state",
    "source",
];

/// Validates a `FunctionResponse`-shaped value. Mirrors
/// `t_function_response`: an empty value is an error, an object must
/// validate as a `FunctionResponse`, and anything else is a type error.
///
/// # Errors
///
/// [`Error::Validation`] if `value` is empty, is not an object, or does not
/// validate as a `FunctionResponse`.
pub fn t_function_response(value: Value) -> Result<Value> {
    if is_falsy(&value) {
        return Err(Error::Validation(
            "function_response is required.".to_owned(),
        ));
    }
    if !value.is_object() {
        return Err(Error::Validation(format!(
            "Could not parse input as FunctionResponse. Unsupported function_response type: {}",
            kind_name(&value)
        )));
    }
    if conforms_to::<crate::types::FunctionResponse>(&value) {
        Ok(value)
    } else {
        Err(Error::Validation(format!(
            "Could not parse input as FunctionResponse: {value}"
        )))
    }
}

/// Validates one `FunctionResponse` or a list of them and returns the list.
/// Mirrors `t_function_responses`.
///
/// # Errors
///
/// [`Error::Validation`] if `value` is empty or any element is not a valid
/// `FunctionResponse` (see [`t_function_response`]).
pub fn t_function_responses(value: Value) -> Result<Value> {
    if is_falsy(&value) {
        return Err(Error::Validation(
            "function_responses are required.".to_owned(),
        ));
    }
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(t_function_response)
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        single => t_function_response(single).map(|one| Value::Array(vec![one])),
    }
}

/// Validates every blob of a list (or one blob) and returns the list.
/// Mirrors `t_blobs`.
///
/// # Errors
///
/// [`Error::Validation`] if any element is not a valid `Blob` (see
/// [`t_blob`]).
pub fn t_blobs(value: Value) -> Result<Value> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(t_blob)
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        other => t_blob(other).map(|one| Value::Array(vec![one])),
    }
}

/// Validates a `Blob`-shaped value. Mirrors `t_blob`; Python's
/// `PIL.Image` branch has no counterpart here (see
/// `tools/codegen/deviations.toml`).
///
/// # Errors
///
/// [`Error::Validation`] if `value` is empty, is not an object, or does not
/// validate as a `Blob`.
pub fn t_blob(value: Value) -> Result<Value> {
    if is_falsy(&value) {
        return Err(Error::Validation("blob is required.".to_owned()));
    }
    if !value.is_object() {
        return Err(Error::Validation(format!(
            "Could not parse input as Blob. Unsupported blob type: {}",
            kind_name(&value)
        )));
    }
    if conforms_to::<crate::types::Blob>(&value) {
        Ok(value)
    } else {
        Err(Error::Validation(format!(
            "Could not parse input as Blob: {value}"
        )))
    }
}

/// Validates a Blob's `mimeType` starts with `image/`. Mirrors
/// `t_image_blob`.
///
/// # Errors
///
/// [`Error::Validation`] if `value` is not a valid `Blob` or its MIME type
/// is not `image/*`.
pub fn t_image_blob(value: Value) -> Result<Value> {
    check_mime_prefix(t_blob(value)?, "image/")
}

/// Validates a Blob's `mimeType` starts with `audio/`. Mirrors
/// `t_audio_blob`.
///
/// # Errors
///
/// [`Error::Validation`] if `value` is not a valid `Blob` or its MIME type
/// is not `audio/*`.
pub fn t_audio_blob(value: Value) -> Result<Value> {
    check_mime_prefix(t_blob(value)?, "audio/")
}

/// Coerces a part-like value to a `Part`: a string becomes a text part, a
/// `File` becomes a `file_data` part, an object must validate as a `Part`
/// (or, failing that, as a `FileData`, which is then wrapped). Mirrors
/// `t_part`; Python's `PIL.Image` branch has no counterpart here.
///
/// # Errors
///
/// [`Error::Validation`] if `value` is `null`, a `File` lacking `uri` or
/// `mime_type`, an object that is neither a `Part` nor a `FileData`, or a
/// value of any other type.
pub fn t_part(value: Value) -> Result<Value> {
    match value {
        Value::Null => Err(Error::Validation("content part is required.".to_owned())),
        Value::String(text) => Ok(json!({ "text": text })),
        Value::Object(ref obj) if FILE_ONLY_KEYS.iter().any(|k| obj.contains_key(*k)) => {
            let uri = field(obj, "uri")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty());
            let mime_type = field(obj, "mime_type")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty());
            match (uri, mime_type) {
                (Some(uri), Some(mime_type)) => Ok(json!({
                    "file_data": { "file_uri": uri, "mime_type": mime_type }
                })),
                _ => Err(Error::Validation(
                    "file uri and mime_type are required.".to_owned(),
                )),
            }
        }
        object @ Value::Object(_) => {
            if conforms_to::<crate::types::Part>(&object) {
                Ok(object)
            } else if conforms_to::<crate::types::FileData>(&object) {
                Ok(json!({ "file_data": object }))
            } else {
                Err(Error::Validation(format!(
                    "Could not validate input as a Part or FileData: {object}"
                )))
            }
        }
        other => Err(Error::Validation(format!(
            "Unsupported content part type: <class '{}'>",
            kind_name(&other)
        ))),
    }
}

/// Coerces a part-like value, or a list of them, to a list of `Part`s.
/// Mirrors `t_parts`.
///
/// # Errors
///
/// [`Error::Validation`] if `value` is `null` or an empty list, or any
/// element fails [`t_part`].
pub fn t_parts(value: Value) -> Result<Value> {
    match value {
        Value::Null => Err(Error::Validation("content parts are required.".to_owned())),
        Value::Array(items) if items.is_empty() => {
            Err(Error::Validation("content parts are required.".to_owned()))
        }
        Value::Array(items) => items
            .into_iter()
            .map(t_part)
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        single => t_part(single).map(|one| Value::Array(vec![one])),
    }
}

/// Maps wire predictions (`{"image": {"gcsUri", "imageBytes"}}`) to
/// `GeneratedImage` values, skipping predictions without an `image`.
/// Returns `null` for an empty or absent list. Mirrors
/// `t_image_predictions`.
///
/// # Errors
///
/// [`Error::Validation`] if an `image` lacks `gcsUri` or `imageBytes`
/// (Python raises `KeyError` there).
pub fn t_image_predictions(value: Value) -> Result<Value> {
    let Value::Array(predictions) = value else {
        return Ok(Value::Null);
    };
    if predictions.is_empty() {
        return Ok(Value::Null);
    }
    predictions
        .iter()
        .filter_map(|prediction| prediction.get("image").filter(|image| is_truthy(image)))
        .map(|image| {
            let get = |key: &str| {
                image.get(key).cloned().ok_or_else(|| {
                    Error::Validation(format!("prediction image is missing `{key}`"))
                })
            };
            Ok(json!({
                "image": { "gcs_uri": get("gcsUri")?, "image_bytes": get("imageBytes")? }
            }))
        })
        .collect::<Result<Vec<_>>>()
        .map(Value::Array)
}

/// `true` if the (already validated) part carries a function call, which
/// makes it a model part rather than a user part.
fn is_function_call_part(part: &Value) -> bool {
    part.as_object()
        .and_then(|obj| field(obj, "function_call"))
        .is_some_and(is_truthy)
}

fn content_of(role: &str, parts: Vec<Value>) -> Value {
    json!({ "parts": parts, "role": role })
}

/// Coerces a content-like value to a `Content`: a valid `Content` object is
/// returned as is; a string, `File`, `Part`-shaped object or list of parts
/// becomes a `user` content (a function-call part becomes a `model`
/// content). Mirrors `t_content`.
///
/// # Errors
///
/// [`Error::Validation`] if `value` is `null`, an object that is neither a
/// `Content` nor a part, or a value of an unsupported type.
pub fn t_content(value: Value) -> Result<Value> {
    match value {
        Value::Null => Err(Error::Validation("content is required.".to_owned())),
        Value::Object(ref obj) if FILE_ONLY_KEYS.iter().any(|k| obj.contains_key(*k)) => {
            t_part(value).map(|part| content_of("user", vec![part]))
        }
        object @ Value::Object(_) => {
            if conforms_to::<crate::types::Content>(&object) {
                Ok(object)
            } else {
                let part = t_part(object)?;
                let role = if is_function_call_part(&part) {
                    "model"
                } else {
                    "user"
                };
                Ok(content_of(role, vec![part]))
            }
        }
        Value::String(_) => t_part(value).map(|part| content_of("user", vec![part])),
        Value::Array(items) => items
            .into_iter()
            .map(t_part)
            .collect::<Result<Vec<_>>>()
            .map(|parts| content_of("user", parts)),
        other => Err(Error::Validation(format!(
            "Unsupported content part type: <class '{}'>",
            kind_name(&other)
        ))),
    }
}

/// Same shape as [`t_contents`] (the Python original's Vertex-only
/// text-extraction branch is skipped). Mirrors the `mldev` branch of
/// `t_contents_for_embed`.
///
/// Unlike [`t_contents`]'s other call sites (which are always followed by
/// a per-item `content_to_mldev` call at the call site -- see the module
/// doc), `_EmbedContentParametersPrivate_to_mldev` uses this transformer's
/// result directly (Python's real source is a bare
/// `[item for item in t.t_contents_for_embed(...)]`), relying on an
/// implicit final `by_alias=True` pydantic serialization this crate has
/// no equivalent step for. So this transformer applies
/// `crate::converters::generated::live_converters::content_to_mldev`
/// itself, camelizing each `Content`'s `Part`s (`inline_data` ->
/// `inlineData`, etc.).
pub fn t_contents_for_embed(value: Value) -> Result<Value> {
    let items = match value {
        Value::Array(items) => items,
        other => vec![other],
    };
    // Python's `_EmbedContentParametersPrivate_to_mldev` is a bare
    // `[item for item in t.t_contents_for_embed(...)]`: the `Content`
    // objects are passed through untouched, and `_common.convert_to_dict`
    // later dumps them *without* `by_alias`, so parts keep their
    // snake_case spelling (`inline_data`, `mime_type`) on the wire. An
    // earlier version of this function camelized here; that was a
    // divergence, not a fix -- verified against google-genai 2.19.0.
    Ok(Value::Array(items))
}

/// Coerces contents to a list of `Content`s. A single value becomes a
/// one-element list through [`t_content`]; in a list, `Content` objects and
/// nested lists stand alone, while each run of consecutive part-like items
/// (strings, `File`s, `Part`s) is grouped into one `user` content, or into
/// a `model` content if the part carries a function call (a user/model
/// switch starts a new content). Mirrors `t_contents`.
///
/// # Errors
///
/// [`Error::Validation`] if `value` is `null` or an empty list, an element
/// is neither a content nor a part, or a part/content is invalid.
pub fn t_contents(value: Value) -> Result<Value> {
    let items = match value {
        Value::Null => return Err(Error::Validation("contents are required.".to_owned())),
        Value::Array(items) if items.is_empty() => {
            return Err(Error::Validation("contents are required.".to_owned()));
        }
        Value::Array(items) => items,
        single => return t_content(single).map(|content| Value::Array(vec![content])),
    };

    let mut result: Vec<Value> = Vec::new();
    let mut accumulated: Vec<Value> = Vec::new();
    for item in items {
        match item {
            Value::Array(parts) => {
                flush_parts(&mut result, &mut accumulated);
                let parts = parts.into_iter().map(t_part).collect::<Result<Vec<_>>>()?;
                result.push(content_of("user", parts));
            }
            item if is_part_like(&item) => {
                let part = t_part(item)?;
                if is_user_part(&part) == are_user_parts(&accumulated) {
                    accumulated.push(part);
                } else {
                    flush_parts(&mut result, &mut accumulated);
                    accumulated.push(part);
                }
            }
            object @ Value::Object(_) => {
                flush_parts(&mut result, &mut accumulated);
                if conforms_to::<crate::types::Content>(&object) {
                    result.push(object);
                } else {
                    return Err(Error::Validation(format!(
                        "Could not validate input as a Content: {object}"
                    )));
                }
            }
            other => {
                return Err(Error::Validation(format!(
                    "Unsupported content type: <class '{}'>",
                    kind_name(&other)
                )));
            }
        }
    }
    flush_parts(&mut result, &mut accumulated);
    Ok(Value::Array(result))
}

/// Whether a `t_contents` list element is a part (as opposed to a content):
/// a string, a `File`, or a non-empty object validating as a `Part` or
/// `FileData` (an empty object counts as an empty `Content`).
fn is_part_like(item: &Value) -> bool {
    match item {
        Value::String(_) => true,
        Value::Object(obj) if obj.is_empty() => false,
        Value::Object(obj) => {
            FILE_ONLY_KEYS.iter().any(|k| obj.contains_key(*k))
                || conforms_to::<crate::types::Part>(item)
                || conforms_to::<crate::types::FileData>(item)
        }
        _ => false,
    }
}

fn is_user_part(part: &Value) -> bool {
    !is_function_call_part(part)
}

fn are_user_parts(parts: &[Value]) -> bool {
    parts.iter().all(is_user_part)
}

/// Moves the accumulated parts into `result` as one content (no-op when
/// empty).
fn flush_parts(result: &mut Vec<Value>, accumulated: &mut Vec<Value>) {
    if accumulated.is_empty() {
        return;
    }
    let role = if are_user_parts(accumulated) {
        "user"
    } else {
        "model"
    };
    result.push(content_of(role, std::mem::take(accumulated)));
}

/// Rewrites a JSON-Schema `{"type": "null"}` into `OpenAPI`'s
/// `nullable: true`, in place. A bare `type: "null"` is removed and
/// `nullable` set; for `anyOf`, the first `{"type": "null"}` member is
/// removed and `nullable` set, and if a single member is left it is merged
/// into the schema and `anyOf` dropped. Mirrors `handle_null_fields`.
pub fn handle_null_fields(schema: &mut Value) {
    let Some(obj) = schema.as_object_mut() else {
        return;
    };
    if obj.get("type").and_then(Value::as_str) == Some("null") {
        obj.insert("nullable".to_owned(), Value::Bool(true));
        obj.remove("type");
        return;
    }
    let has_null_member = obj
        .get("anyOf")
        .and_then(Value::as_array)
        .is_some_and(|members| {
            members
                .iter()
                .any(|m| m.get("type").and_then(Value::as_str) == Some("null"))
        });
    if !has_null_member {
        return;
    }
    obj.insert("nullable".to_owned(), Value::Bool(true));
    let null_member = json!({"type": "null"});
    let remaining = obj
        .get_mut("anyOf")
        .and_then(Value::as_array_mut)
        .map(|members| {
            // Python's `list.remove` only drops a member that is exactly
            // `{"type": "null"}`.
            if let Some(pos) = members.iter().position(|m| *m == null_member) {
                members.remove(pos);
            }
            members.clone()
        });
    if let Some(mut remaining) = remaining
        && remaining.len() == 1
        && let Value::Object(only) = remaining.remove(0)
    {
        obj.extend(only);
        obj.remove("anyOf");
    }
}

/// Always fails: a schema of the given type cannot be converted. Mirrors
/// `_raise_for_unsupported_schema_type`.
///
/// # Errors
///
/// Always returns [`Error::Validation`].
pub fn raise_for_unsupported_schema_type(origin: &Value) -> Result<()> {
    Err(Error::Validation(format!(
        "Unsupported schema type: {origin}"
    )))
}

/// Errors if the schema sets a truthy `additionalProperties` (or its
/// `additional_properties` spelling), which the Gemini Developer API does
/// not support. Checks this one level only; [`process_schema`] calls it at
/// every level. Python only raises when `not client.vertexai`; this crate
/// targets the Gemini Developer API exclusively, so the check always
/// applies. Mirrors `_raise_for_unsupported_mldev_properties`.
///
/// # Errors
///
/// [`Error::Validation`] if `additionalProperties` is truthy.
pub fn raise_for_unsupported_mldev_properties(schema: &Value) -> Result<()> {
    let additional = ["additionalProperties", "additional_properties"]
        .iter()
        .find_map(|key| schema.get(*key).filter(|v| is_truthy(v)));
    if additional.is_some() {
        return Err(Error::Validation(
            "additionalProperties is only supported in Gemini Enterprise Agent Platform \
             mode, not in Gemini Developer API mode."
                .to_owned(),
        ));
    }
    Ok(())
}

/// Renames a schema's `snake_case` keys to their `camelCase` spelling.
const SCHEMA_KEY_RENAMES: [(&str, &str); 4] = [
    ("additional_properties", "additionalProperties"),
    ("any_of", "anyOf"),
    ("prefix_items", "prefixItems"),
    ("property_ordering", "propertyOrdering"),
];

/// Updates the schema and each sub-schema in place to be API-compatible:
/// inlines `$defs`/`$ref` (a reference back into itself becomes `{}`),
/// standardises key spelling, rewrites null types (see
/// [`handle_null_fields`]), turns a string `const` into a single-value
/// `enum`, and, when `order_properties` is set, fills `property_ordering`
/// from the key order of an object schema's `properties`. Mirrors
/// `process_schema` (the `client` argument is dropped: this crate is
/// Developer-API-only).
///
/// # Errors
///
/// [`Error::Validation`] for a truthy `additionalProperties`, a non-string
/// `const`, or an unresolvable `$ref`.
pub fn process_schema(
    schema: &mut Value,
    defs: Option<&Map<String, Value>>,
    order_properties: bool,
) -> Result<()> {
    process_schema_visiting(schema, defs, order_properties, &mut Vec::new())
}

/// `visiting` holds the names of the `$defs` entries currently being
/// expanded (Python tracks `id()`s of the dicts on the path), so a
/// recursive reference terminates.
fn process_schema_visiting(
    schema: &mut Value,
    defs: Option<&Map<String, Value>>,
    order_properties: bool,
    visiting: &mut Vec<String>,
) -> Result<()> {
    if !schema.is_object() {
        return Ok(());
    }
    if schema.get("title").and_then(Value::as_str) == Some("PlaceholderLiteralEnum")
        && let Some(obj) = schema.as_object_mut()
    {
        obj.remove("title");
    }

    raise_for_unsupported_mldev_properties(schema)?;

    if let Some(obj) = schema.as_object_mut() {
        for (from_name, to_name) in SCHEMA_KEY_RENAMES {
            if let Some(value) = obj.remove(from_name).filter(|v| !v.is_null()) {
                obj.insert(to_name.to_owned(), value);
            }
        }
    }

    let owned_defs;
    let defs = if let Some(defs) = defs {
        defs
    } else {
        owned_defs = process_defs(schema, order_properties, visiting)?;
        &owned_defs
    };

    handle_null_fields(schema);

    // After removing null fields, an Optional field with one possible type
    // keeps a `$ref` that has to be flattened.
    if let Some(obj) = schema.as_object_mut()
        && let Some(reference) = obj.remove("$ref")
        && let Value::Object(target) = resolve_ref(&reference, defs)?.clone()
    {
        obj.extend(target);
    }

    if schema.get("anyOf").is_some_and(Value::is_array)
        && let Some(Value::Array(members)) = schema.get_mut("anyOf").map(std::mem::take)
    {
        let processed = members
            .into_iter()
            .map(|member| process_sub_schema(member, defs, order_properties, visiting))
            .collect::<Result<Vec<_>>>()?;
        if let Some(obj) = schema.as_object_mut() {
            obj.insert("anyOf".to_owned(), Value::Array(processed));
        }
        return Ok(());
    }

    let schema_type = schema
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_uppercase);

    // pydantic emits `const` for a one-value `Literal`.
    let constant = schema.get("const").filter(|v| !v.is_null()).cloned();
    if let Some(constant) = constant {
        if schema_type.as_deref() == Some("STRING") {
            if let Some(obj) = schema.as_object_mut() {
                obj.insert("enum".to_owned(), Value::Array(vec![constant]));
                obj.remove("const");
            }
        } else {
            return Err(Error::Validation(
                "Literal values must be strings.".to_owned(),
            ));
        }
    }

    match schema_type.as_deref() {
        Some("OBJECT") => process_object_schema(schema, defs, order_properties, visiting)?,
        Some("ARRAY") => process_array_schema(schema, defs, order_properties, visiting)?,
        _ => {}
    }
    Ok(())
}

/// Pops `$defs` from the top-level schema and processes each entry in
/// place, returning the processed map.
fn process_defs(
    schema: &mut Value,
    order_properties: bool,
    visiting: &mut Vec<String>,
) -> Result<Map<String, Value>> {
    let mut defs = match schema.as_object_mut().and_then(|obj| obj.remove("$defs")) {
        Some(Value::Object(defs)) => defs,
        _ => Map::new(),
    };
    let names: Vec<String> = defs.keys().cloned().collect();
    for name in names {
        // JSON Schema forbids a `$ref` that directly references another
        // `$ref`, so no `$ref` check is needed on the entry itself.
        let mut entry = defs.get(&name).cloned().unwrap_or(Value::Null);
        visiting.push(name.clone());
        let outcome = process_schema_visiting(&mut entry, Some(&defs), order_properties, visiting);
        visiting.pop();
        outcome?;
        defs.insert(name, entry);
    }
    Ok(defs)
}

fn resolve_ref<'a>(reference: &Value, defs: &'a Map<String, Value>) -> Result<&'a Value> {
    let reference = reference.as_str().unwrap_or_default();
    let name = reference.rsplit("defs/").next().unwrap_or(reference);
    defs.get(name)
        .ok_or_else(|| Error::Validation(format!("unresolved schema reference: {reference}")))
}

/// Processes a sub-schema, resolving its `$ref`; a reference back to a
/// definition that is already being expanded yields `{}`.
fn process_sub_schema(
    sub_schema: Value,
    defs: &Map<String, Value>,
    order_properties: bool,
    visiting: &mut Vec<String>,
) -> Result<Value> {
    let mut sub_schema = sub_schema;
    let reference = sub_schema
        .as_object_mut()
        .and_then(|obj| obj.remove("$ref"));
    let Some(reference) = reference else {
        process_schema_visiting(&mut sub_schema, Some(defs), order_properties, visiting)?;
        return Ok(sub_schema);
    };
    let name = reference
        .as_str()
        .map(|r| r.rsplit("defs/").next().unwrap_or(r).to_owned())
        .unwrap_or_default();
    if visiting.contains(&name) {
        return Ok(Value::Object(Map::new()));
    }
    let mut resolved = resolve_ref(&reference, defs)?.clone();
    visiting.push(name);
    let outcome = process_schema_visiting(&mut resolved, Some(defs), order_properties, visiting);
    visiting.pop();
    outcome?;
    Ok(resolved)
}

fn process_object_schema(
    schema: &mut Value,
    defs: &Map<String, Value>,
    order_properties: bool,
    visiting: &mut Vec<String>,
) -> Result<()> {
    let Some(obj) = schema.as_object_mut() else {
        return Ok(());
    };
    if let Some(Value::Object(properties)) = obj.get_mut("properties") {
        for property in properties.values_mut() {
            let taken = std::mem::take(property);
            *property = process_sub_schema(taken, defs, order_properties, visiting)?;
        }
    }
    let property_names: Option<Vec<Value>> = obj
        .get("properties")
        .and_then(Value::as_object)
        .filter(|properties| properties.len() > 1)
        .map(|properties| properties.keys().cloned().map(Value::String).collect());
    if let Some(names) = property_names
        && order_properties
        && !obj.contains_key("propertyOrdering")
    {
        obj.insert("property_ordering".to_owned(), Value::Array(names));
    }
    // `additionalProperties` may legally be a bool; only a sub-schema is
    // recursed into.
    if let Some(additional @ Value::Object(_)) = obj.get_mut("additionalProperties") {
        let taken = std::mem::take(additional);
        *additional = process_sub_schema(taken, defs, order_properties, visiting)?;
    }
    Ok(())
}

fn process_array_schema(
    schema: &mut Value,
    defs: &Map<String, Value>,
    order_properties: bool,
    visiting: &mut Vec<String>,
) -> Result<()> {
    let Some(obj) = schema.as_object_mut() else {
        return Ok(());
    };
    if let Some(items) = obj.get_mut("items").filter(|v| !v.is_null()) {
        let taken = std::mem::take(items);
        *items = process_sub_schema(taken, defs, order_properties, visiting)?;
    }
    if let Some(Value::Array(prefixes)) = obj.get_mut("prefixItems") {
        for prefix in prefixes.iter_mut() {
            let taken = std::mem::take(prefix);
            *prefix = process_sub_schema(taken, defs, order_properties, visiting)?;
        }
    }
    Ok(())
}

/// Builds a `Schema` for an enum from its `(name, value)` members: every
/// member value must be a string or an integer, and integer members are
/// carried as their decimal strings. Mirrors `_process_enum`; Python's
/// `Enum` class has no Rust counterpart, so the members are passed
/// explicitly and the pydantic placeholder-model round trip is replaced by
/// building the resulting `{"type": "STRING", "enum": [...]}` schema
/// directly.
///
/// # Errors
///
/// [`Error::Validation`] if a member value is neither a string nor an
/// integer.
pub fn process_enum(members: &[(String, Value)]) -> Result<Value> {
    let values = members
        .iter()
        .map(|(name, value)| match value {
            Value::String(text) => Ok(Value::String(text.clone())),
            Value::Number(n) if n.is_i64() || n.is_u64() => Ok(Value::String(n.to_string())),
            other => Err(Error::Validation(format!(
                "Enum member {name} value must be a string or integer, got {}",
                kind_name(other)
            ))),
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({ "type": "STRING", "enum": values }))
}

/// Renames an already-typed `Schema` value's own fields to their wire
/// (camelCase) spelling and rejects an
/// unsupported `additional_properties` (see
/// `check_schema_tree`), treating an absent schema as
/// `Null`. Python's Python-type/pydantic-model/enum coercion (`t_schema`
/// accepting a raw `dict`, a `pydantic.BaseModel` subclass, or an `Enum`
/// and deriving a JSON Schema from it via `model_json_schema()`) is not
/// applicable: the Rust API only accepts a [`crate::types::Schema`]
/// directly, or a JSON Schema built from a Rust type via `schemars` (see
/// `with_json_schema_of`/`response_json_schema`, handled by
/// [`t_json_schema`] instead, which needs none of this). For the same
/// reason, `process_schema`'s `$defs`/`$ref` inlining and its
/// `PlaceholderLiteralEnum` title-stripping and `const`-to-`enum`
/// rewriting -- all artifacts of normalizing a `pydantic`
/// `model_json_schema()` dump, which a hand-built [`crate::types::Schema`]
/// never produces -- are not applicable either, and `handle_null_fields`
/// (rewriting a JSON-Schema-style `{"type": "null"}` member of `anyOf`
/// into `nullable: true`) doesn't apply for the same reason: this crate's
/// [`crate::types::Type`] already has its own `Null` variant that
/// serializes directly to the wire `"NULL"` spelling the API expects, with
/// no `anyOf`/`nullable` rewrite needed.
///
/// One divergence from Python is deliberate, not a gap: Python's
/// `process_schema` auto-populates `property_ordering` from a `properties`
/// dict's insertion order when the caller left it unset (`schema['property_ordering']
/// = list(properties.keys())`), relying on Python 3.7+ dicts preserving
/// insertion order. This crate's [`crate::types::Schema::properties`] is a
/// `std::collections::HashMap`, which has no defined/stable iteration
/// order (randomized per-process), so mechanically porting that
/// auto-population would silently emit a `propertyOrdering` with no
/// relationship to any order the caller intended, and a different one on
/// every run -- worse than omitting it. Callers who need deterministic
/// property ordering (it affects generation quality/determinism for
/// structured output) must set [`crate::types::Schema::property_ordering`]
/// explicitly; it passes through unchanged (renamed to `propertyOrdering`)
/// when set.
pub fn t_schema(value: Value) -> Result<Value> {
    if value.is_null() {
        Ok(Value::Null)
    } else {
        check_schema_tree(&value)?;
        Ok(value)
    }
}

/// Wraps a bare voice-name string into a full `SpeechConfig` shape;
/// passes an already-object `SpeechConfig` through unchanged. Mirrors
/// `t_speech_config`.
///
/// No generated `_to_mldev` converter exists for `SpeechConfig` (unlike
/// `Content`/`Tool`, which get a recursive converter call *after* their
/// `t_*` transformer runs -- see the module doc), so this transformer
/// must itself produce wire-cased (camelCase) keys.
pub fn t_speech_config(value: Value) -> Result<Value> {
    match value {
        Value::Null => Ok(Value::Null),
        Value::String(voice_name) => Ok(serde_json::json!({
            "voice_config": { "prebuilt_voice_config": { "voice_name": voice_name } }
        })),
        object => Ok(object),
    }
}

/// Validates `multi_speaker_voice_config` is not set (unsupported by the
/// Live API), then camelizes the result (see [`t_speech_config`] doc for
/// why this transformer must do so itself). Mirrors `t_live_speech_config`.
pub fn t_live_speech_config(value: Value) -> Result<Value> {
    let has_multi_speaker = value
        .as_object()
        .and_then(|o| o.get("multi_speaker_voice_config"))
        .is_some_and(|v| !v.is_null());
    if has_multi_speaker {
        return Err(Error::Validation(
            "multi_speaker_voice_config is not supported in the live API".to_owned(),
        ));
    }
    Ok(value)
}

/// Passes an already-typed `Tool` value through. Mirrors the
/// dict/duck-typed-`Tool` branches of `t_tool`; Python's bare-callable and
/// MCP-tool coercion happen in `crate::extra_utils`/`crate::mcp` before a `Tool`
/// value ever reaches this converter layer.
pub fn t_tool(value: Value) -> Result<Value> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    Ok(value)
}

/// Passes an array of already-typed `Tool` values through. Python's
/// per-callable-tool merging (combining every function-only `Tool` into
/// one) is instead done once, in Rust, when building the `Tool` list (see
/// `crate::extra_utils`). Mirrors `t_tools`.
pub fn t_tools(value: Value) -> Result<Value> {
    match value {
        Value::Array(items) => Ok(Value::Array(items)),
        Value::Null => Ok(Value::Array(vec![])),
        other => Ok(Value::Array(vec![other])),
    }
}

/// Mirrors the `mldev` branch of `t_cached_content_name`
/// (`_resource_name(..., collection_identifier='cachedContents')`).
pub fn t_cached_content_name(value: Value) -> Result<Value> {
    let name = as_str(&value, "cached content name")?;
    Ok(Value::String(resource_name(name, "cachedContents", 2)))
}

/// Counts how many of `keys` are set (non-null) on `obj`.
fn count_set(obj: &Map<String, Value>, keys: &[&str]) -> usize {
    keys.iter().filter(|key| field(obj, key).is_some()).count()
}

const MLDEV_SOURCE_ERROR: &str = "Exactly one of `inlined_requests`, `file_name`, `inlined_embed_content_requests`, or `embed_content_file_name` must be set, other sources are not supported in Gemini API.";

/// Coerces a batch source: an object must set exactly one of
/// `inlined_requests` / `file_name` (and none of the Vertex-only sources), a
/// list becomes `inlined_requests`, and a string becomes `gcs_uri`
/// (`gs://`), `bigquery_uri` (`bq://`), `vertex_dataset_name`
/// (`projects/*/locations/*/datasets/*`) or `file_name` (`files/`). Mirrors
/// the `mldev` branch of `t_batch_job_source`.
///
/// # Errors
///
/// [`Error::Validation`] for an object with a wrong number of sources, or
/// an unsupported string or other value.
pub fn t_batch_job_source(value: Value) -> Result<Value> {
    match value {
        Value::Object(ref obj) => {
            let vertex_sources =
                count_set(obj, &["gcs_uri", "bigquery_uri", "vertex_dataset_name"]);
            let mldev_sources = count_set(obj, &["inlined_requests", "file_name"]);
            if vertex_sources > 0 || mldev_sources != 1 {
                Err(Error::Validation(MLDEV_SOURCE_ERROR.to_owned()))
            } else {
                Ok(value)
            }
        }
        Value::Array(_) => Ok(json!({ "inlined_requests": value })),
        Value::String(ref src) if src.starts_with("gs://") => {
            Ok(json!({ "format": "jsonl", "gcs_uri": [src] }))
        }
        Value::String(ref src) if src.starts_with("bq://") => {
            Ok(json!({ "format": "bigquery", "bigquery_uri": src }))
        }
        Value::String(ref src) if is_vertex_dataset_name(src) => {
            Ok(json!({ "format": "vertex-dataset", "vertex_dataset_name": src }))
        }
        Value::String(ref src) if src.starts_with("files/") => Ok(json!({ "file_name": src })),
        other => Err(Error::Validation(format!("Unsupported source: {other}"))),
    }
}

/// Matches `^projects/[^/]+/locations/[^/]+/datasets/[^/]+$`.
fn is_vertex_dataset_name(name: &str) -> bool {
    let parts: Vec<&str> = name.split('/').collect();
    matches!(
        parts.as_slice(),
        ["projects", project, "locations", location, "datasets", dataset]
            if !project.is_empty() && !location.is_empty() && !dataset.is_empty()
    )
}

/// Validates an embeddings batch source: exactly one of `inlined_requests` /
/// `file_name` must be set. Mirrors `t_embedding_batch_job_source`.
///
/// # Errors
///
/// [`Error::Validation`] if `value` is not an object or does not set
/// exactly one source.
pub fn t_embedding_batch_job_source(value: Value) -> Result<Value> {
    let Value::Object(ref obj) = value else {
        return Err(Error::Validation(format!(
            "Unsupported source type: <class '{}'>",
            kind_name(&value)
        )));
    };
    if count_set(obj, &["inlined_requests", "file_name"]) == 1 {
        Ok(value)
    } else {
        Err(Error::Validation(MLDEV_SOURCE_ERROR.to_owned()))
    }
}

/// Coerces a batch destination: an object passes through, `gs://...`
/// becomes a `jsonl` `gcs_uri` destination and `bq://...` a `bigquery`
/// destination. Mirrors `t_batch_job_destination`.
///
/// # Errors
///
/// [`Error::Validation`] for any other string or value.
pub fn t_batch_job_destination(value: Value) -> Result<Value> {
    match value {
        Value::Object(_) => Ok(value),
        Value::String(ref dest) if dest.starts_with("gs://") => {
            Ok(json!({ "format": "jsonl", "gcs_uri": dest }))
        }
        Value::String(ref dest) if dest.starts_with("bq://") => {
            Ok(json!({ "format": "bigquery", "bigquery_uri": dest }))
        }
        other => Err(Error::Validation(format!(
            "Unsupported destination: {other}"
        ))),
    }
}

/// Renames `inlinedResponses` to `inlinedEmbedContentResponses` if the
/// responses look like embedding results. Mirrors
/// `t_recv_batch_job_destination`.
pub fn t_recv_batch_job_destination(value: Value) -> Result<Value> {
    let Value::Object(mut dest) = value else {
        return Ok(value);
    };
    let looks_like_embedding = dest
        .get("inlinedResponses")
        .and_then(Value::as_object)
        .and_then(|o| o.get("inlinedResponses"))
        .and_then(Value::as_array)
        .is_some_and(|responses| {
            responses.iter().any(|r| {
                r.as_object()
                    .and_then(|o| o.get("response"))
                    .and_then(Value::as_object)
                    .is_some_and(|resp| resp.contains_key("embedding"))
            })
        });
    if looks_like_embedding && let Some(inlined) = dest.remove("inlinedResponses") {
        dest.insert("inlinedEmbedContentResponses".to_owned(), inlined);
    }
    Ok(Value::Object(dest))
}

/// Extracts the bare id from a `batches/{id}` resource name. Mirrors the
/// `mldev` branch of `t_batch_job_name`.
pub fn t_batch_job_name(value: Value) -> Result<Value> {
    let name = as_str(&value, "batch job name")?;
    match name
        .strip_prefix("batches/")
        .filter(|rest| !rest.is_empty() && !rest.contains('/'))
    {
        Some(id) => Ok(Value::String(id.to_owned())),
        None => Err(Error::Validation(format!(
            "Invalid batch job name: {name}."
        ))),
    }
}

/// Maps a `BATCH_STATE_*` wire value to the corresponding `JOB_STATE_*`
/// value, passing unrecognized values through unchanged. Mirrors
/// `t_job_state`.
pub fn t_job_state(value: Value) -> Result<Value> {
    let Some(state) = value.as_str() else {
        return Ok(value);
    };
    let mapped = match state {
        "BATCH_STATE_UNSPECIFIED" => "JOB_STATE_UNSPECIFIED",
        "BATCH_STATE_PENDING" => "JOB_STATE_PENDING",
        "BATCH_STATE_RUNNING" => "JOB_STATE_RUNNING",
        "BATCH_STATE_SUCCEEDED" => "JOB_STATE_SUCCEEDED",
        "BATCH_STATE_FAILED" => "JOB_STATE_FAILED",
        "BATCH_STATE_CANCELLED" => "JOB_STATE_CANCELLED",
        "BATCH_STATE_EXPIRED" => "JOB_STATE_EXPIRED",
        other => return Ok(Value::String(other.to_owned())),
    };
    Ok(Value::String(mapped.to_owned()))
}

/// Delay before the first long-running-operation poll, in seconds.
pub const LRO_POLLING_INITIAL_DELAY_SECONDS: f64 = 1.0;
/// Upper bound of the exponentially growing poll delay, in seconds.
pub const LRO_POLLING_MAXIMUM_DELAY_SECONDS: f64 = 20.0;
/// Total polling budget before giving up, in seconds.
pub const LRO_POLLING_TIMEOUT_SECONDS: f64 = 900.0;
/// Growth factor of the poll delay between polls.
pub const LRO_POLLING_MULTIPLIER: f64 = 1.5;

/// Polls a long-running operation (`name` containing `/operations/`) until
/// it is `done` and returns its `response`; any other value is returned
/// unchanged. Mirrors `t_resolve_operation`.
///
/// Python's loop accumulates `total_seconds += total_seconds` (which never
/// grows, so its timeout cannot trigger); this port adds the time actually
/// slept, so [`LRO_POLLING_TIMEOUT_SECONDS`] takes effect as documented.
///
/// # Errors
///
/// [`Error::Validation`] if polling exceeds the timeout or the finished
/// operation carries an `error`; transport errors from the poll request.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "no Rust converter resolves operations through this transformer yet; `operations` polls through `Operations::get`"
    )
)]
pub(crate) async fn t_resolve_operation(http: &HttpClient, value: Value) -> Result<Value> {
    let name = match value.get("name").and_then(Value::as_str) {
        Some(name) if name.contains("/operations/") => name.to_owned(),
        _ => return Ok(value),
    };
    let mut operation = value;
    let mut total_seconds = 0.0_f64;
    let mut delay_seconds = LRO_POLLING_INITIAL_DELAY_SECONDS;
    while operation.get("done") != Some(&Value::Bool(true)) {
        if total_seconds > LRO_POLLING_TIMEOUT_SECONDS {
            return Err(Error::Validation(format!(
                "Operation {name} timed out.\n{operation}"
            )));
        }
        let response = http.request(Method::GET, &name, None, None, None).await?;
        operation = serde_json::from_slice(&response.body)?;
        tokio::time::sleep(std::time::Duration::from_secs_f64(delay_seconds)).await;
        total_seconds += delay_seconds;
        delay_seconds =
            (delay_seconds * LRO_POLLING_MULTIPLIER).min(LRO_POLLING_MAXIMUM_DELAY_SECONDS);
    }
    match operation.get("error").filter(|error| is_truthy(error)) {
        Some(error) => Err(Error::Validation(format!(
            "Operation {name} failed with error: {error}.\n{operation}"
        ))),
        None => Ok(operation.get("response").cloned().unwrap_or(Value::Null)),
    }
}

/// Strips a `files/` prefix (or extracts the id from a `https://.../files/{id}`
/// URI). Mirrors `t_file_name`; the `File`/`Video`/`GeneratedVideo` object
/// coercion in Python is done by the Rust API's `FileRef`-style argument
/// types before this is called.
pub fn t_file_name(value: Value) -> Result<Value> {
    let name = as_str(&value, "file name")?;
    if name.is_empty() {
        return Err(Error::Validation("file name is required".to_owned()));
    }
    if let Some(after) = name.strip_prefix("https://") {
        let suffix = after
            .split_once("files/")
            .map(|(_, rest)| rest)
            .ok_or_else(|| {
                Error::Validation(format!("could not extract file name from URI: {name}"))
            })?;
        let id: String = suffix
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect();
        if id.is_empty() {
            return Err(Error::Validation(format!(
                "could not extract file name from URI: {name}"
            )));
        }
        return Ok(Value::String(id));
    }
    if let Some(rest) = name.strip_prefix("files/") {
        return Ok(Value::String(rest.to_owned()));
    }
    Ok(Value::String(name.to_owned()))
}

/// Maps a tuning-operation `status` string to the corresponding
/// `JobState` wire value, passing already-canonical or unrecognized
/// values through unchanged. Mirrors `t_tuning_job_status`.
pub fn t_tuning_job_status(value: Value) -> Result<Value> {
    let Some(status) = value.as_str() else {
        return Ok(value);
    };
    let mapped = match status {
        "STATE_UNSPECIFIED" => "JOB_STATE_UNSPECIFIED",
        "CREATING" => "JOB_STATE_RUNNING",
        "ACTIVE" => "JOB_STATE_SUCCEEDED",
        "FAILED" => "JOB_STATE_FAILED",
        other => return Ok(Value::String(other.to_owned())),
    };
    Ok(Value::String(mapped.to_owned()))
}

/// Validates a `Content` or a list of them (no part-to-content coercion,
/// unlike [`t_contents`]) and returns the list. Mirrors `t_contents_strict`
/// together with `t_content_strict`.
///
/// # Errors
///
/// [`Error::Validation`] if any element is not a valid `Content` object.
pub fn t_contents_strict(value: Value) -> Result<Value> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(t_content_strict)
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        single => t_content_strict(single).map(|one| Value::Array(vec![one])),
    }
}

/// Validates a single `Content` object. Mirrors `t_content_strict`.
///
/// # Errors
///
/// [`Error::Validation`] if `value` is not an object validating as a
/// `Content`.
pub fn t_content_strict(value: Value) -> Result<Value> {
    if conforms_to::<crate::types::Content>(&value) {
        Ok(value)
    } else {
        Err(Error::Validation(format!(
            "Could not convert input (type \"{}\") to `types.Content`",
            kind_name(&value)
        )))
    }
}

/// Builds a `LiveClientContent` from `turns` (`null` for none) and
/// `turn_complete`. Mirrors `t_client_content`.
///
/// # Errors
///
/// [`Error::Validation`] if `turns` is not valid content (see
/// [`t_contents_strict`]); the underlying cause is logged at debug level.
pub fn t_client_content(turns: Value, turn_complete: bool) -> Result<Value> {
    if turns.is_null() {
        return Ok(json!({ "turn_complete": turn_complete }));
    }
    let kind = kind_name(&turns);
    t_contents_strict(turns)
        .inspect_err(|cause| tracing::debug!(%cause, "t_client_content: invalid turns"))
        .map(|turns| json!({ "turns": turns, "turn_complete": turn_complete }))
        .map_err(|_| {
            Error::Validation(format!(
                "Could not convert input (type \"{kind}\") to `types.LiveClientContent`"
            ))
        })
}

/// Builds a `LiveClientToolResponse` from one `FunctionResponse` or a list.
/// Mirrors `t_tool_response`.
///
/// # Errors
///
/// [`Error::Validation`] if `input` is empty or not valid function
/// responses (see [`t_function_responses`]); the cause is logged at debug
/// level.
pub fn t_tool_response(input: Value) -> Result<Value> {
    if is_falsy(&input) {
        return Err(Error::Validation(format!(
            "A tool response is required, got: \n{input}"
        )));
    }
    let kind = kind_name(&input);
    t_function_responses(input)
        .inspect_err(|cause| tracing::debug!(%cause, "t_tool_response: invalid input"))
        .map(|responses| json!({ "function_responses": responses }))
        .map_err(|_| {
            Error::Validation(format!(
                "Could not convert input (type \"{kind}\") to `types.LiveClientToolResponse`"
            ))
        })
}

/// Aggregations requested for every evaluation metric.
const METRIC_AGGREGATIONS: [&str; 2] = ["AVERAGE", "STANDARD_DEVIATION"];

/// Builds the metric payload of an evaluation request: a `UnifiedMetric`
/// object is kept as is, other metrics are mapped by `name`
/// (`exact_match`, `bleu`, `rouge*`) or by a `prompt_template`
/// (pointwise); each gets the standard aggregations. Mirrors `t_metrics`
/// (an evaluation feature of the Vertex AI surface; ported for ledger
/// parity, with a `UnifiedMetric` passed through without Python's
/// `model_dump` explicit `null`s).
///
/// # Errors
///
/// [`Error::Validation`] if `metrics` is not a list, a metric has no
/// `name`, or its name/type is unsupported.
pub fn t_metrics(metrics: Value) -> Result<Value> {
    let Value::Array(metrics) = metrics else {
        return Err(Error::Validation("metrics must be a list".to_owned()));
    };
    metrics
        .into_iter()
        .map(metric_payload)
        .collect::<Result<Vec<_>>>()
        .map(Value::Array)
}

fn metric_payload(metric: Value) -> Result<Value> {
    let aggregations = json!(METRIC_AGGREGATIONS);
    if conforms_to::<crate::types::UnifiedMetric>(&metric) {
        let mut payload = metric;
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("aggregation_metrics".to_owned(), aggregations);
        }
        return Ok(payload);
    }
    let name = metric
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_lowercase)
        .ok_or_else(|| Error::Validation("metric `name` is required".to_owned()))?;
    let mut payload = Map::new();
    payload.insert("aggregation_metrics".to_owned(), aggregations);
    let prompt_template = metric
        .get("prompt_template")
        .filter(|template| is_truthy(template));
    if name == "exact_match" {
        payload.insert("exact_match_spec".to_owned(), json!({}));
    } else if name == "bleu" {
        payload.insert("bleu_spec".to_owned(), json!({}));
    } else if name.starts_with("rouge") {
        payload.insert(
            "rouge_spec".to_owned(),
            json!({ "rouge_type": name.replace('_', "") }),
        );
    } else if let Some(template) = prompt_template {
        let mut spec = Map::new();
        spec.insert("metric_prompt_template".to_owned(), template.clone());
        if let Some(instruction) = metric
            .get("judge_model_system_instruction")
            .filter(|v| is_truthy(v))
        {
            spec.insert("system_instruction".to_owned(), instruction.clone());
        }
        if let Some(raw) = metric.get("return_raw_output").filter(|v| is_truthy(v)) {
            spec.insert(
                "custom_output_format_config".to_owned(),
                json!({ "return_raw_output": raw }),
            );
        }
        payload.insert("pointwise_metric_spec".to_owned(), Value::Object(spec));
    } else {
        return Err(Error::Validation(format!(
            "Unsupported metric type or invalid metric name: {name}"
        )));
    }
    Ok(Value::Object(payload))
}

/// Passes a JSON Schema value through unchanged. Mirrors `t_json_schema`,
/// which (confirmed by reading the installed `google-genai` 2.19.0
/// `_transformers.py`, `def t_json_schema(origin): return origin`) really
/// is a pure passthrough with no validation or normalization -- the
/// `response_json_schema` field accepts an arbitrary user-authored JSON
/// Schema `serde_json::Value` verbatim, so there is nothing for this
/// crate's version to add either.
pub fn t_json_schema(value: Value) -> Result<Value> {
    Ok(value)
}

// Rust-only helpers (no upstream namesake in `_transformers.py`).

fn as_str<'a>(value: &'a Value, what: &str) -> Result<&'a str> {
    value
        .as_str()
        .ok_or_else(|| Error::Validation(format!("{what} must be a string, got {value}")))
}

fn as_object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| Error::Validation(format!("{what} must be an object, got {value}")))
}

fn check_mime_prefix(value: Value, prefix: &str) -> Result<Value> {
    // Python reads the pydantic *attribute* `blob.mime_type`, so it always
    // sees the snake_case spelling. This runs one step earlier in the same
    // pipeline, on `crate::types::Blob`'s JSON form -- and `Blob`'s
    // `#[serde(alias = "mimeType")]` is deserialize-only, so serializing a
    // `Blob` always yields `mime_type`. Reading only `mimeType` here made
    // every `t_audio_blob`/`t_image_blob` call fail with "unsupported mime
    // type: None", which broke `LiveSession::send_realtime_input` for all
    // audio and video chunks. Both spellings are accepted so a value that
    // arrived in wire casing still validates.
    let blob = as_object(&value, "blob")?;
    let mime_type = blob
        .get("mime_type")
        .or_else(|| blob.get("mimeType"))
        .and_then(Value::as_str);
    match mime_type {
        Some(m) if m.starts_with(prefix) => Ok(value),
        other => Err(Error::Validation(format!(
            "unsupported mime type: {other:?} (expected `{prefix}*`)"
        ))),
    }
}

/// Returns whether `value` is "truthy" by Python's rules (`bool(value)`):
/// `None`/`False`/`0`/`""`/an empty list or dict are falsy, everything
/// else -- including a non-empty dict, e.g. an `additionalProperties`
/// sub-schema -- is truthy. Needed to mirror
/// `_raise_for_unsupported_mldev_properties`'s
/// `schema.get('additionalProperties') or schema.get('additional_properties')`
/// check exactly (a bare presence/`is_some()` check would wrongly reject
/// an explicit `additional_properties: false`, which Python's `or` chain
/// (falsy) does not).
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Recursively applies [`raise_for_unsupported_mldev_properties`] to a
/// `Schema` and its nested sub-schemas (in `any_of`, `properties` values,
/// `defs` values, `items`, or a dict-shaped `additional_properties`), as
/// `process_schema` does while it recurses. Python only
/// raises when `not client.vertexai`; this crate targets the Gemini
/// Developer API (`mldev`) exclusively (Vertex AI/"Gemini Enterprise
/// Agent Platform mode" is out of scope, see `research.md` R-02/R-05), so
/// the check unconditionally applies here.
fn check_schema_tree(value: &Value) -> Result<()> {
    let Value::Object(map) = value else {
        return Ok(());
    };
    raise_for_unsupported_mldev_properties(value)?;
    if let Some(additional) = map.get("additional_properties") {
        check_schema_tree(additional)?;
    }
    if let Some(items) = map.get("items") {
        check_schema_tree(items)?;
    }
    if let Some(any_of) = map.get("any_of").and_then(Value::as_array) {
        for sub_schema in any_of {
            check_schema_tree(sub_schema)?;
        }
    }
    for key in ["properties", "defs"] {
        if let Some(sub_map) = map.get(key).and_then(Value::as_object) {
            for sub_schema in sub_map.values() {
                check_schema_tree(sub_schema)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A `Schema` goes out exactly as its Rust fields serialize --
    /// `snake_case`, unrenamed.
    ///
    /// Python's `t_schema` does dump a `types.Schema`, run `process_schema`
    /// (which renames `additional_properties`/`any_of`/`prefix_items`/
    /// `property_ordering` to camelCase), and then **re-validate the result
    /// back into a `types.Schema`** -- where those camelCase spellings are
    /// just aliases. `_common.convert_to_dict` then dumps without
    /// `by_alias`, so the rename round-trips away and the wire sees
    /// `snake_case`. Verified field-by-field against google-genai 2.19.0, and
    /// confirmed accepted by the live API. An earlier version of this
    /// transformer camelized here, which was a divergence rather than a fix.
    #[test]
    fn t_schema_leaves_field_names_in_snake_case_like_python() {
        let schema = json!({
            "type": "OBJECT",
            "min_properties": 1,
            "any_of": [{"type": "STRING", "max_length": 5}],
            "properties": {
                "user_id": {"type": "STRING", "min_length": 2},
                "another_field": {"type": "INTEGER"}
            },
            "property_ordering": ["user_id", "another_field"]
        });
        let result = t_schema(schema.clone()).unwrap();
        assert_eq!(result, schema);
    }

    #[test]
    fn t_schema_passes_null_through() {
        assert_eq!(t_schema(Value::Null).unwrap(), Value::Null);
    }

    #[test]
    fn t_schema_rejects_a_truthy_top_level_additional_properties() {
        let schema = json!({"type": "OBJECT", "additional_properties": true});
        let err = t_schema(schema).unwrap_err();
        assert!(err.to_string().contains("additionalProperties"));
    }

    #[test]
    fn t_schema_rejects_a_dict_shaped_additional_properties() {
        let schema = json!({
            "type": "OBJECT",
            "additional_properties": {"type": "STRING"}
        });
        assert!(t_schema(schema).is_err());
    }

    #[test]
    fn t_schema_allows_an_explicit_false_additional_properties() {
        // Python's `schema.get(...) or schema.get(...)` truthiness check
        // treats `False` as falsy, so it doesn't raise; this crate must
        // match that, not just check field presence.
        let schema = json!({"type": "OBJECT", "additional_properties": false});
        assert!(t_schema(schema).is_ok());
    }

    #[test]
    fn t_schema_rejects_additional_properties_nested_in_properties() {
        let schema = json!({
            "type": "OBJECT",
            "properties": {
                "nested": {"type": "OBJECT", "additional_properties": true}
            }
        });
        assert!(t_schema(schema).is_err());
    }

    #[test]
    fn t_schema_rejects_additional_properties_nested_in_any_of() {
        let schema = json!({
            "any_of": [{"type": "OBJECT", "additional_properties": true}]
        });
        assert!(t_schema(schema).is_err());
    }

    #[test]
    fn t_schema_rejects_additional_properties_nested_in_items() {
        let schema = json!({
            "type": "ARRAY",
            "items": {"type": "OBJECT", "additional_properties": true}
        });
        assert!(t_schema(schema).is_err());
    }

    /// The bare-voice-name shorthand expands to the same `snake_case`
    /// shape Python produces -- verified by running google-genai 2.19.0's
    /// `_GenerateContentParameters_to_mldev` with `speech_config="Kore"`,
    /// which yields
    /// `{"voice_config": {"prebuilt_voice_config": {"voice_name": "Kore"}}}`.
    #[test]
    fn t_speech_config_wraps_a_bare_voice_name_in_pythons_snake_case_shape() {
        let result = t_speech_config(json!("Kore")).unwrap();
        assert_eq!(
            result["voice_config"]["prebuilt_voice_config"]["voice_name"],
            "Kore"
        );
    }

    /// An already-structured config is passed through untouched. Python
    /// dumps it without `by_alias`, so its field names stay `snake_case` on
    /// the wire; an earlier version of this transformer camelized here,
    /// which was a divergence rather than a fix.
    #[test]
    fn t_speech_config_passes_an_object_through_unchanged() {
        let input = json!({"voice_config": {"prebuilt_voice_config": {"voice_name": "Puck"}}});
        let result = t_speech_config(input.clone()).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn t_model_adds_models_prefix() {
        assert_eq!(
            t_model(json!("gemini-2.5-flash")).unwrap(),
            json!("models/gemini-2.5-flash")
        );
    }

    #[test]
    fn t_model_leaves_prefixed_names_alone() {
        assert_eq!(
            t_model(json!("tunedModels/x")).unwrap(),
            json!("tunedModels/x")
        );
        assert_eq!(t_model(json!("models/x")).unwrap(), json!("models/x"));
    }

    #[test]
    fn t_model_rejects_empty_and_invalid_names() {
        assert!(t_model(json!("")).is_err());
        assert!(t_model(json!("a?b")).is_err());
    }

    #[test]
    fn t_extract_models_prefers_models_key() {
        let resp = json!({"models": [1], "tunedModels": [2]});
        assert_eq!(t_extract_models(resp).unwrap(), json!([1]));
    }

    #[test]
    fn t_extract_models_falls_back_to_tuned_then_publisher() {
        assert_eq!(
            t_extract_models(json!({"tunedModels": [2]})).unwrap(),
            json!([2])
        );
        assert_eq!(
            t_extract_models(json!({"publisherModels": [3]})).unwrap(),
            json!([3])
        );
        assert_eq!(t_extract_models(json!({})).unwrap(), json!([]));
    }

    #[test]
    fn t_contents_wraps_a_single_content_and_rejects_empty() {
        assert_eq!(
            t_contents(json!({"role": "user"})).unwrap(),
            json!([{"role": "user"}])
        );
        assert!(t_contents(Value::Null).is_err());
        assert!(t_contents(json!([])).is_err());
    }

    #[test]
    fn t_cached_content_name_prepends_collection_for_bare_ids() {
        assert_eq!(
            t_cached_content_name(json!("abc123")).unwrap(),
            json!("cachedContents/abc123")
        );
        assert_eq!(
            t_cached_content_name(json!("cachedContents/abc123")).unwrap(),
            json!("cachedContents/abc123")
        );
    }

    #[test]
    fn t_batch_job_source_requires_exactly_one_source() {
        assert!(t_batch_job_source(json!({"inlined_requests": [1], "file_name": null})).is_ok());
        assert!(
            t_batch_job_source(json!({"inlined_requests": [1], "file_name": "files/x"})).is_err()
        );
        assert!(t_batch_job_source(json!({})).is_err());
    }

    #[test]
    fn t_batch_job_name_extracts_the_bare_id() {
        assert_eq!(
            t_batch_job_name(json!("batches/abc")).unwrap(),
            json!("abc")
        );
        assert!(t_batch_job_name(json!("abc")).is_err());
    }

    #[test]
    fn t_job_state_maps_batch_states_and_passes_through_unknown() {
        assert_eq!(
            t_job_state(json!("BATCH_STATE_SUCCEEDED")).unwrap(),
            json!("JOB_STATE_SUCCEEDED")
        );
        assert_eq!(
            t_job_state(json!("SOMETHING_ELSE")).unwrap(),
            json!("SOMETHING_ELSE")
        );
    }

    #[test]
    fn t_file_name_strips_prefixes() {
        assert_eq!(t_file_name(json!("files/abc")).unwrap(), json!("abc"));
        assert_eq!(
            t_file_name(json!(
                "https://generativelanguage.googleapis.com/v1beta/files/abc123:download"
            ))
            .unwrap(),
            json!("abc123")
        );
        assert_eq!(t_file_name(json!("abc")).unwrap(), json!("abc"));
    }

    #[test]
    fn t_tuning_job_status_maps_known_states() {
        assert_eq!(
            t_tuning_job_status(json!("ACTIVE")).unwrap(),
            json!("JOB_STATE_SUCCEEDED")
        );
        assert_eq!(
            t_tuning_job_status(json!("JOB_STATE_RUNNING")).unwrap(),
            json!("JOB_STATE_RUNNING")
        );
    }

    #[test]
    fn t_live_speech_config_rejects_multi_speaker() {
        assert!(t_live_speech_config(json!({"multi_speaker_voice_config": {}})).is_err());
        assert!(t_live_speech_config(json!({})).is_ok());
    }

    #[test]
    fn t_image_and_audio_blob_validate_mime_prefix() {
        assert!(t_image_blob(json!({"mimeType": "image/png"})).is_ok());
        assert!(t_image_blob(json!({"mimeType": "audio/mp3"})).is_err());
        assert!(t_audio_blob(json!({"mimeType": "audio/mp3"})).is_ok());
    }

    #[test]
    fn t_recv_batch_job_destination_renames_embedding_responses() {
        let dest = json!({
            "inlinedResponses": {"inlinedResponses": [{"response": {"embedding": {}}}]}
        });
        let result = t_recv_batch_job_destination(dest).unwrap();
        assert!(result.get("inlinedEmbedContentResponses").is_some());
        assert!(result.get("inlinedResponses").is_none());
    }

    #[test]
    fn t_image_predictions_maps_images_and_skips_predictions_without_one() {
        let predictions = json!([
            {"image": {"gcsUri": "gs://a", "imageBytes": "AAAA"}},
            {"raiFilteredReason": "x"},
        ]);
        assert_eq!(
            t_image_predictions(predictions).unwrap(),
            json!([{"image": {"gcs_uri": "gs://a", "image_bytes": "AAAA"}}])
        );
        assert_eq!(t_image_predictions(json!([])).unwrap(), Value::Null);
        assert!(t_image_predictions(json!([{"image": {"gcsUri": "gs://a"}}])).is_err());
    }

    #[test]
    fn process_enum_stringifies_integers_and_rejects_other_values() {
        let members = [("A".to_owned(), json!(1)), ("B".to_owned(), json!("x"))];
        assert_eq!(
            process_enum(&members).unwrap(),
            json!({"type": "STRING", "enum": ["1", "x"]})
        );
        assert!(process_enum(&[("A".to_owned(), json!(1.5))]).is_err());
    }

    #[test]
    fn t_client_content_wraps_turns_and_reports_invalid_input() {
        assert_eq!(
            t_client_content(Value::Null, false).unwrap(),
            json!({"turn_complete": false})
        );
        assert_eq!(
            t_client_content(json!({"role": "user"}), true).unwrap(),
            json!({"turns": [{"role": "user"}], "turn_complete": true})
        );
        assert!(t_client_content(json!(1), true).is_err());
    }

    #[test]
    fn t_tool_response_wraps_function_responses() {
        let response = json!({"name": "f", "response": {"ok": true}});
        assert_eq!(
            t_tool_response(response.clone()).unwrap(),
            json!({"function_responses": [response]})
        );
        assert!(t_tool_response(json!([])).is_err());
    }

    #[test]
    fn t_metrics_maps_named_metrics_and_rejects_unknown_ones() {
        let payload = t_metrics(json!([{"name": "Exact_Match"}, {"name": "rouge_l"}])).unwrap();
        assert_eq!(payload[0]["exact_match_spec"], json!({}));
        assert_eq!(payload[1]["rouge_spec"], json!({"rouge_type": "rougel"}));
        assert_eq!(
            payload[0]["aggregation_metrics"],
            json!(["AVERAGE", "STANDARD_DEVIATION"])
        );
        assert!(t_metrics(json!([{"name": "nope"}])).is_err());
    }

    mod resolve_operation {
        use secrecy::SecretString;

        use super::*;
        use crate::types::HttpOptions;

        fn http() -> HttpClient {
            HttpClient::new(SecretString::from("key"), &HttpOptions::default()).unwrap()
        }

        #[tokio::test]
        async fn a_value_that_is_not_an_operation_is_returned_unchanged() {
            let value = json!({"name": "models/m"});
            assert_eq!(
                t_resolve_operation(&http(), value.clone()).await.unwrap(),
                value
            );
        }

        #[tokio::test]
        async fn a_finished_operation_yields_its_response() {
            let done = json!({"name": "x/operations/1", "done": true, "response": {"a": 1}});
            assert_eq!(
                t_resolve_operation(&http(), done).await.unwrap(),
                json!({"a": 1})
            );
        }

        #[tokio::test]
        async fn a_finished_operation_with_an_error_fails() {
            let failed = json!({"name": "x/operations/1", "done": true, "error": {"code": 3}});
            assert!(t_resolve_operation(&http(), failed).await.is_err());
        }
    }

    #[test]
    fn t_recv_batch_job_destination_leaves_non_embedding_responses_alone() {
        let dest =
            json!({"inlinedResponses": {"inlinedResponses": [{"response": {"text": "hi"}}]}});
        let result = t_recv_batch_job_destination(dest.clone()).unwrap();
        assert_eq!(result, dest);
    }
}

#[cfg(test)]
mod blob_mime_tests {
    use serde_json::json;

    use super::{t_audio_blob, t_image_blob};

    /// Regression test for a bug that broke every realtime audio and video
    /// chunk: `check_mime_prefix` read `mimeType`, but `crate::types::Blob`
    /// serializes its MIME field as `mime_type` (the `mimeType` serde
    /// attribute is a deserialize-only alias). Every call therefore saw
    /// `None` and rejected the blob, so `LiveSession::send_realtime_input`
    /// could never send audio or video.
    #[test]
    fn audio_blob_accepts_the_snake_case_spelling_serde_actually_emits() {
        let blob = serde_json::to_value(crate::types::Blob {
            data: Some(b"pcm".to_vec()),
            mime_type: Some("audio/pcm;rate=16000".to_owned()),
            ..Default::default()
        })
        .unwrap();
        assert!(
            blob.get("mime_type").is_some(),
            "Blob must still serialize as `mime_type`; if this changes, revisit check_mime_prefix"
        );
        assert!(t_audio_blob(blob).is_ok());
    }

    #[test]
    fn audio_blob_also_accepts_the_wire_spelling() {
        let blob = json!({"data": "cGNt", "mimeType": "audio/pcm;rate=16000"});
        assert!(t_audio_blob(blob).is_ok());
    }

    #[test]
    fn image_blob_accepts_the_snake_case_spelling() {
        let blob = serde_json::to_value(crate::types::Blob {
            data: Some(b"png".to_vec()),
            mime_type: Some("image/png".to_owned()),
            ..Default::default()
        })
        .unwrap();
        assert!(t_image_blob(blob).is_ok());
    }

    #[test]
    fn a_mismatched_prefix_is_still_rejected() {
        let blob = json!({"data": "cGNt", "mime_type": "image/png"});
        let err = t_audio_blob(blob).unwrap_err();
        assert!(
            err.to_string().contains("unsupported mime type"),
            "unexpected error: {err}"
        );
    }
}

#[cfg(test)]
mod conformance_tests {
    use serde_json::json;

    use super::*;

    fn validation_message(result: Result<Value>) -> String {
        match result {
            Err(Error::Validation(message)) => message,
            other => panic!("expected Error::Validation, got {other:?}"),
        }
    }

    // t_caches_model (mldev branch: no project/location prefixing)

    #[test]
    fn t_caches_model_adds_models_prefix_for_bare_names() {
        assert_eq!(
            t_caches_model(json!("gemini-2.0-flash")).unwrap(),
            json!("models/gemini-2.0-flash")
        );
    }

    #[test]
    fn t_caches_model_keeps_models_and_tuned_models_prefixed_names() {
        assert_eq!(
            t_caches_model(json!("models/m")).unwrap(),
            json!("models/m")
        );
        assert_eq!(
            t_caches_model(json!("tunedModels/m")).unwrap(),
            json!("tunedModels/m")
        );
    }

    #[test]
    fn t_caches_model_rejects_an_empty_name() {
        validation_message(t_caches_model(json!("")));
    }

    // t_function_response

    #[test]
    fn t_function_response_accepts_a_function_response_object() {
        let input = json!({"name": "f", "response": {"ok": true}});
        assert_eq!(t_function_response(input.clone()).unwrap(), input);
    }

    #[test]
    fn t_function_response_rejects_empty_input_as_required() {
        assert_eq!(
            validation_message(t_function_response(json!({}))),
            "function_response is required."
        );
        validation_message(t_function_response(Value::Null));
    }

    #[test]
    fn t_function_response_rejects_a_non_object_with_its_type_name() {
        let message = validation_message(t_function_response(json!("text")));
        assert!(
            message.contains("Unsupported function_response type: str"),
            "{message}"
        );
    }

    #[test]
    fn t_function_response_rejects_an_object_with_an_unknown_field() {
        validation_message(t_function_response(json!({"name": "f", "bogus": 1})));
    }

    // _raise_for_unsupported_schema_type

    #[test]
    fn raise_for_unsupported_schema_type_always_fails_naming_the_origin() {
        let message = validation_message(
            raise_for_unsupported_schema_type(&json!("weird")).map(|()| Value::Null),
        );
        assert!(
            message.starts_with("Unsupported schema type: "),
            "{message}"
        );
        assert!(message.contains("weird"), "{message}");
    }

    // _raise_for_unsupported_mldev_properties
    // Intentional difference: upstream's message has a stray ", ," typo; the
    // Rust text omits it, so only the stable part is asserted.

    #[test]
    fn raise_for_unsupported_mldev_properties_rejects_truthy_additional_properties() {
        let message = validation_message(
            raise_for_unsupported_mldev_properties(&json!({"additionalProperties": true}))
                .map(|()| Value::Null),
        );
        assert!(
            message.contains("only supported in Gemini Enterprise Agent Platform mode"),
            "{message}"
        );
    }

    #[test]
    fn raise_for_unsupported_mldev_properties_rejects_the_snake_case_spelling() {
        assert!(
            raise_for_unsupported_mldev_properties(
                &json!({"additional_properties": {"type": "string"}})
            )
            .is_err()
        );
    }

    #[test]
    fn raise_for_unsupported_mldev_properties_accepts_falsy_or_absent_additional_properties() {
        assert!(
            raise_for_unsupported_mldev_properties(&json!({"additionalProperties": false})).is_ok()
        );
        assert!(
            raise_for_unsupported_mldev_properties(&json!({"additionalProperties": null})).is_ok()
        );
        assert!(raise_for_unsupported_mldev_properties(&json!({"type": "object"})).is_ok());
    }

    // t_embedding_batch_job_source

    #[test]
    fn t_embedding_batch_job_source_accepts_exactly_one_source() {
        let by_file = json!({"file_name": "files/abc"});
        assert_eq!(
            t_embedding_batch_job_source(by_file.clone()).unwrap(),
            by_file
        );
        let inlined = json!({"inlined_requests": {"contents": []}});
        assert_eq!(
            t_embedding_batch_job_source(inlined.clone()).unwrap(),
            inlined
        );
    }

    #[test]
    fn t_embedding_batch_job_source_rejects_no_source() {
        assert_eq!(
            validation_message(t_embedding_batch_job_source(json!({}))),
            MLDEV_SOURCE_ERROR
        );
    }

    #[test]
    fn t_embedding_batch_job_source_rejects_both_sources() {
        let both = json!({"file_name": "files/abc", "inlined_requests": {}});
        assert_eq!(
            validation_message(t_embedding_batch_job_source(both)),
            MLDEV_SOURCE_ERROR
        );
    }

    #[test]
    fn t_embedding_batch_job_source_ignores_null_sources_when_counting() {
        let one_null = json!({"file_name": "files/abc", "inlined_requests": null});
        assert!(t_embedding_batch_job_source(one_null).is_ok());
    }

    #[test]
    fn t_embedding_batch_job_source_rejects_non_objects_with_the_type_name() {
        let message = validation_message(t_embedding_batch_job_source(json!("files/abc")));
        assert!(
            message.starts_with("Unsupported source type: "),
            "{message}"
        );
        assert!(message.contains("str"), "{message}");
    }

    // t_content_strict

    #[test]
    fn t_content_strict_accepts_a_content_object_unchanged() {
        let content = json!({"role": "user", "parts": [{"text": "hi"}]});
        assert_eq!(t_content_strict(content.clone()).unwrap(), content);
    }

    #[test]
    fn t_content_strict_does_not_coerce_a_bare_string_into_content() {
        let message = validation_message(t_content_strict(json!("hi")));
        assert_eq!(
            message,
            "Could not convert input (type \"str\") to `types.Content`"
        );
    }

    #[test]
    fn t_content_strict_rejects_an_object_with_an_unknown_field() {
        validation_message(t_content_strict(json!({"role": "user", "bogus": 1})));
    }
}
