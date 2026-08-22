# Codegen tools

Regenerates this crate's generated sources, test fixtures, and parity
document from the installed `google-genai` Python SDK (`google.genai.types`
pydantic models/enums, and the `_to_mldev`/`_from_mldev` converter
functions in the SDK's private `_*.py` modules) plus the hand-maintained
method ledger `methods.toml`. See `plan.md` / `research.md` for why this is
generated rather than hand-written.

## Setup

```bash
pip install -r tools/codegen/requirements.txt
```

Per this project's environment policy, no virtualenv: run against the
`pyenv`-managed interpreter directly (`${HOME}/.anyenv/envs/pyenv/shims/python`).

## Running

```bash
python tools/codegen/generate.py                     # regenerate everything
python tools/codegen/generate.py --only types        # just src/types/generated/
python tools/codegen/generate.py --only converters   # just src/converters/generated/
python tools/codegen/generate.py --only fixtures     # just tests/fixtures/converters/
python tools/codegen/generate.py --only blocking     # just src/blocking/generated.rs
python tools/codegen/generate.py --only parity       # just docs/parity.md
```

`--only` accepts a comma-separated subset (`--only types,converters`).
Targets run in the order listed above regardless of how they are spelled
on the command line, because `parity` reports on what `types` produced.

`generate.py` runs `cargo fmt --all` after generating, so regeneration is
idempotent under `git diff --exit-code` (the CI `codegen-check` job runs
exactly this and fails if the checked-in generated files drift from what
the installed SDK version produces).

## Customizing generation

- **`gen_types.py`**: excludes/renames are listed inline near the top of
  the file (`_`-prefixed Python-internal classes, hand-written
  `HttpOptions`/`DebugConfig`, etc.).
- **`gen_converters.py`**: functions it can't transpile from the Python
  AST are listed as failures on stderr; hand-write a replacement under
  `tools/codegen/converter_overrides/<fn_name>.rs` and it's spliced in on
  the next run instead of the transpiled body.

## Targets

| Target | Generator | Output | Input |
|---|---|---|---|
| `types` | `gen_types.py` | `src/types/generated/{structs,enums,mod}.rs` | `google.genai.types` pydantic models/enums + `types_overrides.py` |
| `converters` | `gen_converters.py` | `src/converters/generated/*.rs` | the SDK's `_*_to_mldev` / `_*_from_mldev` function ASTs + `converter_overrides/` |
| `fixtures` | `gen_fixtures.py` | `tests/fixtures/converters/**/*.json` | `fixtures_cases.py`, executed against the real Python converters |
| `blocking` | `gen_blocking.py` | `src/blocking/generated.rs` | `methods.toml` (`kind = unary\|stream\|pager\|upload`) |
| `parity` | `gen_parity.py` | `docs/parity.md` | `methods.toml` + `specs/001-port-genai-rust/contracts/parity-matrix.md` + a scan of the repo's test functions |

`methods.toml` is the hand-maintained ledger of every ported public method
(module, Rust owner type, Python name, args, return type, `kind`, HTTP verb
and path). Its header comment documents why `kind = "session"` and
`kind = "manual"` entries exist and are skipped by `gen_blocking.py`.

`gen_parity.py` doubles as a regression check: if
`contracts/parity-matrix.md` marks a method ✅ and `methods.toml` has no
matching entry, it prints the offending rows and exits non-zero (so
`codegen-check` fails). Methods that are genuinely out of scope --
google-genai's Vertex-AI-only `models.edit_image` / `upscale_image` /
`recontext_image` / `segment_image` and `tunings.validate_reward`, all of
which raise `ValueError` when `vertexai=False` -- are listed with their
justification in `NOT_PORTED` at the top of `gen_parity.py`.
