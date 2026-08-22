# Changelog

## Table of Contents

- [Overview](#overview)
- [0.1.0](#010)

## Overview

Notable changes to `google-genai-rs`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Generated types are deliberately not `#[non_exhaustive]`, so you can build them
with struct literals plus `..Default::default()`. In exchange, **adding a field to
a generated type is treated as a minor-version change**, matching the upstream
Python SDK's own policy. Always finish a struct literal with
`..Default::default()`.

## 0.1.0

Initial release. A Rust port of the Google Gen AI Python SDK
([`google-genai` 2.19.0](https://github.com/googleapis/python-genai)) targeting
the **Gemini Developer API**.

### Added

Async-first client (`google_genai::Client`) with one accessor per Python SDK
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
| `blocking` | — | `google_genai::blocking`: a synchronous mirror of the API (minus Live), streams as `Iterator`, its own current-thread runtime |
| `mcp` | — | `google_genai::mcp::mcp_tools`: bridges an MCP server's tools into automatic function calling |

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
