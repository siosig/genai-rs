# Contract: sync report

`sync_diff.py --from 2.27.0 --to 2.28.0` (default `--to` = pinned version; sources via R-03).

## Algorithm
1. Compute symbols (R-04) for both versions.
2. `added` = in new not old; `removed` = in old not new; `changed` = fingerprint differs. Symbols that differ only in docstring/comment/format/import order produce equal fingerprints and are not listed.
3. For each listed symbol attach: `rust` + `status` from the *checked-in* `ledger.toml` (for added: derived counterpart or `unmapped`), and for `changed`/`added`/`removed` a unified diff of `ast.unparse` text limited to that symbol (context 3).
4. Private symbols that a listed public symbol calls are not auto-linked; they appear as their own entries (private symbols are in the ledger).

## JSON (stable key order; deterministic for equal inputs)
```json
{ "from": "2.27.0", "to": "2.28.0",
  "counts": {"changed": 12, "added": 5, "removed": 1, "unmapped": 2},
  "entries": [ { "change": "changed", "module": "_transformers", "qualname": "t_schema",
                 "rust": "src/transformers.rs::t_schema", "status": "ported", "diff": "--- ...\n+++ ..." } ] }
```
## Text form
One block per entry: `CHANGED _transformers::t_schema -> src/transformers.rs::t_schema [ported]` followed by the diff. A trailing summary line `changed=12 added=5 removed=1 unmapped=2`. Renames upstream appear as a removed + an added entry.

## Workflow (documented in `docs/upstream-sync.md` en/ja)
1. `sync_diff.py --from <current pin> --to <new>` → read only the entries.
2. Bump `PINNED_VERSION` + `requirements.in`, relock, `generate.py` (regenerates generated layers + ledger).
3. Port each `changed/added` entry in the named Rust item; run `check_upstream_tests.py` (new/changed upstream tests appear `pending`); run `gen_upstream_cases.py` (oracle corpus) and `cargo test`.
4. `gen_ledger.py` to accept; update CHANGELOG.
