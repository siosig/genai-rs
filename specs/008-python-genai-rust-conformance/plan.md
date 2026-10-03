# Implementation Plan: Python google-genai Conformance (Gemini Developer API)

**Branch**: `008-python-genai-rust-conformance` | **Date**: 2026-10-03 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/008-python-genai-rust-conformance/spec.md`

## Summary

Bring `gemini-genai` to the latest upstream `google-genai` (2.28.0; current pin 2.23.0) for the Gemini Developer API only, and make future upstream syncs cheap for an AI assistant. Four mechanisms:

1. **Structure mirroring** — one Rust file per upstream module, same function/type names (Rust casing) and same definition order; renames recorded in a migration table (breaking release 0.4.0). See [contracts/module-map.md](contracts/module-map.md).
2. **Symbol ledger + sync diff** — a generated, checked-in ledger of every upstream symbol (normalized-AST fingerprint, Rust counterpart, status). A single command prints only changed/added/removed symbols paired with their Rust counterparts. See [contracts/ledger.md](contracts/ledger.md), [contracts/sync-report.md](contracts/sync-report.md).
3. **Upstream-test coverage** — every upstream test (167 files / 1378 functions / 327 table items at 2.28.0) is inventoried and is `mapped` to a Rust test, or `excluded` with a coded reason; table-driven tests are executed through a Python-SDK-generated oracle corpus. See [contracts/upstream-tests.md](contracts/upstream-tests.md).
4. **New 2.24–2.28 surface** — agents, environments, triggers, webhooks, voices, credentials, interactions, all backed by upstream's Speakeasy-generated `_gaos` sub-SDK, generated into `src/gaos/` from its pydantic models and resource AST.

Vertex AI is out of scope everywhere (each Vertex symbol/test is recorded `excluded`/`out_of_scope` with reason code `vertex`).

## Technical Context

**Language/Version**: Rust (edition/toolchain from `rust-toolchain.toml`, unchanged); Python 3.12 (uv-managed) for codegen tooling only.

**Primary Dependencies**: existing crate deps (`reqwest`, `serde`, `serde_json`, `thiserror`, `tokio`, `tracing`, `tokio-tungstenite`, `wiremock` in dev). New dev-only: none planned beyond what `Cargo.toml` already has; `rstest` only if already present. Python: `google-genai==2.28.0`, `pydantic`, `httpx` (transitive), stdlib `ast`/`hashlib`/`tomllib` (3.12 has `tomllib`; writing TOML uses a tiny in-repo emitter, no new dep).

**Storage**: checked-in generated files only (`tools/codegen/ledger.toml`, `tools/codegen/upstream_tests.toml`, `tests/fixtures/upstream/**.json`, `src/gaos/**`, `src/converters/generated/**`).

**Testing**: `cargo test --workspace --all-features` (offline; `wiremock` + existing `tests/common/ws_server.rs`); live tests stay behind the existing opt-in (`tests/e2e*.rs`, env-gated). Python checks: `tools/codegen/check_*.py` run in CI `codegen-check`.

**Target Platform**: library crate, Linux/macOS/Windows (unchanged).

**Project Type**: single Rust library crate + Python codegen tooling.

**Performance Goals**: none beyond not regressing (no hot-path changes; measured claims only per rust.md §6).

**Constraints**: generated files are never hand-edited (AGENTS.md); `clippy::unwrap_used`/`expect_used` deny; `missing_docs` deny; no secret/body logging; no `anyhow`.

**Scale/Scope**: 62k lines of upstream Python in `google/genai` (+30k `_gaos`); 11.7k lines current Rust; ~1.4k upstream test functions.

## Constitution Check

`.specify/memory/constitution.md` is the unfilled template (no ratified principles). `AGENTS.md` and the Rust best-practice rules (Apollo GraphQL handbook based) act as the governing rules. Gate = they are satisfied or deviations are recorded:

| Rule | Status |
|------|--------|
| Generated code not hand-edited | Satisfied; all new Rust under `src/gaos/**` is generated |
| No unwrap/expect/panic in production | Satisfied; existing 7 recorded deviations unchanged; new generated code returns `Result` |
| thiserror, no anyhow | Satisfied |
| Upstream mirroring vs rust.md | Rule 1 wins; deviations listed in `tools/codegen/deviations.toml` (see research R-08) |
| Post-design re-check | Pass (no violations needing Complexity Tracking) |

## Project Structure

### Documentation (this feature)

```text
specs/008-python-genai-rust-conformance/
├── spec.md  plan.md  research.md  data-model.md  quickstart.md  tasks.md
├── checklists/requirements.md
└── contracts/{module-map.md, ledger.md, sync-report.md, upstream-tests.md, gaos-generation.md}
```

### Source Code (repository root) — target layout after this feature

```text
src/
├── lib.rs                      # __init__.py (+ version const)
├── client.rs                   # client.py
├── base_url.rs                 # _base_url.py
├── api_client/                 # _api_client.py  (was src/http/)
│   ├── mod.rs  upload.rs  retry.rs  sse.rs  headers.rs
├── common.rs                   # _common.py (getv/setv/…; was in converters/mod.rs)
├── base_transformers.rs        # _base_transformers.py
├── transformers.rs             # _transformers.py
├── extra_utils.rs              # _extra_utils.py (was part of afc.rs)
├── automatic_function_calling_util.rs   # _automatic_function_calling_util.py (was part of afc.rs)
├── mcp_utils.rs                # _mcp_utils.py (was mcp.rs)
├── errors.rs                   # errors.py (was error.rs)
├── pagers.rs                   # pagers.rs (was pager.rs)
├── models.rs chats.rs files.rs caches.rs tunings.rs batches.rs operations.rs
├── file_search_stores.rs documents.rs
├── tokens.rs                   # tokens.py (was auth_tokens.rs; accessor stays client.auth_tokens())
├── live.rs live_music.rs       # live.py, live_music.py (were live/mod.rs, live/music.rs)
├── agents.rs environments.rs triggers.rs webhooks.rs voices.rs credentials.rs interactions.rs   # NEW (public re-exports + client handles)
├── gaos/                       # NEW, GENERATED from upstream _gaos (types.rs, resources/*.rs, mod.rs)
├── types/                      # types.py (generated + ext.rs, http.rs, conversions.rs unchanged)
├── converters/generated/       # per-module converters (unchanged location, 2.28.0 content)
└── blocking/                   # unchanged location; generated.rs extended by methods.toml

tests/
├── <pymodule>/main.rs + <test_file_stem>.rs   # mirrors google/genai/tests/<pymodule>/test_<stem>.py
├── upstream_table/main.rs + dispatch.rs       # oracle-corpus runner for table-driven upstream tests
├── fixtures/upstream/<pymodule>/<stem>.json   # GENERATED oracle corpus
└── common/ (existing helpers)

tools/codegen/
├── upstream.py                 # PINNED_VERSION = "2.28.0"
├── upstream_src.py             # NEW: fetch/verify upstream git checkout at the pinned tag
├── symbols.py  gen_ledger.py  sync_diff.py  ledger.toml  module_map.toml  deviations.toml   # NEW
├── upstream_tests.py  upstream_tests.toml  upstream_tests_rules.toml  check_upstream_tests.py   # NEW
├── gen_upstream_cases.py  upstream_cases_responses.toml                                       # NEW
└── gen_gaos.py  gaos_overrides.toml                                                           # NEW
```

**Structure Decision**: single crate; flat file-per-upstream-module under `src/`; generated sub-tree `src/gaos/` mirrors `_gaos`; tests mirror upstream test directories. Details in `contracts/module-map.md`.

## Phases (each ends with all gates green; later phases may stop without leaving a broken tree)

| Phase | Content | Gate |
|-------|---------|------|
| P0 Setup & 2.28.0 sync | codegen venv (uv, Py 3.12), re-pin to 2.28.0, regenerate, make crate compile and existing tests pass | G (fmt, clippy, test, doc, `generate.py && git diff --exit-code`) |
| P1 Structure alignment | file/module renames per module-map, split `afc.rs`/`converters/mod.rs`, migration table skeleton | G + `tests/protected_identifiers.rs` passes |
| P2 Ledger + sync diff | `symbols.py`, `gen_ledger.py`, `sync_diff.py`, module map/deviations, CI job | G + ledger check (0 `unmapped` not in out-of-scope) |
| P3 Upstream-test inventory | `upstream_tests.py`, rules, `check_upstream_tests.py`; all 1378 functions classified (`pending` allowed) | G + inventory check |
| P4 Oracle corpus + table runner | `gen_upstream_cases.py`, `tests/upstream_table`, dispatch for every `test_method` | G + all non-excluded table items pass |
| P5 Plain-test porting | per upstream test directory, in parallel; marker comments; inventory flips to `mapped`/`excluded` | G + `pending == 0` for ported dirs |
| P6 `_gaos` surface | `gen_gaos.py`, `src/gaos/`, client accessors, blocking wrappers, 7 public modules, ported gaos/interactions tests | G |
| P7 Parity, docs, release | parity doc regen, `upstream-sync*.md`, CONTRIBUTING sync, CHANGELOG migration table, version 0.4.0, final audit (SC-001..007) | G + all checks |

## Complexity Tracking

No constitution violations. Recorded deviations live in `tools/codegen/deviations.toml` (R-08).
