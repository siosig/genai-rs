# Contributing

Japanese version: [CONTRIBUTING.ja.md](CONTRIBUTING.ja.md)

Thanks for looking. This is an unofficial Rust port of the
[Google Gen AI Python SDK](https://github.com/googleapis/python-genai) for the
Gemini Developer API — see [NOTICE](NOTICE) for what is derived from upstream
and what is not.

For a security problem, do **not** open an issue: follow [SECURITY.md](SECURITY.md).

## Table of contents

- [One-time setup](#one-time-setup)
- [Quality gates](#quality-gates)
- [Generated code](#generated-code)
- [Language policy](#language-policy)
- [Updating pinned things](#updating-pinned-things)
- [Live tests](#live-tests)

## One-time setup

```sh
git clone https://github.com/siosig/genai-rs
cd genai-rs
git config core.hooksPath hooks   # enable the commit-time gates
```

`rust-toolchain.toml` pins the toolchain, so `cargo` installs the right version
by itself. Regenerating code additionally needs Python 3.12.

### The commit hooks

`git config core.hooksPath hooks` enables two gates that apply to everybody:

| Gate | What it blocks |
| --- | --- |
| `secret-scan` | Staged files containing credentials, per secretlint's recommended preset |
| `english-only-content` | Japanese/CJK in a staged file whose name does not match `*.ja*` |

`hooks/commit-msg` additionally rejects a commit message containing Japanese.

Both are re-run server-side by the `secret-scan` workflow, so `--no-verify`
gets you past the local check but not past CI.

`hooks/local.d/` is for personal gates and is git-ignored apart from the
`*.sample` templates. If you use several git identities on one machine and want
to be sure the wrong one never signs a commit here:

```sh
cp hooks/local.d/author.sample hooks/local.d/author
# then edit the address inside
```

That gate is deliberately *not* shipped enabled — it hard-codes a single email
address, so enabling it for everyone would reject every external contribution.

## Quality gates

Run all five before opening a pull request. CI runs the same ones.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --locked
python tools/codegen/generate.py && git diff --exit-code
```

`--locked` is not optional: `Cargo.lock` is committed, so a build that would
have to change it is a build against a different dependency graph than the one
your change was written for.

Two more, if your change could plausibly affect them:

```sh
# MSRV, for every feature combination.
rustup toolchain install 1.88.0 --profile minimal
cargo +1.88.0 check --workspace --all-features --locked
```

Import grouping (`std` / external / `crate`, one `use` per crate) is enforced by
the advisory `fmt-check-nightly` job only, because the rustfmt options behind it
are nightly-only. To match it locally:

```sh
rustup toolchain install nightly --profile minimal --component rustfmt
cargo +nightly fmt --all
```

Generated code is excluded from that pass (`ignore` in `rustfmt.toml`): its
layout is owned by the generator and the stable `cargo fmt` it runs.

`AGENTS.md` has the coding conventions this repository follows.

### The three guard test suites

Beyond the normal tests, three suites exist to stop specific mistakes:

| Suite | Stops |
| --- | --- |
| `tests/attribution.rs` | Losing the Apache-2.0 attribution owed to upstream |
| `tests/protected_identifiers.rs` | A rename touching an identifier that belongs to the upstream API |
| `tests/supply_chain.rs` | An unpinned action, a missing `--locked`, an unverified download |

If one of these fails, read its failure message before changing the test — they
each explain what breaks in production if the property is lost.

## Generated code

These are produced by `tools/codegen/*.py` and **must not be hand-edited**:

- `src/types/generated/`
- `src/converters/generated/`
- `src/blocking/generated.rs`
- `tests/fixtures/converters/`
- `docs/parity.md`

Their inputs -- `tools/codegen/methods.toml`, `tools/codegen/parity-matrix.ja.md`,
`tools/codegen/fixtures_cases.py` -- are tracked and hand-edited.

Change the generator (or a `converter_overrides/<fn>.rs` file) and re-run:

```sh
uv venv --python 3.12 --seed .venv-codegen
.venv-codegen/bin/pip install --require-hashes -r tools/codegen/requirements.txt
.venv-codegen/bin/python tools/codegen/generate.py   # or --only types,converters,…
```

**The interpreter version is part of the input.** `google.genai.types` exposes a
different set of pydantic models depending on it -- 3.12 defines 464 including
`BlobImageUnion`, 3.14 defines 463 without it -- so generating on the wrong one
silently changes the crate's public API and breaks `codegen-check` for whoever
pushes next. `tools/codegen/upstream.py` refuses to run on anything but the
pinned 3.12 (override with `GENAI_ALLOW_PYTHON_DRIFT=1` if you are deliberately
evaluating a newer one).

Every generated file carries an attribution header emitted by
`tools/codegen/attribution.py`, which is the single source for that wording.
Change it there, never in the output.

## Language policy

Tracked files are written in **English**. A Japanese translation lives beside
the original with a `.ja` in its name: `README.ja.md`, `SECURITY.ja.md`,
`CONTRIBUTING.ja.md`, `docs/migrating-from-python.ja.md`. The
`english-only-content` hook enforces this, and it applies to *inputs* too --
`tools/codegen/parity-matrix.ja.md` is a Japanese contract document that the
parity generator reads, and it carries the suffix for exactly that reason.

A generated document that needs a Japanese version keeps its strings in a `.ja`
data file (`tools/codegen/parity_strings.ja.toml`) rather than as escapes inside
the generator. That file is currently unused: the locale support in
`gen_parity.py` that would consume it is not in the tree yet, so `docs/parity.md`
is English-only.

Commit messages are English too (`hooks/commit-msg`).

## Updating pinned things

Everything external is pinned, so updates are deliberate rather than automatic.
Dependabot proposes most of them; these are the manual ones.

| What | Where | How |
| --- | --- | --- |
| Rust dependencies | `Cargo.lock` | Dependabot, or `cargo update -p <crate>` |
| Rust toolchain | `rust-toolchain.toml` and `RUST_TOOLCHAIN` in `.github/workflows/ci.yml` | Edit both; the advisory `clippy-latest` job shows what a newer stable would flag |
| MSRV | `rust-version` in `Cargo.toml` and `RUST_MSRV` in `ci.yml` | Edit both |
| CI actions | `uses:` SHAs | Dependabot updates the SHA and the `# vX.Y.Z` comment together |
| Python interpreter | `PINNED_PYTHON` in `tools/codegen/upstream.py` and `python-version` in `ci.yml` | Edit both, regenerate, and review the resulting API diff |
| Python codegen deps | `tools/codegen/requirements.in` → `.txt` | Edit the `.in`, then `uv pip compile tools/codegen/requirements.in --generate-hashes --python-version 3.12 -o tools/codegen/requirements.txt` (`pip-compile` works too) |
| gitleaks | `GITLEAKS_VERSION` in `secret-scan.yml` | Edit; the checksum comes from the release's own `checksums.txt` |
| secretlint | `SECRETLINT_VERSION` in `secret-scan.yml` **and** `hooks/pre-commit` | Edit **both** — `tests/supply_chain.rs` fails if they disagree |

### Upgrading the upstream SDK

This one is not a dependency bump; it regenerates the whole generated tree.
Dependabot is configured to leave `google-genai` alone for that reason. The
procedure is in the module docstring of `tools/codegen/upstream.py`.

## Live tests

Tests that hit the real API are `#[ignore]`d and skip themselves without a key:

```sh
GEMINI_API_KEY=... cargo test --all-features -- --ignored --nocapture
```

`tests/e2e_expensive.rs` (video generation, batch jobs, Live sessions) costs
meaningfully more quota and needs a second opt-in:

```sh
GEMINI_API_KEY=... GENAI_E2E_EXPENSIVE=1 \
  cargo test --all-features --test e2e_expensive -- --ignored --nocapture
```

Never commit a key. If you think you have, say so in a
[security report](SECURITY.md) rather than force-pushing quietly — a key that
reached a public remote should be rotated regardless of whether the commit
still exists.
