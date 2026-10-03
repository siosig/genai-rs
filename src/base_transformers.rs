//! Transformers shared with the live/tokens converters.
//!
//! Mirrors Python's `_base_transformers.py`. Like [`crate::transformers`],
//! this is the Gemini Developer API (`mldev`) port of the upstream
//! function.

#![expect(
    clippy::missing_errors_doc,
    reason = "`t_bytes` shares Python's uniform `fn(Value) -> Result<Value>` transformer shape, even though it has no failure path to document"
)]

use serde_json::Value;

use crate::errors::Result;

/// Re-encodes a byte field with the standard base64 alphabet. Python's
/// `t_bytes` runs `base64.b64encode` on raw bytes (so converter-built fields
/// such as `bytesBase64Encoded` use the standard alphabet), whereas the rest
/// of the request body is URL-safe encoded by `encode_unserializable_types`.
/// By the time a value reaches the converter layer this crate's generated
/// types have already serialized `Vec<u8>` fields to a (URL-safe) base64
/// string, so the standard form is the same text with two characters
/// swapped back. Non-string values pass through, like Python's
/// already-not-`bytes` fallback.
pub fn t_bytes(value: Value) -> Result<Value> {
    Ok(match value {
        Value::String(encoded) => Value::String(encoded.replace('-', "+").replace('_', "/")),
        other => other,
    })
}
