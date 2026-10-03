"""Maintains and checks `upstream_tests.toml`, the map from every upstream test
to a Rust test (`mapped`), an exclusion with a coded reason (`excluded`), or
`pending`. See specs/008-python-genai-rust-conformance/contracts/upstream-tests.md.

    check_upstream_tests.py --update     refresh from the upstream checkout
    check_upstream_tests.py              check (pending allowed)
    check_upstream_tests.py --final      check, and fail on any pending test
    check_upstream_tests.py --list --dir <dir>   print pending rows of a directory
    check_upstream_tests.py --report     counts per status / reason code
"""

from __future__ import annotations

import argparse
import collections
import fnmatch
import json
import pathlib
import re
import sys
import tomllib

import upstream
import upstream_src
import upstream_tests

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
TOOLS_DIR = REPO_ROOT / "tools" / "codegen"
INVENTORY_PATH = TOOLS_DIR / "upstream_tests.toml"
RULES_PATH = TOOLS_DIR / "upstream_tests_rules.toml"
RUST_TESTS_DIR = REPO_ROOT / "tests"
MARKER_PREFIX = "// upstream-test: "
TEST_ATTRS = ("#[test]", "#[tokio::test]", "#[rstest]", "#[tokio::test(")

STATUS_MAPPED = "mapped"
STATUS_EXCLUDED = "excluded"
STATUS_PENDING = "pending"
REASON_CODES = frozenset(
    {"vertex", "python_only", "replay_infra", "live_network", "pydantic_only", "duplicate_of"}
)
HAND_FIELDS = ("status", "rust", "reason_code", "reason")

Key = tuple[str, str, str]


def _load(path: pathlib.Path) -> list[dict]:
    if not path.exists():
        return []
    with path.open("rb") as handle:
        return tomllib.load(handle).get("test", [])


def _key(row: dict) -> Key:
    return (row["file"], row["name"], row["case"])


def classify(test: upstream_tests.UpstreamTest, rules: list[dict]) -> dict:
    """Returns the hand-editable fields a fresh `test` starts with."""
    label = test.name or test.case
    for rule in rules:
        if not fnmatch.fnmatch(test.file, rule["match_file"]):
            continue
        pattern = rule.get("match_name")
        if pattern and not re.search(pattern, label):
            continue
        return {
            "status": rule["status"],
            "rust": "",
            "reason_code": rule["reason_code"],
            "reason": rule["reason"],
        }
    return {"status": STATUS_PENDING, "rust": "", "reason_code": "", "reason": ""}


def update(path: pathlib.Path, inventory: list[upstream_tests.UpstreamTest], rules: list[dict], prune: bool) -> list[str]:
    """Rewrites the inventory, keeping hand-edited fields; returns notices."""
    existing = {_key(r): r for r in _load(path)}
    corpus = load_corpus()
    notices: list[str] = []
    rows: list[dict] = []
    seen: set[Key] = set()
    for test in inventory:
        key = (test.file, test.name, test.case)
        seen.add(key)
        base = existing.get(key) or {**classify(test, rules)}
        if key not in existing:
            notices.append(f"new: {test.file}::{test.name or test.case}")
        if test.case and base.get("status") == STATUS_PENDING and test.case in corpus.get(test.file, set()):
            stem = pathlib.PurePosixPath(test.file)
            base = {
                **base,
                "status": STATUS_MAPPED,
                "rust": f"tests/fixtures/upstream/{stem.parent.as_posix()}/{stem.stem.removeprefix('test_')}.json::{test.case}",
            }
        rows.append(
            {
                "file": test.file,
                "name": test.name,
                "case": test.case,
                **{field: base.get(field, "") for field in HAND_FIELDS},
            }
        )
    for key in existing.keys() - seen:
        notices.append(f"removed: {key[0]}::{key[1] or key[2]}")
        if not prune:
            rows.append(existing[key])
    path.write_text(render(rows), encoding="utf-8")
    return notices


def render(rows: list[dict]) -> str:
    lines = [
        "# Upstream test coverage map. Generated skeleton; the fields status/rust/",
        "# reason_code/reason are hand-edited and preserved by --update.",
        "# See specs/008-python-genai-rust-conformance/contracts/upstream-tests.md.",
        f"upstream_version = {json.dumps(upstream.PINNED_VERSION)}",
        "",
    ]
    for row in rows:
        lines.append("[[test]]")
        for field in ("file", "name", "case", *HAND_FIELDS):
            lines.append(f"{field} = {json.dumps(row.get(field, ''), ensure_ascii=False)}")
        lines.append("")
    return "\n".join(lines)


def rust_markers(tests_dir: pathlib.Path) -> dict[str, list[str]]:
    """Maps `<file>::<name>` marker text to the Rust files that carry it."""
    found: dict[str, list[str]] = collections.defaultdict(list)
    for path in sorted(tests_dir.rglob("*.rs")):
        lines = path.read_text(encoding="utf-8").splitlines()
        for index, line in enumerate(lines):
            stripped = line.strip()
            if not stripped.startswith(MARKER_PREFIX):
                continue
            window = [item.strip() for item in lines[index + 1 : index + 4]]
            if not any(item.startswith(TEST_ATTRS) for item in window):
                continue
            found[stripped[len(MARKER_PREFIX) :].strip()].append(path.relative_to(REPO_ROOT).as_posix())
    return found


def check(rows: list[dict], inventory: list[upstream_tests.UpstreamTest], markers: dict[str, list[str]], corpus: dict[str, set[str]], final: bool) -> list[str]:
    problems: list[str] = []
    wanted = {(t.file, t.name, t.case) for t in inventory}
    have = {_key(r) for r in rows}
    for key in sorted(wanted - have):
        problems.append(f"new: {key[0]}::{key[1] or key[2]}")
    for key in sorted(have - wanted):
        problems.append(f"removed: {key[0]}::{key[1] or key[2]}")

    used_markers: set[str] = set()
    for row in rows:
        label = f"{row['file']}::{row['name'] or row['case']}"
        status = row["status"]
        if status == STATUS_MAPPED:
            if row["name"] or row["rust"].endswith(".rs") or ".rs::" in row["rust"]:
                if label not in markers:
                    problems.append(f"mapped but no `// upstream-test:` marker: {label}")
                else:
                    used_markers.add(label)
                    if row["rust"] and row["rust"].split("::")[0] not in markers[label]:
                        problems.append(f"marker for {label} is not in {row['rust']}")
            else:
                source = row["file"]
                if row["case"] not in corpus.get(source, set()):
                    problems.append(f"mapped table case missing from corpus: {label}")
        elif status == STATUS_EXCLUDED:
            if row["reason_code"] not in REASON_CODES or not row["reason"]:
                problems.append(f"excluded without a valid reason: {label}")
        elif status == STATUS_PENDING:
            if final:
                problems.append(f"pending: {label}")
        else:
            problems.append(f"unknown status {status!r}: {label}")
    for marker in sorted(set(markers) - used_markers):
        problems.append(f"orphan marker (no mapped row): {marker}")
    return problems


def load_corpus() -> dict[str, set[str]]:
    """Maps each upstream test file to the case names in its oracle corpus."""
    out: dict[str, set[str]] = {}
    root = REPO_ROOT / "tests" / "fixtures" / "upstream"
    for path in sorted(root.rglob("*.json")):
        data = json.loads(path.read_text(encoding="utf-8"))
        if isinstance(data, dict) and "cases" in data:
            out[data["source"]] = {case["name"] for case in data["cases"]}
    return out


def report(rows: list[dict]) -> str:
    status = collections.Counter(r["status"] for r in rows)
    codes = collections.Counter(r["reason_code"] for r in rows if r["status"] == STATUS_EXCLUDED)
    parts = [f"total={len(rows)}"] + [f"{k}={v}" for k, v in sorted(status.items())]
    lines = [" ".join(parts)]
    lines += [f"  excluded[{k}]={v}" for k, v in sorted(codes.items())]
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--update", action="store_true")
    parser.add_argument("--prune", action="store_true")
    parser.add_argument("--final", action="store_true")
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--dir", default="")
    parser.add_argument("--report", action="store_true")
    args = parser.parse_args()

    checkout = upstream_src.ensure_checkout(upstream.PINNED_VERSION)
    inventory = upstream_tests.inventory(checkout)
    rules = tomllib.loads(RULES_PATH.read_text(encoding="utf-8")).get("rule", [])

    if args.update:
        for notice in update(INVENTORY_PATH, inventory, rules, args.prune):
            print(notice, file=sys.stderr)
    rows = _load(INVENTORY_PATH)

    if args.list:
        for row in rows:
            if row["status"] == STATUS_PENDING and row["file"].startswith(args.dir):
                print(f"{row['file']}::{row['name'] or row['case']}")
        return 0

    problems = check(rows, inventory, rust_markers(RUST_TESTS_DIR), load_corpus(), args.final)
    for problem in problems[:300]:
        print(problem, file=sys.stderr)
    if len(problems) > 300:
        print(f"... and {len(problems) - 300} more", file=sys.stderr)
    print(report(rows))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
