# Contract: symbol ledger and ledger tools

## Files and commands (all run with `target/codegen-venv/bin/python`)

| Command | Behavior | Exit |
|---|---|---|
| `tools/codegen/gen_ledger.py` | Reads installed wheel (= pinned version) source, computes symbols + fingerprints, derives Rust counterparts (R-05/R-06), applies `deviations.toml` and `module_map.toml`, writes `tools/codegen/ledger.toml` | 0 |
| `tools/codegen/check_ledger.py` | Re-derives and compares with checked-in `ledger.toml`; fails when (a) the file differs, or (b) any symbol has `status = "unmapped"` | 1 on failure; prints each offending `module::qualname` |
| `tools/codegen/sync_diff.py --from <old-version> [--to <new-version>] [--format json\|text]` | See contracts/sync-report.md | 0 (report printed); 2 on resolution failure, no partial output |

`generate.py` gains target `ledger` (runs `gen_ledger.py`), appended after `parity`; `--only ledger` supported.

## `ledger.toml`
```toml
upstream_version = "2.28.0"

[[symbol]]
module = "_transformers"
qualname = "t_content"
kind = "function"          # function | class | method | const
public = false             # module name or symbol name starting with "_" => false; also false for names not in __all__ when __all__ exists
fingerprint = "3fa9c2d1e07b44aa"
status = "ported"          # ported | generated | merged | out_of_scope | unmapped
rust = "src/transformers.rs::t_content"
reason_code = ""
reason = ""
```
Sorting: by `(module, line order in upstream file)` — **definition order is preserved** (no alphabetical sort) so a reordering upstream is visible.

## Status derivation (in order)
1. `deviations.toml` entry → its `kind` (`renamed|merged|not_ported|generated`; `not_ported` ⇒ `out_of_scope`).
2. Module `out_of_scope` in `module_map.toml` → `out_of_scope` with the module's reason.
3. Vertex-only: name matches `.*_(to|from)_vertex$` or the symbol's source contains an `if self._api_client.vertexai` branch that is the *only* code path (determined by a fixed list in `deviations.toml`, not by heuristics) → `out_of_scope`, `vertex`.
4. `types.py` classes/enums → `generated`, `rust = "src/types/generated::<Name>"`, verified by presence in generated output.
5. Converter function in a converter-bearing module → `generated`, `rust = "src/converters/generated/<module>.rs::<snake>"`.
6. Private method with public sibling → `merged`.
7. Naming-rule hit by regex in a mapped file → `ported`.
8. Otherwise `unmapped`.

## Acceptance
- At the final gate `check_ledger.py` reports 0 `unmapped`.
- Every `out_of_scope` and deviation has a non-empty `reason`, and `reason_code ∈ {vertex, python_only, replay_infra, structural, not_applicable}`.
