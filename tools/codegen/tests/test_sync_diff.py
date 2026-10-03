"""Unit tests for sync_diff.py."""

import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

import sync_diff  # noqa: E402


def _make_roots(tmp: pathlib.Path, versions: dict[str, str]) -> dict[str, pathlib.Path]:
    roots = {}
    for version, source in versions.items():
        root = tmp / version
        (root / "google" / "genai").mkdir(parents=True)
        (root / "google" / "genai" / "mod.py").write_text(source, encoding="utf-8")
        roots[version] = root
    return roots


def _diff(old_src: str, new_src: str):
    with tempfile.TemporaryDirectory() as tmp:
        roots = _make_roots(pathlib.Path(tmp), {"1": old_src, "2": new_src})
        return sync_diff.diff_versions("1", "2", source_resolver=roots.__getitem__, ledger_path=pathlib.Path(tmp) / "none.toml")


class SyncDiffTests(unittest.TestCase):
    def test_docstring_only_change_is_not_reported(self):
        report = _diff('def f():\n    """a"""\n    return 1\n', 'def f():\n    """b"""\n    return 1\n')
        self.assertEqual(report["entries"], [])

    def test_changed_default_is_reported_with_diff_limited_to_symbol(self):
        report = _diff("def f(x=1):\n    return x\n\ndef g():\n    pass\n", "def f(x=2):\n    return x\n\ndef g():\n    pass\n")
        self.assertEqual([(e["change"], e["qualname"]) for e in report["entries"]], [("changed", "f")])
        self.assertIn("+def f(x=2):", report["entries"][0]["diff"])
        self.assertNotIn("def g", report["entries"][0]["diff"])

    def test_added_and_removed_symbols(self):
        report = _diff("def old():\n    pass\n", "def new():\n    pass\n")
        self.assertEqual(
            sorted((e["change"], e["qualname"]) for e in report["entries"]),
            [("added", "new"), ("removed", "old")],
        )
        self.assertEqual(report["counts"], {"changed": 0, "added": 1, "removed": 1, "unmapped": 2})

    def test_report_is_deterministic(self):
        first = _diff("def f(x=1):\n    return x\n", "def f(x=2):\n    return x\n")
        second = _diff("def f(x=1):\n    return x\n", "def f(x=2):\n    return x\n")
        self.assertEqual(sync_diff.render_text(first), sync_diff.render_text(second))

    def test_unresolvable_version_raises_before_any_output(self):
        def resolver(version: str) -> pathlib.Path:
            raise FileNotFoundError(version)

        with self.assertRaises(FileNotFoundError):
            sync_diff.diff_versions("1", "2", source_resolver=resolver)


if __name__ == "__main__":
    unittest.main()
