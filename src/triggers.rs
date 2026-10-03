//! Trigger resources (`client.triggers()`): create, get, list, update, delete and run triggers, and list their executions. Mirrors Python's `triggers.py`.
//!
//! The items are generated into `crate::gaos` (see `tools/codegen/gen_gaos.py`) and
//! re-exported here under the names the upstream module exports.

pub use crate::gaos::exports::triggers::*;

use serde_json::Value;

use crate::errors::Result;

/// Normalizes the `interaction` of a `Triggers.create` request body like
/// [`crate::interactions::normalize_create_body`] (Python's `triggers.create`
/// applies `_normalize_create_body` to `interaction` when it is a dict). A body
/// without an object `interaction` is returned unchanged.
///
/// # Errors
/// Returns [`crate::errors::Error::Validation`] if the `interaction` has an unexpected key.
pub fn normalize_trigger_create_body(mut body: Value) -> Result<Value> {
    if let Some(interaction) = body.get_mut("interaction")
        && interaction.is_object()
    {
        *interaction = crate::interactions::normalize_create_body(interaction.take())?;
    }
    Ok(body)
}
