# Research & Decisions

All decisions are final; the implementer makes no further design choices. Facts were gathered 2026-10-03 from PyPI, tag `v2.28.0` of `googleapis/python-genai`, the `2.23.0` wheel, and the current tree.

## Facts

- PyPI latest `google-genai` = **2.28.0** (tags v2.24.0 … v2.28.0 exist). Pin today = 2.23.0. `Cargo.toml` version = 0.3.1.
- 2.23.0 → 2.28.0 changes in `google/genai/` (excluding tests): modified `__init__ _api_client _common _live_converters _tokens_converters batches caches chats client models tunings types version`; **new** `agents environments triggers webhooks voices credentials`; `_gaos/` (Speakeasy sub-SDK: 344 files / ~30k lines; 38 resource files, 40 request-model files, 211 type files) changed in 179 paths. (`interactions.py` and `_gaos` already existed at 2.23.0; the crate has no interactions resource.)
- Upstream tests live only in the git repo (`google/genai/tests`, 203 `.py`, 1 `.json`, media files); the wheel has none. **Replay recordings are not published** (`GOOGLE_GENAI_REPLAYS_DIRECTORY` is external). 167 `test_*.py` files, 1378 `test_*` functions, 327 `TestTableItem` instances (AST count; `scratchpad/inv.py` logic is re-implemented in `tools/codegen/upstream_tests.py`).
- `pytest_helper.TestTableItem` fields that carry backend semantics: `exception_if_mldev`, `exception_if_vertex`, `skip_in_private`, `override_replay_id`, `has_union` (name must start `test_union_`). Each table item runs for `use_vertex ∈ {True, False}`; the `False` run is the in-scope one.
- `HttpOptions.httpx_client` exists, so the Python SDK can be pointed at an in-process capturing transport.
- `tools/codegen` already generates types, converters, blocking wrappers, converter golden fixtures and the parity doc; Python 3.12 is mandatory for generation (`PINNED_PYTHON`); local interpreter is 3.14, uv has 3.12 (installed).

## R-01 Target version
**Decision**: pin 2.28.0, re-pinned once now, never chased mid-feature. **Rationale**: it is the latest release; spec says latest at planning time. **Alternative**: stay at 2.23.0 — rejected (violates "latest").

## R-02 Codegen environment
**Decision**: `uv venv --python 3.12 target/codegen-venv`; `uv pip install --python target/codegen-venv/bin/python --require-hashes -r tools/codegen/requirements.txt`; every codegen command is run as `target/codegen-venv/bin/python tools/codegen/<script>.py`. `requirements.in`/`requirements.txt` re-locked with `uv pip compile tools/codegen/requirements.in --generate-hashes --python-version 3.12 -o tools/codegen/requirements.txt`. **Rationale**: matches `PINNED_PYTHON`, CI, and `development.md` (uv for Python projects). **Alternative**: pyenv 3.14 — rejected, generates a different type set (464 vs 463 models).

## R-03 Upstream source checkout for tests and old-version diffs
**Decision**: `tools/codegen/upstream_src.py` provides `ensure_checkout(version) -> Path` = `git clone --depth 1 --branch v<version> https://github.com/googleapis/python-genai target/upstream-src/v<version>` (skipped if present) and `verify_matches_wheel(path)` which compares sha256 of every `google/genai/**/*.py` (excluding `tests/`) against the installed wheel and fails on mismatch. For the *old* side of a sync diff it uses `pip download google-genai==<old> --no-deps` + unzip to `target/upstream-src/wheel-<old>/`. **Rationale**: tests are not in the wheel; wheel/tag parity guarantees the tests describe the generated code. **Alternative**: vendoring tests into the repo — rejected (license/size, drift).

## R-04 Symbol fingerprint (what "changed" means)
**Decision**: per module, parse with `ast`; for each module-level function/class/assignment-constant and each class method, remove docstrings (first-statement string `Expr` in module/class/function), `ast.unparse` the node (drops comments and formatting; keeps decorators, annotations, defaults, bodies), `hashlib.sha256(...).hexdigest()[:16]`. Imports are excluded. **Rationale**: comment/format/import-order changes never fire (SC-003); defaults and decorators are inside the hashed text. **Alternative**: git diff text — rejected (noisy, no symbol granularity).

## R-05 Rust counterpart derivation and verification
**Decision**: counterpart is **derived by naming rule**, not hand-listed: Rust file(s) from `module_map.toml`, Rust name = `snake_case(upstream name)` with leading underscores removed; class `Foo` → `struct|enum|trait Foo`; method `Class.m` → `fn m` in the mapped file. Verification = regex over mapped file(s) for `(pub )?(async )?fn <name>\b|struct <Name>\b|enum <Name>\b|trait <Name>\b|const <NAME>\b|type <Name>\b`. Anything the rule cannot place needs an entry in `deviations.toml` (kind `renamed`, `merged`, `not_ported`, `generated`) with a reason, or the ledger check fails. **Rationale**: a derived mapping cannot drift and needs no hand maintenance (token cost). **Alternative**: hand-written mapping table — rejected (007 approach; drifts).

## R-06 Private upstream methods that wrap public ones (`_generate_content` vs `generate_content`)
**Decision**: a `_`-prefixed method whose un-prefixed public sibling exists in the same class is recorded automatically as status `merged` (folded into the public Rust method) — no deviation entry needed; a `_`-prefixed method with no public sibling maps to the un-prefixed Rust name. **Rationale**: Python needs the split for sync/async generated wrappers; Rust has one async method plus generated blocking wrapper.

## R-07 Converters
**Decision**: upstream converter functions `_<Type>_to_mldev` / `_<Type>_from_mldev` stay generated into `src/converters/generated/<module>.rs` (location unchanged); `*_to_vertex`/`*_from_vertex` are `out_of_scope` (reason code `vertex`). Their ledger module column is the upstream module that defines them (`models`, `batches`, …), mapped by `module_map.toml` `converters = true`. `getv/setv/…` helpers move from `converters/mod.rs` to `common.rs` (mirrors `_common.py`) and `gen_converters.py` emits `use crate::common::…`.

## R-08 Rule-1-over-rule-2 deviations (recorded, not hidden)
**Decision**: every place where mirroring upstream conflicts with a Rust best-practice rule is listed in `AGENTS.md` under "Upstream-mirroring deviations" (single home, kept in sync with `CONTRIBUTING.md`); ledger-level naming exceptions stay in `tools/codegen/deviations.toml` with a reason each. Candidates at planning time: ported-test names (upstream name + marker), sync/async collapse, no new `dyn Trait`, the 7 generated-converter panics, `#[expect]` instead of `#[allow]`.

## R-09 Upstream test inventory and classification
**Decision**: `tools/codegen/upstream_tests.py` walks `google/genai/tests/**/test_*.py` with `ast`: every `FunctionDef`/`AsyncFunctionDef` named `test_*` (module-level or inside a `class Test*`) is an entry keyed `<relpath>::<name>` (class tests `<relpath>::<Class>::<name>`); every `pytest_helper.TestTableItem(name=...)` is a *case* keyed `<relpath>::<item name>` under the file's table. Classification rules (`upstream_tests_rules.toml`, evaluated top-down, first match wins):
1. path glob `local_tokenizer/**`, `imports/**`, `public_samples/**`, `test_*replay*`, `_test_api_client` users → `excluded`, reason code `python_only` / `replay_infra` / `live_network`.
2. file stem matches `edit_image|recontext_image|segment_image|upscale|validate_reward|multi_regional|vertex` → `excluded`, `vertex`.
3. function/case level: a test function whose name contains `vertex`, or whose body sets `vertexai=True` for the client it asserts on, → `excluded`, `vertex`. A table item with `exception_if_mldev` set is **in scope** (mldev must raise; the corpus records `error_contains`); a table item with only `exception_if_vertex` is in scope (the mldev run succeeds).
4. everything else → `pending` until ported (`mapped`) or manually `excluded` with a reason code from the closed set `{vertex, python_only, replay_infra, live_network, pydantic_only, duplicate_of}`.
Manual overrides live in `upstream_tests.toml` itself (hand-edited fields `status`, `rust`, `reason_code`, `reason` are preserved by the generator across regenerations; new/removed upstream tests surface as `pending`/`removed`).

## R-10 Table-driven tests (327 items) — oracle corpus
**Decision**: `gen_upstream_cases.py` (run with the checkout on `PYTHONPATH`) imports each table-driven test module, reads `test_table` and `test_method`, and for every in-scope item (mldev run) executes the real Python SDK once with `Client(api_key='test-key', http_options=HttpOptions(base_url=<local>, httpx_client=<capture transport>))`, where the capture transport records `{method, path, query, headers_subset, body}` and returns the canned response chosen from `upstream_cases_responses.toml` (key = `test_method`; default per-method minimal valid body). It writes `tests/fixtures/upstream/<pymodule>/<stem>.json` = list of `{ name, test_method, parameters (pydantic model_dump(mode="json", by_alias=True, exclude_none=True)), expect: { request: {...} | null, error_contains: str|null }, response_body }`. `error_contains` is set when Python raised (the `exception_if_mldev` text must be a substring of the Python error). The Rust runner `tests/upstream_table/` deserializes parameters into the crate's typed params, calls the method against a `wiremock` server, and asserts the recorded request equals the corpus request (method, path, normalized query, body JSON equality) or that the error text contains the expected substring. **Rationale**: no replays exist; the Python SDK is the oracle, so request-shaping behavior is differential-tested. **Alternatives**: hand-written expectations (drift, 327 items) — rejected; Vertex replays — unavailable.

## R-11 Plain (non-table) tests
**Decision**: ported by hand, one Rust file per upstream test file at `tests/<pymodule>/<stem>.rs` (module root `tests/<pymodule>/main.rs` declares `mod <stem>;`), each ported function carries the exact upstream function name and the marker line `// upstream-test: <relpath>::<name>` immediately above `#[test]`/`#[tokio::test]`. Tests whose subject is Python-only are `excluded`. Existing flat files (`tests/models.rs` …) are moved to `tests/<pymodule>/main.rs` and their tests keep their current names (they are extra coverage, not mapped).

## R-12 Interactions & new modules via `_gaos`
**Decision**: generate, don't hand-port. `gen_gaos.py` (a) reflects every pydantic `BaseModel`/`Enum` under `google.genai._gaos.types.**` and `_gaos.models.**` into Rust (reusing the type-mapping helpers of `gen_types.py`; fields it cannot map, mostly open unions, become `serde_json::Value` and are listed in `gaos_overrides.toml` `[unmapped]` with a reason), and (b) extracts resource operations by AST from `_gaos/<resource>.py` (the synchronous class only): method name, HTTP verb, path template, path/query/body parameters, return model, streaming (`text/event-stream`). Output: `src/gaos/types.rs`, `src/gaos/models.rs`, `src/gaos/resources/<resource>.rs`, `src/gaos/mod.rs`. Public modules `agents.rs … interactions.rs` re-export the same names upstream's public modules export (`__all__`), and `client.rs` gains accessors `agents() environments() triggers() webhooks() voices() credentials() interactions()`. Streaming uses the existing `api_client::sse`. **Alternative**: hand-port 344 files — rejected (cost, unsyncable).

## R-13 Method skeleton generation for the classic resources
**Decision**: **not** in this feature. The oracle corpus (R-10) + ledger (R-04/05) already make resource orchestration changes executable and visible; generating `models.rs` skeletons from AST is a separate, larger effort. Recorded as follow-up in `docs/upstream-sync.md`.

## R-14 Vertex handling
**Decision**: no Vertex code is added or kept in hand-written code paths; existing `UnsupportedByBackend` stubs for Vertex-only methods stay (they preserve the public signature and fail clearly). Vertex-only upstream symbols are `out_of_scope` (`reason_code = "vertex"`).

## R-15 Version and migration
**Decision**: crate version 0.4.0 (breaking). `CHANGELOG.md` gets a "Migration from 0.3.x" table (old path → new path) produced by `tools/codegen/gen_migration.py` from `git mv` records listed in `tools/codegen/renames.toml` (one entry per renamed/relocated public item: old path, new path).

## R-16 CI
**Decision**: extend the existing `codegen-check` job to run, after `generate.py && git diff --exit-code`: `check_ledger.py`, `check_upstream_tests.py`. No new CI system.
