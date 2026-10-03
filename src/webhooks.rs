//! Webhook resources (`client.webhooks()`): create, get, list, update, delete, ping webhooks and rotate their signing secret. Mirrors Python's `webhooks.py`.
//!
//! The items are generated into `crate::gaos` (see `tools/codegen/gen_gaos.py`) and
//! re-exported here under the names the upstream module exports.

pub use crate::gaos::exports::webhooks::*;
