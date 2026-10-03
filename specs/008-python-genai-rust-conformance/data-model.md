# Data Model (tooling artifacts)

All files are TOML or JSON, UTF-8, deterministic (sorted keys/entries, `\n` endings, regenerated idempotently).

## Ledger entry — `tools/codegen/ledger.toml`
See [contracts/ledger.md](contracts/ledger.md). Key: `(module, qualname)`. Fields: `kind` (`function|class|method|const`), `public` (bool), `fingerprint` (16 hex), `status` (`ported|generated|merged|out_of_scope|unmapped`), `rust` (string, `path::name`, empty when none), `reason_code` (closed set), `reason` (free text, required when `out_of_scope` or deviation).

## Deviation — `tools/codegen/deviations.toml`
Key `(module, qualname)`; `kind` ∈ `renamed|merged|not_ported|generated`; `rust` (target when `renamed`); `reason`. Presence overrides the naming rule.

## Module map entry — `tools/codegen/module_map.toml`
`[[module]] upstream = "_transformers"; rust = ["src/transformers.rs"]; converters = false` or `out_of_scope = true; reason_code; reason`.

## Upstream test entry — `tools/codegen/upstream_tests.toml`
See [contracts/upstream-tests.md](contracts/upstream-tests.md). Key `(file, name)` plus `case` for table items. Fields: `status` (`mapped|excluded|pending`), `rust`, `reason_code`, `reason`, `cases` (int).

## Oracle case — `tests/fixtures/upstream/<pymodule>/<stem>.json`
```json
{ "upstream_version": "2.28.0", "source": "models/test_generate_content.py", "test_method": "models.generate_content",
  "cases": [ { "name": "test_text", "parameters": {...}, "response_body": {...},
               "expect": { "request": {"method":"POST","path":"/v1beta/models/gemini-2.5-flash:generateContent","query":{},"body":{...}}, "error_contains": null } } ] }
```

## Sync report — printed by `sync_diff.py`
See [contracts/sync-report.md](contracts/sync-report.md).

## Gaos generation inputs — `tools/codegen/gaos_overrides.toml`
`[unmapped."<Module>.<Type>.<field>"] reason = "..."` (field emitted as `serde_json::Value`); `[rename."<python name>"] rust = "<Rust name>"` for collisions with `types.rs` names.

## Renames — `tools/codegen/renames.toml`
`[[rename]] old = "gemini_genai::http::…"; new = "gemini_genai::api_client::…"` — source of the CHANGELOG migration table.
