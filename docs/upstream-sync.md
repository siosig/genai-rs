# Upstream sync status

Hand-maintained (unlike `parity.md`, this is not generated — update it by
hand whenever the upstream pin changes). Answers two questions a future
maintainer will have: "what upstream state are we actually pinned to?" and
"is there anything upstream we know about but haven't taken yet, and why?"

## Table of Contents

- [Current pin](#current-pin)
- [Deliberately deferred](#deliberately-deferred)
- [Permanently out of scope](#permanently-out-of-scope)

## Current pin

| Upstream version | Upstream ref | Synced on | Feature |
|---|---|---|---|
| 2.19.0 | `66807187` | 2026-08-22 | 001-port-genai-rust |
| 2.23.0 | `e384b55` (tag `v2.23.0`) | 2026-09-12 | 003-upstream-2-23-sync |

`tools/codegen/upstream.py`'s `PINNED_VERSION` is the single source of truth
this table restates; see that file's own header comment for the exact
mechanics of bumping it.

## Deliberately deferred

Upstream changes we know about but have not taken, with a concrete resume
condition — not "eventually", something you can actually check for.

| Upstream commit | Change | Why deferred | Resume when | User impact |
|---|---|---|---|---|
| `2580638` | Automatic function calling: stop running the caller's function once the remote-call budget is exhausted (previously it ran anyway, with no request left to send the result to) | Lands after the v2.23.0 tag; not on PyPI as of this sync (the newest published release is 2.23.0, and 2.24.0 is unreleased). Pinning past a PyPI release would mean pinning by commit SHA instead of version, which the codegen pipeline's hash-verified dependency lock (`tools/codegen/requirements.txt`, `--require-hashes`) isn't set up for | Upstream publishes 2.24.0 (or later) to PyPI | **Yes.** When the remote-call budget is exhausted, this crate still runs the registered function one time too many, with no request left to send the result to. If that function has side effects (billing, sending a message, writing a record), those side effects still happen. See `src/afc.rs` |

An empty table here would be the normal, desired state — it isn't evidence
the table was forgotten, it means nothing is currently being held back.

## Permanently out of scope

Not "not yet" — deliberately not ported, with the decision that settled it.

| Area | Why | Decided in |
|---|---|---|
| Vertex AI backend | This crate targets the Gemini Developer API only; asking for Vertex AI (`vertexai(true)`, `project`/`location`, `GOOGLE_GENAI_USE_VERTEXAI`) fails fast with `Error::UnsupportedBackend` | 001 |
| Interactions API / `_gaos` (environments, agents, webhooks, triggers) | A separate API surface upstream is actively growing (the bulk of the 2.19.0→2.23.0 diff lives here); left for a future feature if it's ever ported | 001 |
| `local_tokenizer` | Not ported | 001 |
| `_replay_api_client` | Upstream's own test-replay harness; this crate proves parity via its own golden-fixture mechanism instead (`tools/codegen/gen_fixtures.py`) | 003 |

"Permanent" isn't immutable — Interactions, in particular, could become
in-scope for a future feature. If that happens, move its row here into a
new "current pin" note explaining the change, don't just delete it.
