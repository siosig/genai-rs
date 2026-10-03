//! Ports of `google/genai/tests/transformers/` (the `_transformers.py` unit
//! tests). The Rust transformers work on `serde_json::Value`, so Python
//! pydantic objects (`types.Part(text=...)`) become the JSON they serialize
//! to; the functions are reached through `__test_support`.

#[path = "../common/mod.rs"]
mod common;

// One module per upstream test file.
mod blobs;
mod bytes;
mod function_responses;
mod schema;
mod t_batch;
mod t_content;
mod t_contents;
mod t_part;
mod t_parts;
mod t_tool;
mod t_tools;
