//! Request/response types for the Gemini Developer API.
//!
//! Most types here are **generated** from the Python SDK's `types.py` by
//! `tools/codegen/gen_types.py` (see `generated/`) and re-exported from
//! this module. `http` (this module's sibling) and small helpers in `ext`
//! and `conversions` are hand-written. See `AGENTS.md` for the
//! do-not-hand-edit policy on `generated/`.

mod conversions;
mod ext;
pub mod generated;
mod http;

pub use conversions::Contents;
pub use ext::JsonSchemaTypeOrList;
pub use generated::*;
pub use http::{HttpOptions, HttpResponse, HttpRetryOptions};
