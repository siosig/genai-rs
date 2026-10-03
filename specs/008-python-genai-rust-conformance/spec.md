# Feature Specification: Python google-genai Conformance (Gemini Developer API)

**Feature Branch**: `008-python-genai-rust-conformance`

**Created**: 2026-10-03

**Status**: Draft

**Input**: User description: "Make the Rust crate `gemini-genai` a Rust implementation that conforms to the latest upstream Python `google-genai` SDK for the Gemini Developer API. Vertex AI is out of scope (too complex). The test suite must cover at least every upstream Python test that is not Vertex-AI-specific. First rule: keep the Rust structure as close to Python as possible, or structured so upstream changes are easy to absorb with Claude. Second rule: follow the Rust best-practice rules (Apollo GraphQL handbook based). The prior branch 007-upstream-sync-ease is discarded."

## Context

The crate already generates types, converters, blocking wrappers, golden converter fixtures and a parity table from the upstream SDK (pinned 2.23.0). The hand-written layer (request orchestration, transformers, upload, live sessions, HTTP plumbing) is organised by Rust convenience, so absorbing an upstream release means reading both code bases and semantically decomposing each change, which is token-expensive for an AI assistant. The previous attempt (007) also targeted Vertex AI and was discarded; its work-in-progress is stashed, not merged. This feature starts fresh from `main`, drops Vertex AI, and makes upstream correspondence and upstream-test coverage the primary design drivers.

**Actors**: *Maintainer* (a human, or Claude acting for them, performing an upstream sync); *Crate user* (consumer of `gemini-genai` against the Gemini Developer API).

**Rule priority** (from the owner): (1) ease of following upstream with Claude, (2) compliance with the Rust best-practice rules in the Rust best-practice rules (Apollo GraphQL handbook based). When they conflict, rule 1 wins and the deviation is recorded.

## Clarifications

### Session 2026-10-03

- Q: Is Vertex AI in scope? → A: No. Vertex-only methods, Vertex authentication/credentials, Vertex-only converters and fields, and Vertex-only upstream tests are out of scope, each listed with a reason. This reverses the discarded 007 decision.
- Q: What is the test floor? → A: Every upstream Python test that is not Vertex-AI-specific must have a Rust counterpart, or be listed as explicitly excluded with a reason (for example Python-only behavior, pydantic-only behavior, replay-infrastructure internals).
- Q: May the public API change to match upstream? → A: Yes. Public paths and names may be renamed or relocated to match upstream; the release is a breaking release with a migration table.
- Q: Is the Interactions API (interactions, agents, environments, triggers, webhooks) in scope? → A: Yes (recommended option A). Types and resource methods are generated from upstream wherever regular enough, and the corresponding non-Vertex upstream tests are ported.
- Q: Which upstream version is the target? → A: The latest upstream release at planning time (currently pinned 2.23.0; planning must check for a newer one).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Absorb an upstream release with minimal reading (Priority: P1)

A maintainer upgrades the pinned upstream version. Because each upstream module, class, method and helper has a predictable Rust counterpart (same name in Rust casing, same relative order), and because changes to mechanical layers are produced by generation, the maintainer (or Claude) reads only the changed upstream symbols and the Rust items that correspond to them, and never has to search for where a change belongs.

**Why this priority**: This is the owner's first rule and the core cost driver. It delivers value even if later stories are only partly done.

**Independent Test**: Replay the 2.22.x → 2.23.0 transition. For every upstream symbol that changed, a maintainer can find its Rust counterpart by name alone (no search through unrelated files), and the set of files a sync touches is limited to those counterparts plus regenerated artifacts.

**Acceptance Scenarios**:

1. **Given** an upstream public module, class, method or module-level function, **When** the maintainer looks for its Rust counterpart, **Then** a Rust item with the corresponding name (Rust casing) exists in the corresponding module, and corresponding items keep the upstream relative order.
2. **Given** an upstream release that changes some symbols, **When** the maintainer runs the sync workflow, **Then** it reports only the changed, added and removed upstream symbols, each paired with its Rust counterpart(s).
3. **Given** a Rust item that deviates from upstream structure because of a Rust language constraint, **When** the maintainer reviews it, **Then** the deviation is recorded with its reason next to the correspondence record.
4. **Given** an upstream change that only affects generated layers (types, converters, blocking wrappers), **When** regeneration runs, **Then** no hand edit is needed and the regenerate-and-diff check is clean.

---

### User Story 2 - Upstream's non-Vertex tests all run against the Rust crate (Priority: P1)

A maintainer can run one command and see every non-Vertex upstream Python test (taken from the upstream repository's test suite at the target release) executed in Rust form, or recorded as excluded with a reason. A behavior mismatch against upstream fails a test that names its upstream origin.

**Why this priority**: The owner requires coverage of all upstream non-Vertex tests. This converts "is the sync correct?" from reading into executing, which lowers both token cost and defect risk.

**Independent Test**: The test-coverage report enumerates every upstream test; each is mapped to a passing Rust test, or listed excluded with a reason. Introduce a deliberate divergence in one covered behavior; a mapped test fails and names the upstream test.

**Acceptance Scenarios**:

1. **Given** the upstream test suite at the target release, **When** the coverage check runs, **Then** every upstream test is either mapped to a Rust test or listed excluded with a reason, and the check fails if one is neither.
2. **Given** an upstream test that is Vertex-specific (Vertex-only method, credentials, endpoint or field), **When** the check runs, **Then** it is listed as excluded with the reason "Vertex AI out of scope" and is not counted as a gap.
3. **Given** an upstream test that exercises both backends, **When** it is ported, **Then** the Gemini Developer API variant is ported and the Vertex variant is recorded as excluded.
4. **Given** a new upstream release that adds or changes tests, **When** the coverage check runs, **Then** the new or changed upstream tests appear as unmapped until ported or excluded.

---

### User Story 3 - Full Gemini Developer API parity, nothing silently missing (Priority: P2)

A crate user can use every Gemini Developer API capability the target upstream version offers (models, chats, files, caches, batches, tunings, operations, file search stores, documents, auth tokens, live sessions, automatic function calling, MCP tooling, pagers, and the typed request/response models). Anything not ported is listed with a written reason.

**Why this priority**: Gaps force ad-hoc porting later and make test coverage (Story 2) impossible to claim. It is P2 because it builds on the correspondence rules of Story 1.

**Independent Test**: The parity check enumerates every public upstream symbol and fails if one is neither mapped to a Rust item nor listed out of scope with a reason.

**Acceptance Scenarios**:

1. **Given** the target upstream version, **When** the parity check runs, **Then** 100% of public upstream symbols are mapped or listed out of scope with a reason.
2. **Given** a Vertex-only upstream capability (for example image editing, upscaling, recontext, segmentation, reward validation, Vertex credentials), **When** the parity table is rendered, **Then** it is listed as out of scope with the reason "Vertex AI out of scope".
3. **Given** a Python-ecosystem-only capability (pydantic helpers, local tokenizer, replay client), **When** the parity table is rendered, **Then** it is listed out of scope with a reason.

---

### User Story 4 - Code stays compliant with the Rust best-practice rules (Priority: P2)

A reviewer can verify that the new and moved code satisfies the Rust rules in the Rust best-practice rules (Apollo GraphQL handbook based), using the repository's quality gates, without per-file inspection.

**Why this priority**: Second rule of the owner; it constrains how Stories 1–3 are delivered and is cheap to enforce mechanically.

**Independent Test**: Format, lint (warnings denied), test, doc and generated-sync gates all pass; a search finds no unwrap/expect/panic in production code outside the recorded generated-converter invariants.

**Acceptance Scenarios**:

1. **Given** the finished change, **When** all quality gates run, **Then** they pass with no new lint suppressions other than locally justified ones.
2. **Given** a place where mirroring upstream structure conflicts with a Rust rule, **When** a reviewer inspects it, **Then** the conflict and the chosen side (rule 1 wins) are recorded.

---

### Edge Cases

- Upstream renames or moves a symbol: it is reported as removed plus added; the maintainer remaps. Rename heuristics are out of scope.
- One upstream symbol is ported by several Rust items, or one Rust item ports several: correspondence supports many-to-many.
- An upstream test relies on recorded server replays: it is ported using the crate's own mock/recorded-response approach, or excluded if it only tests the replay machinery itself.
- An upstream test mixes Vertex and Developer API assertions: split; only the Developer API half counts.
- Upstream tests need live network and an API key: they are classified as live tests, kept behind an explicit opt-in, and still counted as covered when ported.
- The target upstream version moves between specify and implement: the target is re-pinned once at plan time and not re-chased mid-feature.
- Uncommitted or stashed work from the discarded 007 effort exists: it is left untouched.
- Generated-file hand edits: the existing regenerate-and-diff check must keep failing on drift.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The crate MUST implement, for the Gemini Developer API, every in-scope public capability of the target upstream release (latest release at planning time), including the Interactions API surface (interactions, agents, environments, triggers, webhooks).
- **FR-002**: Vertex AI MUST be out of scope: Vertex-only methods, Vertex authentication and credential handling, Vertex-only converters and fields, and Vertex-only upstream tests MUST each be listed out of scope with a reason; no Vertex code path is added.
- **FR-003**: Out-of-scope items MUST be limited to (a) Vertex AI, (b) Python-ecosystem-only features that do not affect request/response behavior, and (c) upstream test infrastructure; each MUST carry a written reason in a machine-checkable list.
- **FR-004**: Rust modules MUST correspond by name to upstream modules, and ported functions and types MUST keep upstream names (adapted to Rust casing) and relative definition order, except where a documented deviation is required by a Rust language constraint or by a higher-priority rule.
- **FR-005**: The system MUST maintain a machine-checkable correspondence record mapping each upstream public symbol to its Rust item(s) and marking each as generated, ported-by-hand, or out of scope, with the reason when applicable.
- **FR-006**: A sync workflow MUST report, for an upgrade between two upstream versions, only the changed, added and removed upstream symbols, each paired with its Rust counterpart(s) and a before/after diff limited to that symbol; comment-, docstring-, formatting- and import-order-only changes MUST NOT be reported. The report MUST be deterministic and available in machine-readable and compact human-readable forms.
- **FR-007**: Mechanical layers (types, converters, blocking wrappers, golden fixtures, parity table) MUST remain generated from the upstream SDK, reproducibly, and the existing regenerate-and-diff check MUST pass. Generated files MUST NOT be hand edited.
- **FR-008**: Every upstream test that is not Vertex-AI-specific MUST have a Rust counterpart, or be listed as excluded with a reason; the mapping MUST be machine-checkable and the check MUST fail for any upstream test that is neither mapped nor excluded.
- **FR-009**: Each ported test MUST be traceable to its upstream test (by name or identifier), and its failure message MUST identify that origin.
- **FR-010**: Tests requiring network access and credentials MUST be opt-in and MUST NOT run in the default test command; all other ported tests MUST run offline.
- **FR-011**: Pure-logic helpers (transformers and similar side-effect-free functions) MUST be verified against the real upstream SDK as an oracle over shared cases, or be generated; any helper that is neither MUST be marked hand-written with a reason.
- **FR-012**: The Rust code MUST comply with the Rust best-practice rules (Apollo GraphQL handbook based): typed library errors (no `anyhow`, no `Box<dyn Error>` in public API), no `unwrap`/`expect`/`panic!` in production code (except recorded generated-converter invariants), documented public items, named constants for identifier-like strings, no silent discarding of errors, locally justified lint suppressions only, and the quality gates (format, clippy with warnings denied across all targets and features, tests, doc build with warnings denied, generated-sync) MUST pass.
- **FR-013**: Where mirroring upstream structure conflicts with a `rust.md` rule, upstream correspondence wins, and the deviation MUST be recorded with its reason.
- **FR-014**: API keys, request bodies and response bodies containing user content MUST NOT be logged.
- **FR-015**: Because public paths and names may change, the release MUST be a breaking version and the changelog MUST include a migration table mapping every renamed or relocated public item from its old path to its new path.
- **FR-016**: The sync procedure documentation (English and Japanese variants, per repository convention) and `CONTRIBUTING.md` MUST describe the new workflow end to end and remain consistent with `AGENTS.md`.
- **FR-017**: The work MUST start from `main`, MUST NOT modify git history, and MUST NOT delete the stashed or in-progress files of the discarded 007 effort.

### Key Entities

- **Upstream symbol**: a public module, class, method, module-level function or constant of the target upstream SDK.
- **Correspondence record**: links one upstream symbol to one or more Rust items with a kind (generated / hand-ported / out of scope), the upstream version last reconciled, and an optional reason or deviation note.
- **Sync report**: output of one comparison between two upstream versions listing changed, added, removed and unmapped symbols.
- **Upstream test**: a test function in the upstream repository's test suite at the target release.
- **Test mapping**: links one upstream test to a Rust test, or to an exclusion with a reason.
- **Out-of-scope record**: an upstream capability or test not ported, with its justification.
- **Oracle case corpus**: shared inputs executed against upstream and Rust to compare outputs.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of public upstream symbols of the target version are mapped or listed out of scope with a reason; the parity check fails otherwise.
- **SC-002**: 100% of upstream tests of the target version are mapped to a passing Rust test or listed excluded with a reason; 0 non-Vertex tests are excluded for convenience (only the reasons allowed by FR-003).
- **SC-003**: For the 2.22.x → 2.23.0 transition, the sync report contains 100% of symbols known to have changed and 0 symbols that changed only in comments or formatting.
- **SC-004**: The text a maintainer must read to perform a sync (report plus the Rust items it names) is at most 25% of the upstream-plus-Rust source read for the 2.23.0 sync; measured once on that transition and recorded.
- **SC-005**: A deliberately introduced behavioral divergence in at least 5 sampled covered helpers or methods is caught by the default offline test run in 100% of samples, with the failure naming the upstream test or case.
- **SC-006**: All quality gates pass on the final tree, with 0 new `unwrap`/`expect`/`panic!` in production code and 0 unjustified lint suppressions.
- **SC-007**: At least 90% of pure-logic helpers (by count) are generated or oracle-tested; the remainder are listed with reasons.

## Assumptions

- The upstream test suite is available from the upstream source repository at the tag of the target release (it is not shipped in the published package); acquiring it is a plan-time task.
- "Upstream tests that are not Vertex-AI-specific" are determined per test case; tests parameterized over both backends count once for the Developer API variant.
- Existing generators and generated directories are reused; generator changes are allowed, hand edits to generated output are not.
- The crate keeps its typed Rust API; upstream dynamic Python structures are mirrored by name and order, not transliterated (rule 2 and `AGENTS.md` conventions still apply).
- Python-ecosystem-only features (pydantic helpers, local tokenizer, replay client machinery) are out of scope unless they affect request/response behavior against the Gemini Developer API.
- The release version bump is the next breaking version of the 0.x series, decided at plan time.
- Previous in-progress work (007) is stashed and ignored; nothing from it is assumed.
