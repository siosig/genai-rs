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
    // Python's `_EmbedContentParametersPrivate_to_mldev` is a bare
    // `[item for item in t.t_contents_for_embed(...)]`: the `Content`
    // objects are passed through untouched, and `_common.convert_to_dict`
    // later dumps them *without* `by_alias`, so parts keep their
    // snake_case spelling (`inline_data`, `mime_type`) on the wire. An
    // earlier version of this function camelized here; that was a
    // divergence, not a fix -- verified against google-genai 2.19.0.
    Ok(Value::Array(items))
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
            "voice_config": { "prebuilt_voice_config": { "voice_name": voice_name } }
        })),
        object => Ok(object),
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
    Ok(value)
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

/// Passes a JSON Schema value through unchanged. Mirrors `t_json_schema`,
/// which (confirmed by reading the installed `google-genai` 2.19.0
/// `_transformers.py`, `def t_json_schema(origin): return origin`) really
/// is a pure passthrough with no validation or normalization -- the
/// `response_json_schema` field accepts an arbitrary user-authored JSON
/// Schema `serde_json::Value` verbatim, so there is nothing for this
/// crate's version to add either.
pub(crate) fn t_json_schema(value: Value) -> Result<Value> {
    Ok(value)
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

/// Recursively errors if any `Schema` (or nested sub-schema, in `any_of`,
/// `properties` values, `defs` values, `items`, or a dict-shaped
/// `additional_properties`) sets a truthy `additional_properties`.
/// Mirrors `_raise_for_unsupported_mldev_properties`, called at the top of
/// every `process_schema` invocation (i.e. once per schema level, since
/// `process_schema` recurses into every nested schema). Python only
/// raises when `not client.vertexai`; this crate targets the Gemini
/// Developer API (`mldev`) exclusively (Vertex AI/"Gemini Enterprise
/// Agent Platform mode" is out of scope, see `research.md` R-02/R-05), so
/// the check unconditionally applies here.
fn reject_unsupported_mldev_properties(value: &Value) -> Result<()> {
    let Value::Object(map) = value else {
        return Ok(());
    };
    if let Some(additional) = map.get("additional_properties") {
        if is_truthy(additional) {
            return Err(Error::Validation(
                "additionalProperties is only supported in Gemini Enterprise Agent Platform \
                 mode, not in Gemini Developer API mode."
                    .to_owned(),
            ));
        }
        reject_unsupported_mldev_properties(additional)?;
    }
    if let Some(items) = map.get("items") {
        reject_unsupported_mldev_properties(items)?;
    }
    if let Some(any_of) = map.get("any_of").and_then(Value::as_array) {
        for sub_schema in any_of {
            reject_unsupported_mldev_properties(sub_schema)?;
        }
    }
    for key in ["properties", "defs"] {
        if let Some(sub_map) = map.get(key).and_then(Value::as_object) {
            for sub_schema in sub_map.values() {
                reject_unsupported_mldev_properties(sub_schema)?;
            }
        }
    }
    Ok(())
}

/// Renames an already-typed `Schema` value's own fields to their wire
/// (camelCase) spelling (see [`camelize_schema`]) and rejects an
/// unsupported `additional_properties` (see
/// [`reject_unsupported_mldev_properties`]), treating an absent schema as
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
pub(crate) fn t_schema(value: Value) -> Result<Value> {
    if value.is_null() {
        Ok(Value::Null)
    } else {
        reject_unsupported_mldev_properties(&value)?;
        Ok(value)
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
