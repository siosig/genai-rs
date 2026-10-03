//! Integration tests for the Interactions client facade, ported from
//! `google/genai/tests/interactions/`: URL paths, auth headers, retries,
//! timeouts and request/response serialization, against a wiremock server.
//!
//! `test_httpx_compat.py` has no Rust counterpart: every test there exercises
//! Python's httpx object model (see `tools/codegen/upstream_tests.toml`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::default_trait_access,
    clippy::too_many_lines,
    clippy::redundant_closure_for_method_calls,
    clippy::needless_pass_by_value,
    reason = "ported upstream tests: a failed setup or assertion should panic, and each test mirrors one long Python function and its `Default` arguments"
)]

#[path = "../common/mod.rs"]
mod common;
mod normalize;
mod recording;
mod test_auth;
mod test_integration;
mod test_paths;
