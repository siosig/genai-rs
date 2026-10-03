#!/usr/bin/env python3
"""Single entry point for regenerating `src/**/generated/*.rs` from the
installed `google-genai` Python SDK.

Usage:
    python tools/codegen/generate.py [--only types,converters]

Each generator writes files under its own `OUT_DIR` (see `gen_types.py` /
`gen_converters.py`); this script's only extra job is running `cargo fmt`
on the crate afterward so regeneration is idempotent under `git diff
--exit-code` regardless of the generators' own output formatting.
"""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
TOOLS_DIR = REPO_ROOT / "tools" / "codegen"
sys.path.insert(0, str(TOOLS_DIR))

TARGETS = ("types", "converters", "fixtures", "cases", "gaos", "blocking", "parity", "ledger")


def run_types() -> None:
    import gen_types  # noqa: PLC0415

    gen_types.main()


def run_converters() -> None:
    import gen_converters  # noqa: PLC0415

    gen_converters.main()


def run_fixtures() -> None:
    import gen_fixtures  # noqa: PLC0415

    gen_fixtures.main()


def run_cases() -> None:
    # A subprocess: the case generator swaps `google.genai` for the upstream
    # checkout (it needs upstream's `tests` package), which must not leak into
    # the other targets of this process.
    subprocess.run(
        [sys.executable, str(TOOLS_DIR / "gen_upstream_cases.py")],
        cwd=TOOLS_DIR,
        check=True,
    )


def run_gaos() -> None:
    import gen_gaos  # noqa: PLC0415

    gen_gaos.main()


def run_blocking() -> None:
    import gen_blocking  # noqa: PLC0415

    gen_blocking.main()


def run_parity() -> None:
    import gen_parity  # noqa: PLC0415

    gen_parity.main()


def run_ledger() -> None:
    import gen_ledger  # noqa: PLC0415

    gen_ledger.main()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--only",
        default=",".join(TARGETS),
        help=f"comma-separated subset of {TARGETS} to regenerate (default: all)",
    )
    args = parser.parse_args()
    requested = [t.strip() for t in args.only.split(",") if t.strip()]
    unknown = [t for t in requested if t not in TARGETS]
    if unknown:
        parser.error(f"unknown --only target(s): {unknown}; choose from {TARGETS}")

    # Fails fast if requirements.txt / Cargo.toml / src/lib.rs disagree with
    # the pinned upstream version, or if this interpreter is not the pinned
    # one -- before any generated file is stamped. Both are inputs the output
    # depends on, and both fail silently if left unchecked.
    import upstream  # noqa: PLC0415

    upstream.assert_supported_python()
    upstream.assert_all_in_sync()

    if "types" in requested:
        run_types()
    if "converters" in requested:
        run_converters()
    if "fixtures" in requested:
        run_fixtures()
    if "cases" in requested:
        run_cases()
    if "gaos" in requested:
        run_gaos()
    if "blocking" in requested:
        run_blocking()
    if "parity" in requested:
        run_parity()
    if "ledger" in requested:
        run_ledger()

    fmt = subprocess.run(
        ["cargo", "fmt", "--all"],
        cwd=REPO_ROOT,
        check=False,
    )
    if fmt.returncode != 0:
        print("generate.py: `cargo fmt --all` failed; generated files may be unformatted", file=sys.stderr)
        sys.exit(fmt.returncode)


if __name__ == "__main__":
    main()
