"""Writes the 0.3.x -> 0.4.0 migration table into CHANGELOG.md.

Reads `renames.toml` (one `[[rename]]` per moved public path) and rewrites the
Markdown table between the `<!-- migration:start -->` and
`<!-- migration:end -->` markers. Idempotent: a second run changes nothing.

Usage:
    python tools/codegen/gen_migration.py           # rewrite CHANGELOG.md
    python tools/codegen/gen_migration.py --check   # exit 1 if it is stale
"""

from __future__ import annotations

import argparse
import pathlib
import sys
import tomllib

TOOLS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[1]
RENAMES_PATH = TOOLS_DIR / "renames.toml"
CHANGELOG_PATH = REPO_ROOT / "CHANGELOG.md"
START_MARKER = "<!-- migration:start -->"
END_MARKER = "<!-- migration:end -->"
HEADER = ("Old path", "New path", "Note")


def _cell(text: str) -> str:
    return text.replace("|", "\\|")


def load_renames(path: pathlib.Path = RENAMES_PATH) -> list[dict[str, str]]:
    with path.open("rb") as handle:
        entries = tomllib.load(handle)["rename"]
    for entry in entries:
        if not entry.get("old") or not entry.get("new"):
            raise ValueError(f"rename entry without old/new: {entry}")
    return entries


def render_table(entries: list[dict[str, str]]) -> str:
    rows = [
        f"| {_cell(HEADER[0])} | {_cell(HEADER[1])} | {_cell(HEADER[2])} |",
        "|---|---|---|",
    ]
    rows.extend(
        f"| `{_cell(e['old'])}` | `{_cell(e['new'])}` | {_cell(e.get('note', ''))} |"
        for e in entries
    )
    return "\n".join(rows)


def splice(text: str, table: str) -> str:
    start = text.find(START_MARKER)
    end = text.find(END_MARKER)
    if start < 0 or end < 0 or end < start:
        raise ValueError(f"CHANGELOG.md needs {START_MARKER} ... {END_MARKER}")
    head = text[: start + len(START_MARKER)]
    tail = text[end:]
    return f"{head}\n\n{table}\n\n{tail}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail instead of writing")
    args = parser.parse_args()

    table = render_table(load_renames())
    current = CHANGELOG_PATH.read_text(encoding="utf-8")
    updated = splice(current, table)
    if updated == current:
        return 0
    if args.check:
        print("CHANGELOG.md migration table is stale; run tools/codegen/gen_migration.py", file=sys.stderr)
        return 1
    CHANGELOG_PATH.write_text(updated, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
