//! Helpers used by the generated `mldev` request/response converters
//! (`generated/`): the converter dispatcher and the crate-internal error and
//! object-shape helpers. The `getv`/`setv` path accessors live in
//! `crate::common` (mirroring Python's `_common.py`).

pub mod generated;

use serde_json::{Map, Value};

use crate::errors::{Error, Result};

/// Invokes a generated `_X_to_mldev`/`_X_from_mldev` converter by its
/// Python name (e.g. `"_GenerateContentParameters_to_mldev"`), calling it
/// as `(input, None, None)`. This is a thin `pub` re-export of
/// [`generated::dispatch`] solely so the golden-fixture converter test
/// suite (`tests/converters_golden.rs`, a separate integration-test crate
/// that can only see `pub` items) can reach it; it is not part of this
/// crate's public API.
///
/// # Errors
/// Returns whatever the underlying converter returns, including
/// [`Error::UnsupportedByBackend`] for Vertex-AI-only fields, or
/// [`Error::Validation`] if `name` does not match a known converter.
#[doc(hidden)]
pub fn dispatch_converter(name: &str, input: &Value) -> Result<Value> {
    generated::dispatch(name, input)
}

/// Narrows a generated `_to_mldev` converter's `Value` return into a `&mut
/// Map`. Every such converter unconditionally returns `Value::Object(...)`
/// (see `tools/codegen/gen_converters.py`'s `to_object` template) -- a
/// true crate-internal invariant, not a caller mistake -- so this is the
/// single, documented place that invariant is asserted, rather than
/// repeating `.as_object_mut().expect(...)` at every call site.
#[expect(
    clippy::expect_used,
    reason = "documented invariant: every generated `_to_mldev` converter returns Value::Object"
)]
pub(crate) fn as_object_mut(value: &mut Value) -> &mut Map<String, Value> {
    value
        .as_object_mut()
        .expect("converters always return an Object")
}

/// Builds the [`Error::UnsupportedByBackend`] a generated converter raises
/// when a Vertex-AI-only field is set on a Gemini Developer API client.
pub(crate) fn vertex_only_error(field: &'static str) -> Error {
    Error::UnsupportedByBackend {
        field,
        backend: crate::errors::Backend::VertexAi,
    }
}
