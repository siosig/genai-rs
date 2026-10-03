"""Fails when `ledger.toml` is stale or any upstream symbol is unmapped."""

from __future__ import annotations

import pathlib
import sys
import tomllib

import gen_ledger
import upstream

ALLOWED_REASON_CODES = frozenset(
    {"vertex", "python_only", "replay_infra", "structural", "not_applicable"}
)


def main() -> int:
    upstream.assert_supported_python()
    version = upstream.assert_supported_version()
    import google.genai  # noqa: PLC0415

    package_dir = pathlib.Path(google.genai.__file__).parent
    entries = gen_ledger.build_ledger(version, package_dir)
    problems: list[str] = []

    expected = gen_ledger.render(version, entries)
    actual = gen_ledger.LEDGER_PATH.read_text(encoding="utf-8") if gen_ledger.LEDGER_PATH.exists() else ""
    if expected != actual:
        problems.append("tools/codegen/ledger.toml is stale; run tools/codegen/generate.py --only ledger")

    for entry in entries:
        key = f"{entry['module']}::{entry['qualname']}"
        if entry["status"] == gen_ledger.STATUS_UNMAPPED:
            problems.append(f"unmapped: {key}")
        if entry["status"] == gen_ledger.STATUS_OUT_OF_SCOPE:
            if entry["reason_code"] not in ALLOWED_REASON_CODES or not entry["reason"]:
                problems.append(f"out_of_scope without a valid reason: {key}")
    for problem in problems[:200]:
        print(problem, file=sys.stderr)
    if len(problems) > 200:
        print(f"... and {len(problems) - 200} more", file=sys.stderr)
    print(f"check_ledger.py: {len(entries)} symbols, {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
