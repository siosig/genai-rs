# Changelog

## Table of Contents

- [Overview](#overview)
- [Unreleased](#unreleased)
- [0.1.0](#010)

## Overview

Notable changes to `gemini-genai`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Generated types are deliberately not `#[non_exhaustive]`, so you can build them
with struct literals plus `..Default::default()`. In exchange, **adding a field to
a generated type is treated as a minor-version change**, matching the upstream
Python SDK's own policy. Always finish a struct literal with
`..Default::default()`.

## Unreleased

### Changed

- **Renamed the package and library.** The package is now `gemini-genai` and
  the library is `gemini_genai`; update dependency declarations and change
  `use google_genai::...` to `use gemini_genai::...`. The vendor name was
  dropped because Apache-2.0 grants no trademark rights (Section 6) and the
  old library name matched the upstream Python import path `google.genai`
  exactly, which read as an official Google product. `gemini-genai-rs` was not
  available on crates.io, hence `gemini-genai`.
- **Changed the client-identification header value.** Requests now send
  `gemini-genai/<version> gl-rust/<version>` in `x-goog-api-client` and
  `user-agent` instead of `google-genai-sdk/<version>`, which is the label the
  official Python SDK uses. Header *names* are unchanged. This affects only
  what the server records about the client; no request or response body
  changes.
- **Pinned the build.** `Cargo.lock` is now committed and CI runs with
  `--locked`, `rust-toolchain.toml` names an exact version instead of tracking
  `stable`, the code generator's Python dependencies are locked with hashes,
  and every CI action is pinned to a commit SHA. Dependabot proposes the
  updates that pinning would otherwise prevent.

### Security

- Update `serde_with` 3.17 → 3.21 (GHSA-7gcf-g7xr-8hxj, `KeyValueMap` panic on
  empty input) and `time` 0.3.45 → 0.3.47+ (CVE-2026-25727, stack exhaustion).
  Neither is reachable through this crate -- `KeyValueMap` is not used, and
  `time` is present in `Cargo.lock` only as an unactivated optional dependency
  -- but both are flagged by Dependabot and there is no reason to sit on them.

### Changed

- **MSRV 1.85 → 1.88.** Both security fixes above, and `rmcp` behind the `mcp`
  feature, require 1.88; an MSRV that blocks security patches is not worth
  keeping, and 1.88 is over a year old. `rust-version` now covers every feature
  combination, and the `msrv` CI job checks `--all-features`.

### Fixed

- **`codegen-check` can actually run now.** `gen_parity.py` read its input from
  `specs/`, which is git-ignored, so the job had never once succeeded on a clean
  checkout -- it failed with `FileNotFoundError` before reaching `git diff`. The
  parity matrix now lives at `tools/codegen/parity-matrix.md`, beside the
  generator that consumes it.
- **Pinned the interpreter the generated code is produced with.**
  `google.genai.types` exposes a different set of pydantic models per Python
  version (3.12: 464 including `BlobImageUnion`; 3.14: 463 without it), so the
  committed output depended on whoever ran the generator last.
  `tools/codegen/upstream.py` now refuses to generate on anything but the pinned
  3.12, the same way it already refused a mismatched SDK version. Regenerating
  under 3.12 adds the `BlobImageUnion` type; converters and fixtures are
  unchanged, so no request or response body changes.

- **`--no-default-features` now compiles.** `tokio`'s `io-util` feature, which
  `src/http/upload.rs` needs for `AsyncReadExt`, was missing from `Cargo.toml`.
  The crate only ever built because the default `live` feature pulls in
  tokio-tungstenite, which enabled it as a side effect — Cargo features are
  additive across the whole graph, so a crate that compiles only because a
  sibling dependency turned on a feature is one dependency change away from
  breaking. A CI matrix now builds all eight selectable feature combinations.

### Added

- `NOTICE` at the repository root, plus SPDX attribution headers on every
  generated file, recording that the generated types, converters, blocking
  wrappers, parity table and converter fixtures are derived from
  `google-genai` 2.19.0 (Copyright 2025 Google LLC, Apache-2.0) and what was
  changed. Required by Sections 4(b) and 4(c) of the Apache License; the
  repository previously carried no upstream copyright notice at all.
- `SECURITY.md`, `CONTRIBUTING.md`, issue templates, and `hooks/` — the
  commit-time secret and language gates, previously local-only, are now in the
  repository and enabled with `git config core.hooksPath hooks`.
- `tests/attribution.rs`, `tests/protected_identifiers.rs` and
  `tests/supply_chain.rs`, which fail if the attribution is lost, if a future
  rename touches an upstream-owned identifier (`GOOGLE_API_KEY`,
  `x-goog-api-key`, `googleSearch`, the `google.ai.generativelanguage.*`
  service path, or the upstream package name `google-genai` itself), or if an
  action, download or lockfile stops being pinned.

## 0.1.0

Initial release. A Rust port of the Google Gen AI Python SDK
([`google-genai` 2.19.0](https://github.com/googleapis/python-genai)) targeting
the **Gemini Developer API**.

### Added

Async-first client (`gemini_genai::Client`) with one accessor per Python SDK
module:

| Module | Covered |
|---|---|
| `models` | `generate_content` (incl. automatic function calling), `generate_content_stream`, `embed_content`, `count_tokens`, `get`, `list`, `update`, `delete`, `generate_videos`, `generate_images` (deprecated upstream) |
| `chats` | `create`, `send_message`, `send_message_stream`, `get_history` (curated and comprehensive) |
| `files` | `upload` (resumable protocol), `get`, `list`, `delete`, `download`, `register_files` |
| `caches` | `create`, `get`, `list`, `update`, `delete` |
| `tunings` | `tune`, `get`, `cancel` |
| `batches` | `create`, `create_embeddings`, `get`, `list`, `cancel`, `delete` |
| `operations` | `get` (generic over the operation type, e.g. `GenerateVideosOperation`) |
| `file_search_stores` | `create`, `get`, `list`, `delete`, `import_file`, `upload_to_file_search_store`, `download_media`, and nested `documents()`: `get`, `list`, `delete` |
| `auth_tokens` | `create` (ephemeral Live API tokens) |
| `live` | `connect` + `LiveSession` (`send_client_content`, `send_realtime_input`, `send_tool_response`, `receive`, `close`, `setup_complete`), and `live().music()` (`connect`, `set_weighted_prompts`, `set_music_generation_config`, `play`, `pause`, `stop`, `reset_context`, `receive`, `close`) |

Supporting surface:

- **Types** — 411 structs and 79 enums generated from `google.genai.types`, same
  names as Python, `snake_case` fields. Generated enums carry an
  `Unknown(String)` variant so server-added values still deserialize.
- **Content ergonomics** — `impl Into<Contents>` accepts `&str`, `String`,
  `Part`, `Vec<Part>`, `Content`, `Vec<Content>`; `Part::from_text` /
  `from_bytes` / `from_uri` / `from_function_call` / `from_function_response`,
  plus `Part::from_file_bytes` (reads a path, infers the MIME type).
- **Structured output** — `GenerateContentConfig::with_json_schema_of::<T>()`
  derives the response schema from any `schemars::JsonSchema` type.
- **Automatic function calling** — `afc::function_tool` wraps an async Rust
  function as a model-callable tool; `Tool::from_function` declares it;
  `generate_content` drives the loop, honouring
  `AutomaticFunctionCallingConfig` (`disable`, `maximum_remote_calls` — default
  10, `ignore_call_history`).
- **Pagination** — `Pager<T>` with `page()`, `name()`, `page_size()`, `config()`,
  `next_page().await`, and `into_stream()`.
- **Errors** — a `thiserror` `Error` enum: `Api` (boxed `ApiError` with `code` /
  `status` / `message` / `details` / `response_headers`), `Http`, `Json`, `Io`,
  `Validation`, `UnsupportedByBackend`, `UnsupportedBackend`, `FunctionCall`,
  `WebSocket`, `Stream`, `Upload`, `NoMorePages`, `BlockingInsideRuntime`.
- **HTTP configuration** — `HttpOptions` (base URL, API version, headers,
  timeout, extra body) and `HttpRetryOptions` (off by default, matching Python).

Cargo features:

| Feature | Default | Enables |
|---|---|---|
| `rustls-tls` | ✅ | TLS via `rustls` + `webpki-roots` |
| `native-tls` | — | TLS via the platform's native stack |
| `live` | ✅ | the bidirectional realtime (WebSocket) API |
| `blocking` | — | `gemini_genai::blocking`: a synchronous mirror of the API (minus Live), streams as `Iterator`, its own current-thread runtime |
| `mcp` | — | `gemini_genai::mcp::mcp_tools`: bridges an MCP server's tools into automatic function calling |

### Known limitations

- **The AFC registry is process-wide.** `Tool::from_function` stores callables in
  a global map keyed by the declared function name, because `Tool` is a plain
  serializable struct and `generate_content`'s Python-mirroring signature has no
  side channel for a per-call function map. Two different callables registered
  under one name collide — the later wins for both. Python rebuilds its
  `function_map` per call and has no such coupling. Use unique names, and prefer
  registering tools once at startup.
- **`Chat::record_history` is private.** History is maintained by `send_message`
  / `send_message_stream`; Python exposes the method publicly.
- **Chat history omits AFC's intermediate turns.** `Chat::send_message`
  delegates to `models().generate_content`, so automatic function calling runs
  there and only the final, post-tool-call response is recorded. Python instead
  disables `generate_content`'s AFC and drives the loop inside `chats.py`,
  recording each intermediate `functionCall` / `functionResponse` turn. After a
  tool round-trip the two SDKs' `get_history()` therefore differ; the response's
  `automatic_function_calling_history` still carries the full exchange.
- **`Client::http_options()` is not exposed.** The contract reserved a public
  accessor; 0.1.0 keeps the effective options internal.
- **`models().generate_videos` takes one `GenerateVideosSource`** instead of
  Python's four mutually exclusive keyword arguments.
- `ChatStream` only records the model's turn once fully drained — abandoning it
  half-way leaves that turn out of the history.

### Not implemented (deferred)

| Surface | Status | Reason |
|---|---|---|
| Vertex AI backend (auth, endpoints, `_to_vertex` converters) | deferred | Out of scope for this release. `vertexai(true)`, `project`, `location`, or `GOOGLE_GENAI_USE_VERTEXAI=1` returns `Error::UnsupportedBackend`. Vertex-only *types* are still generated; Vertex-only *fields* on a request return `Error::UnsupportedByBackend`. |
| `models.compute_tokens` | present, always errors | Vertex-only upstream; returns `Error::UnsupportedBackend("models.compute_tokens")`. |
| `tunings.list` | present, always errors | Vertex-only upstream, and no `_to_mldev` converter exists to build a faithful request; returns `Error::UnsupportedByBackend`. |
| `models.edit_image` / `upscale_image` / `recontext_image` / `segment_image` | absent | Python raises `ValueError` for these outside Vertex AI. |
| `tunings.validate_reward` | absent | Same Vertex-only guard upstream. |
| `local_tokenizer` | deferred | Needs a `sentencepiece` binding. |
| NextGen modules (`interactions`, `agents`, `webhooks`, `triggers`, `environments`) | deferred | A separate, independently generated preview SDK upstream with its own error model. |
| replay harness / `DebugConfig` | not ported | This crate tests with golden JSON fixtures plus `wiremock` instead. |
| `live.AsyncSession.send` / `start_stream` | not ported | Deprecated upstream; `send_client_content` / `send_realtime_input` replace them. |
| `tunings.display_experiment_button` / `display_model_tuning_button` | N/A | IPython-only. |
| `httpx_*` / `aiohttp_client` / `client_args` on `HttpOptions` | N/A | Python-runtime-specific. |

See [`docs/parity.md`](docs/parity.md) for the generated, method-by-method
mapping, and [`docs/migrating-from-python.md`](docs/migrating-from-python.md) for
the migration guide.
