//! Agent resources (`client.agents()`): create, get, list and delete agents. Mirrors Python's `agents.py`, a re-export layer over the generated `_gaos` sub-SDK.
//!
//! The items are generated into `crate::gaos` (see `tools/codegen/gen_gaos.py`) and
//! re-exported here under the names the upstream module exports.

pub use crate::gaos::exports::agents::*;
