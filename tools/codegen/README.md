# Codegen tools

Regenerates `src/types/generated/*.rs` and `src/converters/generated/*.rs`
from the installed `google-genai` Python SDK (`google.genai.types`
pydantic models/enums, and the `_to_mldev`/`_from_mldev` converter
functions in the SDK's private `_*.py` modules). See `plan.md` /
`research.md` for why this is generated rather than hand-written.

## Setup

```bash
pip install -r tools/codegen/requirements.txt
```

Per this project's environment policy, no virtualenv: run against the
`pyenv`-managed interpreter directly (`${HOME}/.anyenv/envs/pyenv/shims/python`).

## Running

```bash
python tools/codegen/generate.py                    # regenerate everything
python tools/codegen/generate.py --only types        # just src/types/generated/
python tools/codegen/generate.py --only converters   # just src/converters/generated/
```

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

## `tools/codegen/generate.py --only fixtures|blocking|parity`

Not yet implemented (see `tasks.md` T032 / T041 / T087); `generate.py`
currently only supports `types` and `converters`.
