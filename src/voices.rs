//! Voice resources (`client.voices()`): create, get, list and delete custom voices. Mirrors Python's `voices.py`.
//!
//! The items are generated into `crate::gaos` (see `tools/codegen/gen_gaos.py`) and
//! re-exported here under the names the upstream module exports.

pub use crate::gaos::exports::voices::*;
