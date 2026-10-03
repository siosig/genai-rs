# Codegen tools

Regenerates this crate's generated sources, test fixtures, oracle corpus,
parity document and symbol ledger from the installed `google-genai` Python SDK
(`google.genai.types`, the `_to_mldev`/`_from_mldev` converter functions in the
SDK's private `_*.py` modules, and the private `_gaos` sub-SDK), plus the
hand-maintained inputs listed under [Inputs](#inputs). It also holds the tools
that make an upstream version bump reviewable: a symbol ledger, a sync diff and
an upstream-test inventory. The workflow is in
[docs/upstream-sync.md](../../docs/upstream-sync.md).

## Setup

The interpreter version is part of the input (`google.genai.types` differs
between Python versions), so use the pinned 3.12 in a dedicated venv:

```bash
uv venv --python 3.12 target/codegen-venv
uv pip install --python target/codegen-venv/bin/python --require-hashes -r tools/codegen/requirements.txt
PY=target/codegen-venv/bin/python
```

`upstream_src.py` additionally shallow-clones the upstream tag (tests included)
into `target/upstream-src/`, which is git-ignored; this needs network access.

## Running

```bash
$PY tools/codegen/generate.py                     # regenerate everything
$PY tools/codegen/generate.py --only types        # just src/types/generated/
$PY tools/codegen/generate.py --only converters,cases   # a comma-separated subset
```

Targets always run in the order listed in the table below regardless of how
they are spelled on the command line, because later ones report on earlier
ones (`parity` on `types`, `ledger` on all hand-written files).

`generate.py` runs `cargo fmt --all` after generating, so regeneration is
idempotent under `git diff --exit-code` (the CI `codegen-check` job runs
exactly this and fails if the checked-in generated files drift from what the
installed SDK version produces).

## Targets

| Target | Generator | Output | Input |
|---|---|---|---|
| `types` | `gen_types.py` | `src/types/generated/{structs,enums,mod}.rs` | `google.genai.types` pydantic models/enums |
| `converters` | `gen_converters.py` | `src/converters/generated/*.rs` | the SDK's `_*_to_mldev` / `_*_from_mldev` function ASTs + `converter_overrides/` |
| `fixtures` | `gen_fixtures.py` | `tests/fixtures/converters/**/*.json` | `fixtures_cases.py`, executed against the real Python converters |
| `cases` | `gen_upstream_cases.py` | `tests/fixtures/upstream/**/*.json` | upstream's table-driven tests, executed against the real Python SDK (the oracle corpus) |
| `gaos` | `gen_gaos.py` | `src/gaos/**`, `tests/fixtures/gaos/*.json`, `tests/gaos/generated_ops.rs`, `methods_gaos.toml` | the installed `google.genai._gaos` package + `gaos_overrides.toml` |
| `blocking` | `gen_blocking.py` | `src/blocking/generated.rs` | `methods.toml` and `methods_gaos.toml` (`kind = unary\|stream\|pager\|upload`) |
| `parity` | `gen_parity.py` | `docs/parity.md`, `docs/parity.ja.md` | `methods.toml` + `parity-matrix.ja.md` + a scan of the repo's test functions |
| `ledger` | `gen_ledger.py` | `tools/codegen/ledger.toml` | the installed SDK's symbols + `module_map.toml` + `deviations.toml` |

## Inputs

Hand-edited, tracked, and read by the generators above:

| File | Meaning |
|---|---|
| `methods.toml` | Ledger of every ported public method (module, Rust owner type, Python name, args, return type, `kind`, HTTP verb and path). Its header comment documents why `kind = "session"` and `kind = "manual"` entries exist and are skipped by `gen_blocking.py` |
| `module_map.toml` | Upstream module to Rust file(s); the table is reproduced in `docs/upstream-sync.md` |
| `deviations.toml` | Exceptions to the naming rule (`renamed`, `merged`, `not_ported`, `generated`), each with a reason |
| `upstream_tests_rules.toml` | Rules that classify upstream tests as `excluded` with a reason code |
| `gaos_overrides.toml` | Name collisions and per-resource overrides for `gen_gaos.py` |
| `converter_overrides/<fn>.rs` | Hand-written replacement for a converter `gen_converters.py` cannot transpile; spliced in on the next run |
| `renames.toml` | One entry per public Rust path moved by the structure alignment; feeds `gen_migration.py` |
| `fixtures_cases.py`, `parity-matrix.ja.md`, `parity_strings.ja.toml` | Golden-fixture cases and the parity document's contract and Japanese strings |

## Checks and sync tools

| Tool | Use | Exit |
|---|---|---|
| `check_ledger.py` | Re-derives the ledger and fails when `ledger.toml` differs or any symbol is `unmapped` | 1 on failure |
| `check_upstream_tests.py` | Re-inventories upstream's tests; fails on new/removed tests, a `mapped` entry without its `// upstream-test:` marker, or an `excluded` entry without a valid reason code. `--final` also requires `pending = 0`; `--update` rewrites the inventory preserving hand edits (`--prune` drops removed tests); `--list [--dir <prefix>]` lists the `pending` ones; `--report` prints counts only | 1 on failure |
| `sync_diff.py --from <old> [--to <new>] [--format text\|json]` | Prints only the upstream symbols that changed, were added or were removed between two versions, with the Rust counterpart and status from the ledger | 2 when a version cannot be resolved |
| `upstream_src.py` | Fetches the upstream tag and wheel sources the other tools read, and verifies the tag equals the wheel | non-zero on mismatch |
| `gen_migration.py [--check]` | Writes the old-path/new-path table into `CHANGELOG.md` between its `migration:start`/`migration:end` markers from `renames.toml` | 1 with `--check` when stale |

Unit tests for the tools: `python -m unittest discover tools/codegen/tests`.

## Customizing generation

- **`gen_types.py`**: excludes/renames are listed inline near the top of the
  file (`_`-prefixed Python-internal classes, hand-written
  `HttpOptions`/`DebugConfig`, etc.).
- **`gen_converters.py`**: functions it can't transpile from the Python AST
  are listed as failures on stderr; hand-write a replacement under
  `converter_overrides/<fn_name>.rs`.
- **`gen_gaos.py`**: unions it cannot express as a tagged enum become
  `serde_json::Value` and are counted as unmapped in its output.

`gen_parity.py` doubles as a regression check: if `parity-matrix.ja.md` marks a
method as ported and `methods.toml` has no matching entry, it prints the
offending rows and exits non-zero (so `codegen-check` fails). Methods that are
genuinely out of scope -- google-genai's Vertex-AI-only `models.edit_image` /
`upscale_image` / `recontext_image` / `segment_image` and
`tunings.validate_reward`, all of which raise `ValueError` when
`vertexai=False` -- are listed with their justification in `NOT_PORTED` at the
top of `gen_parity.py`.
