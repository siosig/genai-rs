//! Hand-written extensions to the generated types: ergonomic constructors
//! and response accessors that mirror Python properties/classmethods, plus
//! the handful of small support types the generator's field-type overrides
//! reference (see `tools/codegen/gen_types.py` `FIELD_TYPE_OVERRIDES`).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::generated::{
    BatchJob, CodeExecutionResult, ExecutableCode, FunctionCall, GenerateContentConfig,
    GenerateContentResponse, GenerateVideosOperation, ImportFileOperation, JSONSchema,
    JSONSchemaType, Language, Outcome, Part, PartMediaResolution, PartMediaResolutionLevel, Schema,
    TuningJob, Type, UploadToFileSearchStoreOperation,
};
use crate::{
    converters::generated::operations_converters as op_conv,
    errors::{Error, Result},
};

/// Vertex AI job states that count as succeeded. Mirrors Python's
/// `JOB_STATES_SUCCEEDED_VERTEX`.
pub const JOB_STATES_SUCCEEDED_VERTEX: &[&str] = &["JOB_STATE_SUCCEEDED"];

/// Gemini Developer API job states that count as succeeded. Mirrors
/// Python's `JOB_STATES_SUCCEEDED_MLDEV`.
pub const JOB_STATES_SUCCEEDED_MLDEV: &[&str] = &["ACTIVE"];

/// Every job state that counts as succeeded (Vertex AI states followed by
/// Gemini Developer API states). Mirrors Python's `JOB_STATES_SUCCEEDED`
/// (`JOB_STATES_SUCCEEDED_VERTEX + JOB_STATES_SUCCEEDED_MLDEV`). The
/// Developer API's tuning converter reports `JOB_STATE_*` values, so the
/// Vertex names are needed even for this crate's Developer-API-only use.
pub const JOB_STATES_SUCCEEDED: &[&str] = &["JOB_STATE_SUCCEEDED", "ACTIVE"];

/// Vertex AI job states that count as ended. Mirrors Python's
/// `JOB_STATES_ENDED_VERTEX`.
pub const JOB_STATES_ENDED_VERTEX: &[&str] = &[
    "JOB_STATE_SUCCEEDED",
    "JOB_STATE_FAILED",
    "JOB_STATE_CANCELLED",
    "JOB_STATE_EXPIRED",
];

/// Gemini Developer API job states that count as ended. Mirrors Python's
/// `JOB_STATES_ENDED_MLDEV`.
pub const JOB_STATES_ENDED_MLDEV: &[&str] = &["ACTIVE", "FAILED"];

/// Every job state that counts as ended (Vertex AI states followed by
/// Gemini Developer API states). Mirrors Python's `JOB_STATES_ENDED`.
pub const JOB_STATES_ENDED: &[&str] = &[
    "JOB_STATE_SUCCEEDED",
    "JOB_STATE_FAILED",
    "JOB_STATE_CANCELLED",
    "JOB_STATE_EXPIRED",
    "ACTIVE",
    "FAILED",
];

/// A long-running operation that can be polled via
/// [`crate::operations::Operations::get`]. Mirrors Python's abstract
/// `types.Operation` (re-exported as [`crate::operations::OperationLike`]).
///
/// Implemented for every operation type this crate's methods return:
/// [`GenerateVideosOperation`] (from `models().generate_videos`),
/// [`ImportFileOperation`] (from `file_search_stores().import_file`), and
/// [`UploadToFileSearchStoreOperation`] (from
/// `file_search_stores().upload_to_file_search_store`). Mirrors Python's
/// `operations.get`, which is generic over `TypeVar('T', bound=types.Operation)`.
pub trait Operation: Sized {
    /// The operation's resource name (e.g. `operations/abc123`).
    fn name(&self) -> Option<&str>;

    /// Rebuilds this operation from a raw poll response body.
    ///
    /// Mirrors Python's `Operation.from_api_response` classmethod, which
    /// dispatches to the *type-specific* `_X_Operation_from_mldev`
    /// converter. That step is load-bearing, not cosmetic: the wire shape
    /// nests the payload under keys the Rust type doesn't name directly
    /// (e.g. a completed video operation arrives as
    /// `response.generateVideoResponse.generatedSamples[]`, which the
    /// converter remaps onto `response.generated_videos[]`), so
    /// deserializing the raw body would silently yield an operation with
    /// an empty result.
    ///
    /// # Errors
    /// Returns [`crate::Error::Json`] if `wire` doesn't match this
    /// operation's expected shape.
    fn from_api_response(wire: &Value) -> Result<Self>;
}

impl Operation for GenerateVideosOperation {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn from_api_response(wire: &Value) -> Result<Self> {
        let mldev = op_conv::generate_videos_operation_from_mldev(wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }
}

impl Operation for ImportFileOperation {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn from_api_response(wire: &Value) -> Result<Self> {
        let mldev = op_conv::import_file_operation_from_mldev(wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }
}

impl Operation for UploadToFileSearchStoreOperation {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn from_api_response(wire: &Value) -> Result<Self> {
        let mldev = op_conv::upload_to_file_search_store_operation_from_mldev(wire, None, None)?;
        Ok(serde_json::from_value(mldev)?)
    }
}

/// Whether `state` (a job-state wire string) is in `states`.
fn state_in(state: Option<&super::generated::JobState>, states: &[&str]) -> bool {
    state.is_some_and(|state| states.contains(&state.as_str()))
}

impl TuningJob {
    /// Whether the tuning job has ended. Mirrors Python's
    /// `TuningJob.has_ended` property.
    #[must_use]
    pub fn has_ended(&self) -> bool {
        state_in(self.state.as_ref(), JOB_STATES_ENDED)
    }

    /// Whether the tuning job has succeeded. Mirrors Python's
    /// `TuningJob.has_succeeded` property.
    #[must_use]
    pub fn has_succeeded(&self) -> bool {
        state_in(self.state.as_ref(), JOB_STATES_SUCCEEDED)
    }
}

impl BatchJob {
    /// Whether the batch job has ended (`false` when `state` is unset).
    /// Mirrors Python's `BatchJob.done` property.
    #[must_use]
    pub fn done(&self) -> bool {
        state_in(self.state.as_ref(), JOB_STATES_ENDED)
    }
}

impl From<PartMediaResolutionLevel> for PartMediaResolution {
    fn from(level: PartMediaResolutionLevel) -> Self {
        Self {
            level: Some(level),
            ..Default::default()
        }
    }
}

impl Part {
    /// Builds a [`Part`] carrying model-generated code to execute. Mirrors
    /// Python's `Part.from_executable_code`.
    #[must_use]
    pub fn from_executable_code(code: impl Into<String>, language: Language) -> Self {
        Self {
            executable_code: Some(ExecutableCode {
                code: Some(code.into()),
                language: Some(language),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Builds a [`Part`] carrying the result of executing code. Mirrors
    /// Python's `Part.from_code_execution_result`.
    #[must_use]
    pub fn from_code_execution_result(outcome: Outcome, output: impl Into<String>) -> Self {
        Self {
            code_execution_result: Some(CodeExecutionResult {
                outcome: Some(outcome),
                output: Some(output.into()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Sets the input media resolution. Covers the `media_resolution`
    /// argument of Python's `Part.from_uri` / `Part.from_bytes`, whose
    /// `str` / enum / object forms collapse into
    /// `impl Into<PartMediaResolution>` (a [`PartMediaResolutionLevel`] or a
    /// full [`PartMediaResolution`]).
    #[must_use]
    pub fn with_media_resolution(
        mut self,
        media_resolution: impl Into<PartMediaResolution>,
    ) -> Self {
        self.media_resolution = Some(media_resolution.into());
        self
    }
}

/// `JSONSchema.type`: either a single [`JSONSchemaType`] or a list of them
/// (JSON Schema allows both forms for a `"type"` keyword). Mirrors the
/// Python SDK's `Union[JSONSchemaType, list[JSONSchemaType]]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonSchemaTypeOrList {
    /// A single JSON Schema type.
    Single(JSONSchemaType),
    /// Multiple allowed JSON Schema types.
    Multiple(Vec<JSONSchemaType>),
}

impl From<JSONSchemaType> for JsonSchemaTypeOrList {
    fn from(value: JSONSchemaType) -> Self {
        Self::Single(value)
    }
}

impl From<Vec<JSONSchemaType>> for JsonSchemaTypeOrList {
    fn from(value: Vec<JSONSchemaType>) -> Self {
        Self::Multiple(value)
    }
}

/// Field names of `JSONSchema` in upstream declaration order. The order is
/// load-bearing: upstream's conversion walks a `model_dump()` in this order
/// (e.g. `type` is seen before `any_of`).
const JSON_SCHEMA_FIELDS: [&str; 24] = [
    "type",
    "format",
    "title",
    "description",
    "default",
    "items",
    "min_items",
    "max_items",
    "enum",
    "properties",
    "required",
    "min_properties",
    "max_properties",
    "minimum",
    "maximum",
    "min_length",
    "max_length",
    "pattern",
    "additional_properties",
    "any_of",
    "unique_items",
    "ref",
    "defs",
    "one_of",
];

const FIELD_TYPE: &str = "type";
const FIELD_ITEMS: &str = "items";
const FIELD_ANY_OF: &str = "any_of";
const FIELD_PROPERTIES: &str = "properties";
const FIELD_REF: &str = "ref";
const FIELD_DEFS: &str = "defs";
const FIELD_TITLE: &str = "title";
/// The JSON Schema spelling of `defs` / `ref` (`$defs` / `$ref`).
const JSON_SCHEMA_DEFS_KEYWORD: &str = "$defs";
const JSON_SCHEMA_NULL_TYPE: &str = "null";

/// The JSON Schema keyword for a `snake_case` `Schema` field, as the Gemini
/// API and the JSON Schema 2020-12 draft spell it.
fn json_schema_keyword(field: &str) -> &str {
    match field {
        "min_items" => "minItems",
        "max_items" => "maxItems",
        "min_properties" => "minProperties",
        "max_properties" => "maxProperties",
        "min_length" => "minLength",
        "max_length" => "maxLength",
        "additional_properties" => "additionalProperties",
        "any_of" => "anyOf",
        "ref" => "$ref",
        "defs" => JSON_SCHEMA_DEFS_KEYWORD,
        other => other,
    }
}

/// The `JSONSchema` fields that belong to each non-null type when a union
/// like `["string", "array"]` is split into `any_of` sub-schemas.
fn related_fields(json_type: &str) -> &'static [&'static str] {
    match json_type {
        "number" | "integer" => &[
            "description",
            "enum",
            "format",
            "maximum",
            "minimum",
            "title",
        ],
        "string" => &[
            "description",
            "enum",
            "format",
            "max_length",
            "min_length",
            "pattern",
            "title",
        ],
        "object" => &[
            "any_of",
            "description",
            "max_properties",
            "min_properties",
            "properties",
            "required",
            "title",
        ],
        "array" => &["description", "items", "max_items", "min_items", "title"],
        "boolean" => &["description", "title"],
        _ => &[],
    }
}

/// Splits a JSON Schema `type` value (a string or an array of strings) into
/// its non-null types and whether `"null"` was among them.
fn normalize_json_schema_type(value: Option<&Value>) -> (Vec<String>, bool) {
    let names: Vec<&str> = match value {
        Some(Value::String(name)) => vec![name.as_str()],
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    let nullable = names.contains(&JSON_SCHEMA_NULL_TYPE);
    let non_null = names
        .into_iter()
        .filter(|name| *name != JSON_SCHEMA_NULL_TYPE)
        .map(str::to_owned)
        .collect();
    (non_null, nullable)
}

/// Parses a JSON Schema value through [`JSONSchema`] (accepting both the
/// `snake_case` and the JSON Schema keyword spelling of every field) and
/// returns it as a `snake_case` object that holds only the fields that are set.
fn normalize_json_schema(value: &Value) -> Result<Map<String, Value>> {
    let parsed = JSONSchema::deserialize(value)?;
    match serde_json::to_value(parsed)? {
        Value::Object(map) => Ok(map),
        other => Err(Error::Validation(format!(
            "a JSON Schema must be an object, got {other}"
        ))),
    }
}

fn expect_object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| Error::Validation(format!("{what} must be a JSON object")))
}

/// Resolves a local `$ref` such as `#/$defs/Foo` against the root schema.
/// Like upstream, the resolved schema (and its direct properties) lose their
/// `title`.
fn resolve_ref(ref_path: &str, root: &Value) -> Result<Map<String, Value>> {
    let target = ref_path
        .trim_start_matches(['#', '/'])
        .split('/')
        .map(|part| {
            if part == JSON_SCHEMA_DEFS_KEYWORD {
                FIELD_DEFS
            } else {
                part
            }
        })
        .try_fold(root, |current, part| {
            current
                .get(part)
                .ok_or_else(|| Error::Validation(format!("cannot resolve $ref `{ref_path}`")))
        })?;
    let mut resolved = expect_object(target, "a resolved $ref target")?.clone();
    resolved.remove(FIELD_TITLE);
    resolved
        .get_mut(FIELD_PROPERTIES)
        .and_then(Value::as_object_mut)
        .into_iter()
        .flat_map(Map::values_mut)
        .filter_map(Value::as_object_mut)
        .for_each(|property| {
            property.remove(FIELD_TITLE);
        });
    Ok(resolved)
}

/// Copies a `JSONSchema` field that has a same-named `Schema` field.
fn set_plain_field(schema: &mut Schema, name: &str, value: &Value) {
    let text = || value.as_str().map(str::to_owned);
    let strings = || {
        value.as_array().map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map_or_else(|| item.to_string(), str::to_owned)
                })
                .collect::<Vec<_>>()
        })
    };
    match name {
        FIELD_REF => schema.r#ref = text(),
        "default" => schema.default = Some(value.clone()),
        "description" => schema.description = text(),
        "enum" => schema.r#enum = strings(),
        "format" => schema.format = text(),
        "max_items" => schema.max_items = value.as_i64(),
        "max_length" => schema.max_length = value.as_i64(),
        "max_properties" => schema.max_properties = value.as_i64(),
        "maximum" => schema.maximum = value.as_f64(),
        "min_items" => schema.min_items = value.as_i64(),
        "min_length" => schema.min_length = value.as_i64(),
        "min_properties" => schema.min_properties = value.as_i64(),
        "minimum" => schema.minimum = value.as_f64(),
        "pattern" => schema.pattern = text(),
        "required" => schema.required = strings(),
        FIELD_TITLE => schema.title = text(),
        // `unique_items`, `one_of` and `additional_properties` have no
        // (converted) counterpart on `Schema`.
        _ => {}
    }
}

/// Whether `schema` only says "null" (`{nullable: true}` or `{type: NULL}`).
fn is_null_marker(schema: &Schema) -> bool {
    let nullable_only = Schema {
        nullable: Some(true),
        ..Schema::default()
    };
    let null_type = Schema {
        r#type: Some(Type::Null),
        ..Schema::default()
    };
    *schema == nullable_only || *schema == null_type
}

/// Unwraps `any_of: [<null marker>, <schema>]` into `<schema>` with
/// `nullable = true`, carrying over the outer `default`.
fn unwrap_nullable_any_of(schema: Schema) -> Schema {
    let unwrapped = schema
        .any_of
        .as_deref()
        .filter(|parts| parts.len() == 2 && parts.iter().any(is_null_marker))
        .and_then(|parts| parts.iter().rev().find(|part| !is_null_marker(part)))
        .cloned();
    match unwrapped {
        Some(type_part) => Schema {
            nullable: Some(true),
            default: schema.default.or(type_part.default.clone()),
            ..type_part
        },
        None => schema,
    }
}

/// Splits a union-like schema (`["string", "array"]`) into an object whose
/// `any_of` holds one sub-schema per type, each carrying only the fields
/// related to that type. Everything else (including `default`) is dropped.
fn split_union_type(
    current: &Map<String, Value>,
    non_null_types: &[String],
) -> Result<Map<String, Value>> {
    tracing::warn!(
        "JSONSchema type is union-like, e.g. [\"null\", \"string\", \"array\"]. Converting it \
         into multiple sub-schemas, and copying them into the any_of field of the Schema. The \
         value of `default` field is ignored because it is ambiguous to tell which sub-schema it \
         belongs to."
    );
    let any_of = non_null_types
        .iter()
        .map(|json_type| {
            let sub_schema = related_fields(json_type)
                .iter()
                .filter_map(|field| {
                    current
                        .get(*field)
                        .map(|value| ((*field).to_owned(), value.clone()))
                })
                .chain([(FIELD_TYPE.to_owned(), Value::String(json_type.clone()))])
                .collect::<Map<String, Value>>();
            normalize_json_schema(&Value::Object(sub_schema)).map(Value::Object)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Map::from_iter([(
        FIELD_ANY_OF.to_owned(),
        Value::Array(any_of),
    )]))
}

fn convert_json_schema(
    dict: &Map<String, Value>,
    root: &Value,
    visited_refs: &mut HashSet<String>,
) -> Result<Schema> {
    let ref_path = dict
        .get(FIELD_REF)
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty());
    if let Some(path) = ref_path
        && !visited_refs.insert(path.to_owned())
    {
        return Ok(Schema::default());
    }
    let resolved = ref_path.map(|path| resolve_ref(path, root)).transpose()?;
    let current = resolved.as_ref().unwrap_or(dict);

    let mut schema = Schema::default();
    let (non_null_types, nullable) = normalize_json_schema_type(current.get(FIELD_TYPE));
    let is_union_like = non_null_types.len() > 1;
    let reformed = if is_union_like {
        schema.nullable = nullable.then_some(true);
        Some(split_union_type(current, &non_null_types)?)
    } else {
        None
    };
    let current = reformed.as_ref().unwrap_or(current);

    for name in JSON_SCHEMA_FIELDS {
        let Some(value) = current.get(name).filter(|value| !value.is_null()) else {
            continue;
        };
        match name {
            FIELD_DEFS => {}
            FIELD_ITEMS => {
                let items =
                    convert_json_schema(expect_object(value, "items")?, root, visited_refs)?;
                schema.items = Some(Box::new(items));
            }
            FIELD_ANY_OF => {
                let parts = value
                    .as_array()
                    .ok_or_else(|| Error::Validation("any_of must be an array".to_owned()))?
                    .iter()
                    .map(|part| {
                        convert_json_schema(expect_object(part, "any_of item")?, root, visited_refs)
                    })
                    .collect::<Result<Vec<_>>>()?;
                // Faithful to upstream: only an empty `any_of` makes an
                // untyped schema an OBJECT.
                if schema.r#type.is_none() && !is_union_like && parts.is_empty() {
                    schema.r#type = Some(Type::Object);
                }
                schema.any_of = Some(parts);
            }
            FIELD_PROPERTIES => {
                let properties = expect_object(value, "properties")?
                    .iter()
                    .map(|(key, property)| {
                        convert_json_schema(expect_object(property, key)?, root, visited_refs)
                            .map(|converted| (key.clone(), converted))
                    })
                    .collect::<Result<_>>()?;
                schema.properties = Some(properties);
            }
            FIELD_TYPE => {
                let (types, nullable) = normalize_json_schema_type(Some(value));
                schema.nullable = nullable.then_some(true).or(schema.nullable);
                if let Some(first) = types.into_iter().next() {
                    schema.r#type = Some(Type::from(first));
                }
            }
            other => set_plain_field(&mut schema, other, value),
        }
    }

    // An `items` that converted to nothing is dropped from an ARRAY.
    if schema.r#type == Some(Type::Array)
        && schema
            .items
            .as_deref()
            .is_some_and(|items| *items == Schema::default())
    {
        schema.items = None;
    }
    let schema = unwrap_nullable_any_of(schema);

    if let Some(path) = ref_path {
        visited_refs.remove(path);
    }
    Ok(schema)
}

/// Converts a [`Schema`] to a JSON Schema object (upstream's
/// `convert_schema`); `Schema` fields without a JSON Schema counterpart
/// (`example`, `property_ordering`) are dropped.
fn convert_schema(schema: &Schema) -> Map<String, Value> {
    let mut out = Map::new();
    let mut types: Vec<String> = Vec::new();
    let mut put = |field: &str, value: Option<Value>| {
        if let Some(value) = value {
            out.insert(json_schema_keyword(field).to_owned(), value);
        }
    };
    let schemas = |items: &[Schema]| Value::Array(items.iter().map(convert_schema_value).collect());
    let keyed = |map: &std::collections::HashMap<String, Schema>| {
        Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), convert_schema_value(value)))
                .collect(),
        )
    };
    let strings = |items: &[String]| json_strings(items);

    // Same order as `Schema`'s declaration: `nullable` is seen before `type`.
    put(
        "additional_properties",
        schema.additional_properties.clone(),
    );
    put("defs", schema.defs.as_ref().map(&keyed));
    put(FIELD_REF, schema.r#ref.clone().map(Value::String));
    put(FIELD_ANY_OF, schema.any_of.as_deref().map(schemas));
    put("default", schema.default.clone());
    put("description", schema.description.clone().map(Value::String));
    put("enum", schema.r#enum.as_deref().map(strings));
    put("format", schema.format.clone().map(Value::String));
    put(
        FIELD_ITEMS,
        schema.items.as_deref().map(convert_schema_value),
    );
    put("max_items", schema.max_items.map(Value::from));
    put("max_length", schema.max_length.map(Value::from));
    put("max_properties", schema.max_properties.map(Value::from));
    put("maximum", schema.maximum.map(Value::from));
    put("min_items", schema.min_items.map(Value::from));
    put("min_length", schema.min_length.map(Value::from));
    put("min_properties", schema.min_properties.map(Value::from));
    put("minimum", schema.minimum.map(Value::from));
    // Upstream adds NULL for any set `nullable`, including `false`.
    if schema.nullable.is_some() {
        types.push(JSON_SCHEMA_NULL_TYPE.to_owned());
    }
    put("pattern", schema.pattern.clone().map(Value::String));
    put(FIELD_PROPERTIES, schema.properties.as_ref().map(&keyed));
    put("required", schema.required.as_deref().map(strings));
    put(FIELD_TITLE, schema.title.clone().map(Value::String));
    if let Some(schema_type) = schema
        .r#type
        .as_ref()
        .filter(|schema_type| **schema_type != Type::TypeUnspecified)
    {
        types.push(schema_type.as_str().to_lowercase());
    }
    match types.len() {
        0 => {}
        1 => {
            out.insert(FIELD_TYPE.to_owned(), Value::String(types.remove(0)));
        }
        _ => {
            out.insert(FIELD_TYPE.to_owned(), json_strings(&types));
        }
    }
    out
}

fn convert_schema_value(schema: &Schema) -> Value {
    Value::Object(convert_schema(schema))
}

fn json_strings(items: &[String]) -> Value {
    Value::Array(items.iter().cloned().map(Value::String).collect())
}

impl Schema {
    /// Converts a JSON Schema (2020-12 draft, as used by `OpenAPI` 3.1) into a
    /// [`Schema`] for the Gemini Developer API.
    ///
    /// Mirrors Python's `Schema.from_json_schema(json_schema=...)` with its
    /// defaults (`api_option='GEMINI_API'`,
    /// `raise_error_on_unsupported_field=False`): unsupported keywords are
    /// ignored, union-like types (`["string", "array"]`) become `any_of`,
    /// `["string", "null"]` becomes `nullable`, and local `$ref`s are
    /// resolved (a reference cycle yields an empty schema). Both the JSON
    /// Schema keyword spelling (`anyOf`, `$ref`) and `snake_case` are accepted.
    ///
    /// `enum` values that are not strings are kept as their JSON text, since
    /// `Schema::enum` is a list of strings.
    ///
    /// Prefer passing JSON Schema directly through `response_json_schema` /
    /// `parameters_json_schema`; this conversion is lossy.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Json`] when `json_schema` is not a JSON Schema
    /// object, and [`Error::Validation`] when a `$ref` cannot be resolved.
    ///
    /// # Examples
    ///
    /// ```
    /// use gemini_genai::types::{Schema, Type};
    /// use serde_json::json;
    ///
    /// let schema = Schema::from_json_schema(&json!({"type": ["string", "null"]}))?;
    /// assert_eq!(schema.r#type, Some(Type::String));
    /// assert_eq!(schema.nullable, Some(true));
    /// # Ok::<(), gemini_genai::Error>(())
    /// ```
    pub fn from_json_schema(json_schema: &Value) -> Result<Self> {
        let root = Value::Object(normalize_json_schema(json_schema)?);
        let dict = expect_object(&root, "a JSON Schema")?;
        convert_json_schema(dict, &root, &mut HashSet::new())
    }

    /// Converts this [`Schema`] into a JSON Schema (2020-12 draft) value.
    ///
    /// Mirrors Python's `Schema.json_schema` property, but returns the
    /// JSON Schema in its keyword spelling (`anyOf`, `minItems`, ...) so it
    /// can be passed straight to `response_json_schema`. `Type` becomes the
    /// lower-case JSON Schema type, `nullable` adds `"null"` to `type`, and
    /// fields without a JSON Schema counterpart (`example`,
    /// `property_ordering`) are dropped. Unlike upstream, `defs` entries are
    /// converted like every other sub-schema instead of being copied raw.
    ///
    /// # Examples
    ///
    /// ```
    /// use gemini_genai::types::{Schema, Type};
    /// use serde_json::json;
    ///
    /// let schema = Schema {
    ///     r#type: Some(Type::String),
    ///     nullable: Some(true),
    ///     ..Schema::default()
    /// };
    /// assert_eq!(schema.json_schema(), json!({"type": ["null", "string"]}));
    /// ```
    #[must_use]
    pub fn json_schema(&self) -> Value {
        convert_schema_value(self)
    }
}

impl GenerateContentResponse {
    fn first_candidate_parts(&self) -> Option<&[Part]> {
        self.candidates
            .as_deref()
            .and_then(|candidates| candidates.first())
            .and_then(|candidate| candidate.content.as_ref())
            .and_then(|content| content.parts.as_deref())
    }

    /// The concatenated text of every non-thought text part in the first
    /// candidate, or `None` if there is no text. Mirrors Python's
    /// `GenerateContentResponse.text` property.
    #[must_use]
    pub fn text(&self) -> Option<String> {
        let parts = self.first_candidate_parts()?;
        let mut out = String::new();
        let mut found = false;
        for part in parts {
            if part.thought == Some(true) {
                continue;
            }
            if let Some(text) = &part.text {
                out.push_str(text);
                found = true;
            }
        }
        found.then_some(out)
    }

    /// Every part of the first candidate, if any. Mirrors Python's
    /// `GenerateContentResponse.parts` property.
    #[must_use]
    pub fn parts(&self) -> Option<&[Part]> {
        self.first_candidate_parts()
    }

    /// Every function call requested by the first candidate. Mirrors
    /// Python's `GenerateContentResponse.function_calls` property.
    #[must_use]
    pub fn function_calls(&self) -> Vec<&FunctionCall> {
        self.first_candidate_parts()
            .into_iter()
            .flatten()
            .filter_map(|part| part.function_call.as_ref())
            .collect()
    }

    /// The first candidate's executable-code part, if any. Mirrors
    /// Python's `GenerateContentResponse.executable_code` property.
    #[must_use]
    pub fn executable_code(&self) -> Option<&ExecutableCode> {
        self.first_candidate_parts()?
            .iter()
            .find_map(|part| part.executable_code.as_ref())
    }

    /// The first candidate's code-execution-result part, if any. Mirrors
    /// Python's `GenerateContentResponse.code_execution_result` property.
    #[must_use]
    pub fn code_execution_result(&self) -> Option<&CodeExecutionResult> {
        self.first_candidate_parts()?
            .iter()
            .find_map(|part| part.code_execution_result.as_ref())
    }
}

impl GenerateContentConfig {
    /// Sets `response_json_schema` from `T`'s [`schemars::JsonSchema`]
    /// derivation (via `schemars::schema_for!`), and defaults
    /// `response_mime_type` to `"application/json"` if it isn't already
    /// set. Convenience for structured output driven by a plain Rust
    /// type, mirroring the ergonomics of Python's `response_schema=SomeType`
    /// (which coerces a `pydantic.BaseModel`/`dataclass`/`Enum` via
    /// `model_json_schema()` in `t_schema`) -- this crate has no
    /// equivalent type-introspection path for its own
    /// [`crate::types::Schema`] (see `t_schema` in
    /// `crate::transformers`), so `response_json_schema` (passed through
    /// verbatim by `t_json_schema`) is the route for a caller who would
    /// rather derive a schema from a type than build one by hand.
    #[must_use]
    pub fn with_json_schema_of<T: schemars::JsonSchema>(mut self) -> Self {
        self.response_json_schema = Some(schemars::schema_for!(T).to_value());
        if self.response_mime_type.is_none() {
            self.response_mime_type = Some("application/json".to_owned());
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::generated::{Candidate, Content},
        *,
    };

    fn response_with_parts(parts: Vec<Part>) -> GenerateContentResponse {
        GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    parts: Some(parts),
                    role: Some("model".to_owned()),
                }),
                ..Default::default()
            }]),
            ..Default::default()
        }
    }

    #[test]
    fn text_concatenates_non_thought_text_parts() {
        let response = response_with_parts(vec![
            Part::from_text("Hello, "),
            Part {
                text: Some("(thinking)".to_owned()),
                thought: Some(true),
                ..Default::default()
            },
            Part::from_text("world!"),
        ]);
        assert_eq!(response.text().as_deref(), Some("Hello, world!"));
    }

    #[test]
    fn text_is_none_without_candidates() {
        assert_eq!(GenerateContentResponse::default().text(), None);
    }

    #[test]
    fn function_calls_collects_every_function_call_part() {
        let response = response_with_parts(vec![
            Part::from_function_call("a", std::collections::HashMap::new()),
            Part::from_text("not a call"),
            Part::from_function_call("b", std::collections::HashMap::new()),
        ]);
        let calls = response.function_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name.as_deref(), Some("a"));
        assert_eq!(calls[1].name.as_deref(), Some("b"));
    }

    #[expect(
        dead_code,
        reason = "fields exist only to drive schemars::JsonSchema derivation below (schema_for! inspects the type, not an instance); no test constructs or reads a value of this type"
    )]
    #[derive(schemars::JsonSchema)]
    struct Country {
        name: String,
        population: u64,
    }

    #[test]
    fn with_json_schema_of_sets_response_json_schema_and_defaults_mime_type() {
        let config = GenerateContentConfig::default().with_json_schema_of::<Country>();
        assert_eq!(
            config.response_mime_type.as_deref(),
            Some("application/json")
        );
        let schema = config.response_json_schema.unwrap();
        assert_eq!(schema["properties"]["name"]["type"], "string");
        assert_eq!(schema["properties"]["population"]["type"], "integer");
    }

    #[test]
    fn with_json_schema_of_does_not_override_an_explicit_mime_type() {
        let config = GenerateContentConfig {
            response_mime_type: Some("text/x.enum".to_owned()),
            ..Default::default()
        }
        .with_json_schema_of::<Country>();
        assert_eq!(config.response_mime_type.as_deref(), Some("text/x.enum"));
    }
}
