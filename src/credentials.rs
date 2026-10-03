//! Credential resources (`client.credentials()`, experimental upstream): create, get, list, update and delete credentials. Mirrors Python's `credentials.py`.
//!
//! The items are generated into `crate::gaos` (see `tools/codegen/gen_gaos.py`) and
//! re-exported here under the names the upstream module exports.

pub use crate::gaos::exports::credentials::*;
