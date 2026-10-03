# Final audit — success criteria and gates

Measured on 2026-10-03 against google-genai 2.28.0 (crate 0.4.0). Every number below came from a command listed in `quickstart.md`, or from the measurement notes under each row.

## Success criteria

| SC | Target | Measured | Verdict |
|----|--------|----------|---------|
| SC-001 | 100% of public upstream symbols mapped or out of scope with a reason | `check_ledger.py`: 2081 symbols (673 generated, 235 ported, 176 merged, 997 out of scope with a coded reason), 0 unmapped | Met |
| SC-002 | 100% of upstream tests mapped to a passing Rust test or excluded with a reason | `check_upstream_tests.py --final`: 1697 tests/cases, 1111 mapped, 586 excluded, 0 pending. Excluded by reason: python_only 295, pydantic_only 97, vertex 179, replay_infra 11, live_network 5, duplicate_of 2 | Met, with one caveat below |
| SC-003 | Sync report has every changed symbol and no comment-only change | 2.23.0→2.28.0: independent AST comparison finds changed 83 / added 13 / removed 11; the report lists exactly those (missed 0, comment-or-format-only entries 0). 2.22.0→2.23.0: 3 of 3 | Met (the independent check uses a different hash — `ast.dump` — over the same docstring-stripped notion of "same") |
| SC-004 | A sync reads at most 25% of the source read for the 2.23.0 sync | 2.19.0→2.23.0: sync report 77 KB + the 13 Rust items it names 2 KB = 79 KB, against 1.97 MB for the changed upstream modules plus the Rust files that sync touched: 4.0% | Met |
| SC-005 | A deliberate divergence in 5 sampled helpers/methods is caught offline, naming the origin | `t_model`, `t_cached_content_name`, `t_file_name`: caught by the oracle corpus, naming `<test file>::<case>`. `t_contents`, `t_batch_job_source`: caught by ported unit tests carrying the upstream test name (not by the corpus alone). 5/5 | Met |
| SC-006 | All gates pass; no new `unwrap`/`expect`/`panic!` in production code; no unjustified suppression | fmt, clippy (`-D warnings`, all targets and features), doc (`-D warnings`), 1476 tests pass (17 live tests ignored), ledger, inventory, Python unit tests, generator idempotence all exit 0. The 15 production `expect`/`unwrap` sites all pre-date this work and carry `#[expect(…, reason)]`; no new `#[allow]` in hand-written code | Met |
| SC-007 | At least 90% of pure-logic helpers generated or tested; the rest listed | 66 in-scope helpers in `_transformers`, `_base_transformers`, `_common`, `_extra_utils`: 66 referenced by a unit test, golden, or corpus case (100%) | Met |

## Caveats

- **SC-002 exclusions** rest on an audit of 34 sampled `python_only`/`pydantic_only` rows (seed 7). Four were wrong (File coercion) and were ported; the other 30 held. About 370 such rows were not sampled, so the exclusion list is checked for reason codes and wording, not row by row.
- **Corpus scope**: 260 table cases run through the oracle corpus; cases whose `test_method` is a file-local function (not a client method), resumable uploads, and Vertex-named cases were ported by hand or excluded (`tests/fixtures/upstream/_custom.json`).
- **Known gaps** are listed in `docs/upstream-sync.md` (Interactions derived properties and request-union deserialization, SSE unterminated final event, MIME guessing difference).

## Gate record

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`, `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features`, `cargo test --workspace --all-features --no-fail-fast` (1476 passed, 0 failed, 17 ignored), `tools/codegen/generate.py` (twice, identical output), `check_ledger.py`, `check_upstream_tests.py --final`, `python -m unittest discover tools/codegen/tests`, `gen_migration.py --check`: all exit 0.
