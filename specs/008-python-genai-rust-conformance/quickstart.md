# Quickstart: validating the feature

Prereqs: Rust toolchain from `rust-toolchain.toml`, `uv`, `git`, network (for the pinned tag).

```bash
# 1. codegen env (R-02)
uv venv --python 3.12 target/codegen-venv
uv pip install --python target/codegen-venv/bin/python --require-hashes -r tools/codegen/requirements.txt
PY=target/codegen-venv/bin/python

# 2. regenerate everything and verify sync (generated files must not drift)
$PY tools/codegen/generate.py && git diff --exit-code

# 3. ledger and upstream-test coverage
$PY tools/codegen/check_ledger.py
$PY tools/codegen/check_upstream_tests.py --final
$PY tools/codegen/sync_diff.py --from 2.23.0 --to 2.28.0 --format text | tail -1   # summary line

# 4. Rust gates
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```
Expected: every command exits 0; `check_upstream_tests.py --final` prints `pending=0`; the sync summary line shows nonzero `changed/added/removed` for 2.23.0→2.28.0 (measured: `changed=83 added=13 removed=11`).

Divergence drill (SC-005): in `src/transformers.rs` change one branch of `t_part`; `cargo test` fails with a message naming an `upstream-test:`/corpus case; revert.

Live tests (opt-in only): `GEMINI_API_KEY=… cargo test --features … -- --ignored` per existing `tests/e2e*.rs` conventions; never in the default run.
