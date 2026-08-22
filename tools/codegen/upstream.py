"""Single source of truth for the upstream `google-genai` version this
port is generated from.

`google-genai` ships frequently, and every regeneration must be traceable
to one exact upstream release: the generated type/converter surface is a
1:1 mechanical port, so a header claiming 2.19.0 while the interpreter has
2.24.0 installed would silently produce code that matches neither.

Everything that stamps or checks a version reads it from here:

- `gen_types.py` / `gen_converters.py` — the `// @generated ... from
  google-genai <version>` header on every generated file.
- `tools/codegen/requirements.in` — the human-edited pin, and
  `tools/codegen/requirements.txt` — the lock generated from it with
  `--generate-hashes`. Both are checked; `assert_supported_version()`
  catches any drift at generation time rather than letting it reach the
  generated output.

## Upgrading to a new upstream release

1. Bump `PINNED_VERSION` here and the `google-genai==` pin in
   `tools/codegen/requirements.in` to the same value, then relock:
   `uv pip compile tools/codegen/requirements.in --generate-hashes
   --python-version 3.12 -o tools/codegen/requirements.txt`
2. `pip install --require-hashes -r tools/codegen/requirements.txt`
3. `python tools/codegen/generate.py`
4. `cargo check --all-features` — new or renamed `t_*` transformers show up
   here as missing-function errors; add them to `src/transformers.rs`.
5. `cargo test --all-features` — the golden converter fixtures
   (`tests/fixtures/converters/`) are regenerated from the new SDK in step
   3, so a behavioural change upstream surfaces as a diff there.
6. Update `CHANGELOG.md` with the new upstream version.
"""

from __future__ import annotations

# The upstream release this port is generated from and verified against.
# Keep in sync with the `google-genai==` pin in requirements.txt.
PINNED_VERSION = "2.19.0"


def installed_version() -> str:
    """Returns the `google-genai` version actually importable right now."""
    import google.genai  # noqa: PLC0415

    return str(google.genai.__version__)


def assert_supported_version() -> str:
    """Returns the upstream version to stamp into generated files, after
    verifying the installed SDK matches [`PINNED_VERSION`].

    Raises `SystemExit` on a mismatch: generating from an unpinned version
    would stamp a header that misrepresents what the output was derived
    from. Set `GENAI_ALLOW_VERSION_DRIFT=1` to stamp the *installed*
    version anyway, which is the intended path while evaluating an
    upgrade (see this module's docstring).
    """
    import os  # noqa: PLC0415
    import sys  # noqa: PLC0415

    found = installed_version()
    if found == PINNED_VERSION:
        return found

    if os.environ.get("GENAI_ALLOW_VERSION_DRIFT") == "1":
        print(
            f"warning: generating from google-genai {found}, but "
            f"upstream.py pins {PINNED_VERSION} (GENAI_ALLOW_VERSION_DRIFT=1)",
            file=sys.stderr,
        )
        return found

    raise SystemExit(
        f"google-genai {found} is installed, but this port is pinned to "
        f"{PINNED_VERSION}.\n"
        f"  - to regenerate against the pinned release:\n"
        f"      pip install 'google-genai=={PINNED_VERSION}'\n"
        f"  - to evaluate an upgrade to {found}, follow the steps in\n"
        f"    tools/codegen/upstream.py's module docstring, or set\n"
        f"      GENAI_ALLOW_VERSION_DRIFT=1\n"
        f"    to stamp {found} into the generated headers for a trial run."
    )


# Every place the pinned version is repeated, and how to find it there.
# Keeping these in sync by hand is exactly the kind of thing that rots
# silently, so `assert_all_in_sync()` checks them on every generation run
# (and therefore in CI's `codegen-check` job).
_VERSION_SITES: tuple[tuple[str, str], ...] = (
    # The human-edited declaration. `requirements.txt` beside it is the
    # generated lock; its own pin is checked below with a pattern that tolerates
    # the trailing ` \` continuation `--generate-hashes` emits.
    ("tools/codegen/requirements.in", r"^google-genai==(?P<version>[\w.]+)\s*$"),
    (
        "tools/codegen/requirements.txt",
        r"^google-genai==(?P<version>[\w.]+)\s*(?:\\\s*)?$",
    ),
    (
        "Cargo.toml",
        r"\[package\.metadata\.upstream\][^\[]*?version\s*=\s*\"(?P<version>[\w.]+)\"",
    ),
    (
        "src/lib.rs",
        r"pub const UPSTREAM_GENAI_VERSION:\s*&str\s*=\s*\"(?P<version>[\w.]+)\"",
    ),
)


def assert_all_in_sync() -> None:
    """Verifies every file that repeats the pinned upstream version agrees
    with [`PINNED_VERSION`].

    Raises `SystemExit` listing each mismatch. Called by `generate.py`, so a
    half-finished version bump fails the `codegen-check` CI job instead of
    shipping generated files whose headers contradict `Cargo.toml`.
    """
    import pathlib  # noqa: PLC0415
    import re  # noqa: PLC0415

    repo_root = pathlib.Path(__file__).resolve().parents[2]
    problems: list[str] = []
    for relative_path, pattern in _VERSION_SITES:
        path = repo_root / relative_path
        if not path.exists():
            problems.append(f"{relative_path}: file not found")
            continue
        match = re.search(pattern, path.read_text(encoding="utf-8"), re.MULTILINE | re.DOTALL)
        if match is None:
            problems.append(f"{relative_path}: no version declaration matched {pattern!r}")
        elif match.group("version") != PINNED_VERSION:
            problems.append(
                f"{relative_path}: declares {match.group('version')}, "
                f"but upstream.py pins {PINNED_VERSION}"
            )

    if problems:
        raise SystemExit(
            "upstream version is out of sync across the repo:\n  "
            + "\n  ".join(problems)
            + f"\n\nSet every site above to {PINNED_VERSION}, or change "
            "PINNED_VERSION in tools/codegen/upstream.py."
        )
