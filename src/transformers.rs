//! Hand-written `t_*` transformers used by the generated converters
//! (`crate::converters::generated`), mirroring Python's `_transformers.py`
//! / `_base_transformers.py`.
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
    clippy::unnecessary_wraps,
    clippy::needless_pass_by_value,
    reason = "every `t_*` transformer intentionally shares Python's uniform `fn(Value) -> Result<Value>` shape, even where a given transformer currently has no failure path or doesn't need ownership of its argument"
)]

use serde_json::{Map, Value};

use crate::error::{Error, Result};

/// Converts a `snake_case` identifier to `camelCase`, matching pydantic's
/// `alias_generator=to_camel` (used by every generated type; see
/// `tools/codegen/gen_types.py`'s `#[serde(alias = ...)]` output).
fn snake_to_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper_next = false;
    for ch in s.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Recursively renames every object key in `value` from `snake_case` to
/// `camelCase`. Only safe for objects whose keys are *all* known field
/// names (e.g. `SpeechConfig`/`VoiceConfig`) -- **not** for `Schema`,
/// where `properties`/`$defs` map keys are arbitrary user-chosen field
/// names that must be preserved verbatim (see [`camelize_schema`]).
fn camelize_keys_recursive(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (snake_to_camel(&k), camelize_keys_recursive(v)))
                .collect(),
        ),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(camelize_keys_recursive).collect())
        }
        other => other,
    }
}

/// The subset of `Schema`'s own field names that differ between their
/// Rust (`snake_case`) and wire (`camelCase`) spelling; every other field
/// (`type`, `format`, `items`, `properties`, `enum`, ...) is already a
/// single word and spelled identically in both. Mirrors the 4-entry
/// rename table in Python's `_transformers.process_schema`, extended
/// with the `min_*`/`max_*` fields it (and pydantic's `by_alias=True`
/// serialization, which this crate has no equivalent step for) also
/// renames.
const SCHEMA_FIELD_RENAMES: &[(&str, &str)] = &[
    ("additional_properties", "additionalProperties"),
    ("any_of", "anyOf"),
    ("max_items", "maxItems"),
    ("max_length", "maxLength"),
    ("max_properties", "maxProperties"),
    ("min_items", "minItems"),
    ("min_length", "minLength"),
    ("min_properties", "minProperties"),
    ("property_ordering", "propertyOrdering"),
];

/// Recursively renames a [`crate::types::Schema`] value's own field names
/// to their wire (camelCase) spelling, without touching the arbitrary
/// user-chosen keys inside `properties`/`defs` maps (those are recursed
/// into as *values*, not renamed as keys). See `SCHEMA_FIELD_RENAMES`.
fn camelize_schema(value: Value) -> Value {
    let Value::Object(map) = value else {
        return value;
    };
    let renames: std::collections::HashMap<&str, &str> =
        SCHEMA_FIELD_RENAMES.iter().copied().collect();
    let mut out = Map::new();
    for (key, val) in map {
        let wire_key = renames
            .get(key.as_str())
            .copied()
            .unwrap_or(key.as_str())
            .to_owned();
        let converted = match key.as_str() {
            "any_of" => Value::Array(
                val.as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(camelize_schema)
                    .collect(),
            ),
            "items" | "additional_properties" => camelize_schema(val),
            "properties" | "defs" => Value::Object(
                val.as_object()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(k, v)| (k, camelize_schema(v)))
                    .collect(),
            ),
            _ => val,
        };
        out.insert(wire_key, converted);
    }
    Value::Object(out)
}

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

/// Normalizes a model resource name (mirrors the `mldev` branch of
/// Python's `t_model`).
pub(crate) fn t_model(value: Value) -> Result<Value> {
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
pub(crate) fn t_models_url(base_models: Value) -> Result<Value> {
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
pub(crate) fn t_extract_models(value: Value) -> Result<Value> {
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
pub(crate) fn t_caches_model(value: Value) -> Result<Value> {
    t_model(value)
}

/// Wraps a single Blob-shaped value into a one-element array, or passes an
/// array through unchanged. Mirrors `t_blobs`.
pub(crate) fn t_blobs(value: Value) -> Result<Value> {
    Ok(match value {
        Value::Array(items) => Value::Array(items),
        other => Value::Array(vec![other]),
    })
}

fn check_mime_prefix(value: Value, prefix: &str) -> Result<Value> {
    let mime_type = as_object(&value, "blob")?
        .get("mimeType")
        .and_then(Value::as_str);
    match mime_type {
        Some(m) if m.starts_with(prefix) => Ok(value),
        other => Err(Error::Validation(format!(
            "unsupported mime type: {other:?} (expected `{prefix}*`)"
        ))),
    }
}

/// Validates a Blob's `mimeType` starts with `image/`. Mirrors
/// `t_image_blob`.
pub(crate) fn t_image_blob(value: Value) -> Result<Value> {
    check_mime_prefix(value, "image/")
}

/// Validates a Blob's `mimeType` starts with `audio/`. Mirrors
/// `t_audio_blob`.
pub(crate) fn t_audio_blob(value: Value) -> Result<Value> {
    check_mime_prefix(value, "audio/")
}

/// Validates a Content-shaped value is present. Mirrors `t_content`;
/// the Python original's `str`/`Part`/`PIL.Image` coercion happens instead
/// in `types::conversions` before this is called.
pub(crate) fn t_content(value: Value) -> Result<Value> {
    if value.is_null() {
        return Err(Error::Validation("content is required".to_owned()));
    }
    Ok(value)
}

/// Ensures `contents` is a non-empty array, wrapping a single Content into
/// a one-element array. Mirrors `t_contents`.
pub(crate) fn t_contents(value: Value) -> Result<Value> {
    match value {
        Value::Null => Err(Error::Validation("contents are required".to_owned())),
        Value::Array(items) if items.is_empty() => {
            Err(Error::Validation("contents are required".to_owned()))
        }
        Value::Array(items) => Ok(Value::Array(items)),
        other => Ok(Value::Array(vec![other])),
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
/// [`crate::converters::generated::live_converters::content_to_mldev`]
/// itself, camelizing each `Content`'s `Part`s (`inline_data` ->
/// `inlineData`, etc.).
pub(crate) fn t_contents_for_embed(value: Value) -> Result<Value> {
    let items = match value {
        Value::Array(items) => items,
        other => vec![other],
    };
    let converted = items
        .into_iter()
        .map(|item| {
            crate::converters::generated::live_converters::content_to_mldev(&item, None, None)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Value::Array(converted))
}

/// Prepends a resource-name collection prefix if missing and doing so
/// would not violate the given collection hierarchy depth. Mirrors the
/// `mldev` (non-Vertex) branch of `_resource_name`.
fn resource_name_mldev(name: &str, collection: &str, hierarchy_depth: usize) -> String {
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

/// Mirrors the `mldev` branch of `t_cached_content_name`
/// (`_resource_name(..., collection_identifier='cachedContents')`).
pub(crate) fn t_cached_content_name(value: Value) -> Result<Value> {
    let name = as_str(&value, "cached content name")?;
    Ok(Value::String(resource_name_mldev(
        name,
        "cachedContents",
        2,
    )))
}

/// Validates exactly one of `inlined_requests`/`file_name` is set (the
/// mldev batch-job-source constraint). Mirrors the `mldev` branch of
/// `t_batch_job_source`; the Rust API already requires a typed
/// `BatchJobSource`, so the `str`/`list` coercion branches don't apply.
pub(crate) fn t_batch_job_source(value: Value) -> Result<Value> {
    let obj = as_object(&value, "batch job source")?;
    let has_inlined = obj.get("inlined_requests").is_some_and(|v| !v.is_null());
    let has_file = obj.get("file_name").is_some_and(|v| !v.is_null());
    if u8::from(has_inlined) + u8::from(has_file) != 1 {
        return Err(Error::Validation(
            "exactly one of `inlined_requests` or `file_name` must be set for a Gemini Developer API batch job source".to_owned(),
        ));
    }
    Ok(value)
}

/// Renames `inlinedResponses` to `inlinedEmbedContentResponses` if the
/// responses look like embedding results. Mirrors
/// `t_recv_batch_job_destination`.
pub(crate) fn t_recv_batch_job_destination(value: Value) -> Result<Value> {
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
    if looks_like_embedding {
        if let Some(inlined) = dest.remove("inlinedResponses") {
            dest.insert("inlinedEmbedContentResponses".to_owned(), inlined);
        }
    }
    Ok(Value::Object(dest))
}

/// Extracts the bare id from a `batches/{id}` resource name. Mirrors the
/// `mldev` branch of `t_batch_job_name`.
pub(crate) fn t_batch_job_name(value: Value) -> Result<Value> {
    let name = as_str(&value, "batch job name")?;
    match name
        .strip_prefix("batches/")
        .filter(|rest| !rest.is_empty() && !rest.contains('/'))
    {
        Some(id) => Ok(Value::String(id.to_owned())),
        None => Err(Error::Validation(format!("invalid batch job name: {name}"))),
    }
}

/// Maps a `BATCH_STATE_*` wire value to the corresponding `JOB_STATE_*`
/// value, passing unrecognized values through unchanged. Mirrors
/// `t_job_state`.
pub(crate) fn t_job_state(value: Value) -> Result<Value> {
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

/// Strips a `files/` prefix (or extracts the id from a `https://.../files/{id}`
/// URI). Mirrors `t_file_name`; the `File`/`Video`/`GeneratedVideo` object
/// coercion in Python is done by the Rust API's `FileRef`-style argument
/// types before this is called.
pub(crate) fn t_file_name(value: Value) -> Result<Value> {
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
pub(crate) fn t_tuning_job_status(value: Value) -> Result<Value> {
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

/// Wraps a bare voice-name string into a full `SpeechConfig` shape;
/// passes an already-object `SpeechConfig` through unchanged. Mirrors
/// `t_speech_config`.
///
/// No generated `_to_mldev` converter exists for `SpeechConfig` (unlike
/// `Content`/`Tool`, which get a recursive converter call *after* their
/// `t_*` transformer runs -- see the module doc), so this transformer
/// must itself produce wire-cased (camelCase) keys.
pub(crate) fn t_speech_config(value: Value) -> Result<Value> {
    match value {
        Value::Null => Ok(Value::Null),
        Value::String(voice_name) => Ok(serde_json::json!({
            "voiceConfig": { "prebuiltVoiceConfig": { "voiceName": voice_name } }
        })),
        object => Ok(camelize_keys_recursive(object)),
    }
}

/// Validates `multi_speaker_voice_config` is not set (unsupported by the
/// Live API), then camelizes the result (see [`t_speech_config`] doc for
/// why this transformer must do so itself). Mirrors `t_live_speech_config`.
pub(crate) fn t_live_speech_config(value: Value) -> Result<Value> {
    let has_multi_speaker = value
        .as_object()
        .and_then(|o| o.get("multi_speaker_voice_config"))
        .is_some_and(|v| !v.is_null());
    if has_multi_speaker {
        return Err(Error::Validation(
            "multi_speaker_voice_config is not supported in the live API".to_owned(),
        ));
    }
    Ok(camelize_keys_recursive(value))
}

/// Passes an already-typed `Tool` value through. Mirrors the
/// dict/duck-typed-`Tool` branches of `t_tool`; Python's bare-callable and
/// MCP-tool coercion happen in `crate::afc`/`crate::mcp` before a `Tool`
/// value ever reaches this converter layer.
pub(crate) fn t_tool(value: Value) -> Result<Value> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    Ok(value)
}

/// Passes an array of already-typed `Tool` values through. Python's
/// per-callable-tool merging (combining every function-only `Tool` into
/// one) is instead done once, in Rust, when building the `Tool` list (see
/// `crate::afc`). Mirrors `t_tools`.
pub(crate) fn t_tools(value: Value) -> Result<Value> {
    match value {
        Value::Array(items) => Ok(Value::Array(items)),
        Value::Null => Ok(Value::Array(vec![])),
        other => Ok(Value::Array(vec![other])),
    }
}

/// Passes a JSON Schema value through unchanged. Mirrors `t_json_schema`
/// (`return origin`).
pub(crate) fn t_json_schema(value: Value) -> Result<Value> {
    Ok(value)
}

/// Renames an already-typed `Schema` value's own fields to their wire
/// (camelCase) spelling (see [`camelize_schema`]), treating an absent
/// schema as `Null`. Python's Python-type/pydantic-model/enum coercion
/// (`t_schema`) is not applicable: the Rust API only accepts a
/// [`crate::types::Schema`] directly, or a JSON Schema built from a Rust
/// type via `schemars` (see `response_json_schema`). No generated
/// `_to_mldev` converter exists for `Schema` (Python's `t_schema` does
/// this renaming itself, via `process_schema` + a final `by_alias=True`
/// serialization this crate has no equivalent step for), so this
/// transformer must produce wire casing itself.
pub(crate) fn t_schema(value: Value) -> Result<Value> {
    if value.is_null() {
        Ok(Value::Null)
    } else {
        Ok(camelize_schema(value))
    }
}

/// Passes a value through unchanged. Mirrors `t_bytes`'s
/// already-not-`bytes` fallback: this crate's generated types serialize
/// `Vec<u8>` fields as base64 strings via `serde_with` before a value
/// ever reaches the converter layer (see `types::generated`), so there is
/// never raw binary data here to encode.
pub(crate) fn t_bytes(value: Value) -> Result<Value> {
    Ok(value)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn camelize_schema_renames_known_fields_but_preserves_user_property_names() {
        let schema = json!({
            "type": "OBJECT",
            "min_length": 1,
            "any_of": [{"type": "STRING", "max_length": 5}],
            "properties": {
                "user_id": {"type": "STRING", "min_length": 2},
                "another_field": {"type": "INTEGER"}
            },
            "property_ordering": ["user_id", "another_field"]
        });
        let result = camelize_schema(schema);
        assert_eq!(result["minLength"], 1);
        assert_eq!(result["anyOf"][0]["maxLength"], 5);
        assert_eq!(
            result["propertyOrdering"],
            json!(["user_id", "another_field"])
        );
        // Property *names* (arbitrary user data) must never be renamed.
        assert!(result["properties"].get("user_id").is_some());
        assert!(result["properties"].get("userId").is_none());
        assert_eq!(result["properties"]["user_id"]["minLength"], 2);
    }

    #[test]
    fn t_schema_camelizes_a_schema_object() {
        let schema = json!({"type": "OBJECT", "min_properties": 1});
        let result = t_schema(schema).unwrap();
        assert_eq!(result["minProperties"], 1);
    }

    #[test]
    fn t_schema_passes_null_through() {
        assert_eq!(t_schema(Value::Null).unwrap(), Value::Null);
    }

    #[test]
    fn t_speech_config_wraps_a_bare_voice_name_with_camel_keys() {
        let result = t_speech_config(json!("Kore")).unwrap();
        assert_eq!(
            result["voiceConfig"]["prebuiltVoiceConfig"]["voiceName"],
            "Kore"
        );
    }

    #[test]
    fn t_speech_config_camelizes_an_object_input() {
        let result = t_speech_config(
            json!({"voice_config": {"prebuilt_voice_config": {"voice_name": "Puck"}}}),
        )
        .unwrap();
        assert_eq!(
            result["voiceConfig"]["prebuiltVoiceConfig"]["voiceName"],
            "Puck"
        );
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
    fn t_recv_batch_job_destination_leaves_non_embedding_responses_alone() {
        let dest =
            json!({"inlinedResponses": {"inlinedResponses": [{"response": {"text": "hi"}}]}});
        let result = t_recv_batch_job_destination(dest.clone()).unwrap();
        assert_eq!(result, dest);
    }
}
