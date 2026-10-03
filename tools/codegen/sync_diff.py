"""Prints only what changed between two upstream releases, paired with the Rust
counterpart recorded in `ledger.toml`.

    sync_diff.py --from 2.27.0 [--to 2.28.0] [--format text|json]

Symbols whose fingerprint is equal are never listed, so comment-, docstring-,
format- and import-order-only changes cost nothing to read. See
specs/008-python-genai-rust-conformance/contracts/sync-report.md.
"""

from __future__ import annotations

import argparse
import difflib
import json
import pathlib
import sys
import tomllib
from collections.abc import Callable

import symbols as sym
import upstream
import upstream_src

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
LEDGER_PATH = REPO_ROOT / "tools" / "codegen" / "ledger.toml"
PACKAGE_SUBDIR = pathlib.Path("google") / "genai"
EXIT_RESOLUTION_FAILURE = 2
DIFF_CONTEXT_LINES = 3

CHANGE_CHANGED = "changed"
CHANGE_ADDED = "added"
CHANGE_REMOVED = "removed"

Key = tuple[str, str]


def _package_dir(root: pathlib.Path) -> pathlib.Path:
    return root / PACKAGE_SUBDIR


def _load_ledger(path: pathlib.Path) -> dict[Key, dict]:
    if not path.exists():
        return {}
    with path.open("rb") as handle:
        rows = tomllib.load(handle).get("symbol", [])
    return {(r["module"], r["qualname"]): r for r in rows}


def _unified(old: str, new: str, label: str) -> str:
    lines = difflib.unified_diff(
        old.splitlines(),
        new.splitlines(),
        fromfile=f"a/{label}",
        tofile=f"b/{label}",
        lineterm="",
        n=DIFF_CONTEXT_LINES,
    )
    return "\n".join(lines)


def diff_versions(
    old: str,
    new: str,
    source_resolver: Callable[[str], pathlib.Path] = upstream_src.wheel_source,
    ledger_path: pathlib.Path = LEDGER_PATH,
) -> dict:
    """Returns the sync report between two upstream versions."""
    old_syms = {(s.module, s.qualname): s for s in sym.extract_symbols(_package_dir(source_resolver(old)))}
    new_syms = {(s.module, s.qualname): s for s in sym.extract_symbols(_package_dir(source_resolver(new)))}
    ledger = _load_ledger(ledger_path)
    entries: list[dict] = []

    def entry(change: str, key: Key, diff: str) -> dict:
        row = ledger.get(key, {})
        return {
            "change": change,
            "module": key[0],
            "qualname": key[1],
            "rust": row.get("rust", ""),
            "status": row.get("status", "unmapped"),
            "diff": diff,
        }

    for key, symbol in new_syms.items():
        label = f"{key[0]}.py::{key[1]}"
        previous = old_syms.get(key)
        if previous is None:
            entries.append(entry(CHANGE_ADDED, key, _unified("", symbol.source, label)))
        elif previous.fingerprint != symbol.fingerprint:
            entries.append(entry(CHANGE_CHANGED, key, _unified(previous.source, symbol.source, label)))
    for key, symbol in old_syms.items():
        if key not in new_syms:
            entries.append(entry(CHANGE_REMOVED, key, _unified(symbol.source, "", f"{key[0]}.py::{key[1]}")))

    counts = {
        CHANGE_CHANGED: sum(e["change"] == CHANGE_CHANGED for e in entries),
        CHANGE_ADDED: sum(e["change"] == CHANGE_ADDED for e in entries),
        CHANGE_REMOVED: sum(e["change"] == CHANGE_REMOVED for e in entries),
        "unmapped": sum(e["status"] == "unmapped" for e in entries),
    }
    return {"from": old, "to": new, "counts": counts, "entries": entries}


def render_text(report: dict) -> str:
    """Renders the report as one block per entry plus a summary line."""
    blocks: list[str] = []
    for item in report["entries"]:
        head = f"{item['change'].upper()} {item['module']}::{item['qualname']}"
        target = item["rust"] or "-"
        blocks.append(f"{head} -> {target} [{item['status']}]\n{item['diff']}")
    c = report["counts"]
    blocks.append(f"changed={c['changed']} added={c['added']} removed={c['removed']} unmapped={c['unmapped']}")
    return "\n\n".join(blocks)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--from", dest="old", required=True)
    parser.add_argument("--to", dest="new", default=upstream.PINNED_VERSION)
    parser.add_argument("--format", choices=("text", "json"), default="text")
    args = parser.parse_args()
    try:
        report = diff_versions(args.old, args.new)
    except Exception as exc:  # noqa: BLE001 -- any resolution failure must print nothing else
        print(f"sync_diff.py: cannot resolve sources: {exc}", file=sys.stderr)
        sys.exit(EXIT_RESOLUTION_FAILURE)
    if args.format == "json":
        print(json.dumps(report, indent=2, ensure_ascii=False, sort_keys=False))
    else:
        print(render_text(report))


if __name__ == "__main__":
    main()
