"""Unit tests for check_upstream_tests.py."""

import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

import check_upstream_tests as c  # noqa: E402
import upstream_tests as u  # noqa: E402


def _test(name: str = "test_a", file: str = "m/test_x.py", case: str = "") -> u.UpstreamTest:
    return u.UpstreamTest(file, name, case, 1, False)


def _row(test: u.UpstreamTest, **fields) -> dict:
    base = {"file": test.file, "name": test.name, "case": test.case, "status": "pending", "rust": "", "reason_code": "", "reason": ""}
    return {**base, **fields}


class CheckTests(unittest.TestCase):
    def test_missing_marker_fails(self):
        t = _test()
        problems = c.check([_row(t, status="mapped", rust="tests/m/x.rs::test_a")], [t], {}, {}, False)
        self.assertTrue(any("no `// upstream-test:` marker" in p for p in problems))

    def test_excluded_without_reason_fails(self):
        t = _test()
        problems = c.check([_row(t, status="excluded")], [t], {}, {}, False)
        self.assertTrue(any("excluded without a valid reason" in p for p in problems))

    def test_new_upstream_test_is_pending(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "inv.toml"
            c.update(path, [_test()], [], prune=False)
            rows = c._load(path)
        self.assertEqual(rows[0]["status"], "pending")

    def test_final_fails_with_pending(self):
        t = _test()
        self.assertEqual(c.check([_row(t)], [t], {}, {}, False), [])
        self.assertTrue(any(p.startswith("pending:") for p in c.check([_row(t)], [t], {}, {}, True)))

    def test_orphan_marker_fails(self):
        t = _test()
        problems = c.check([_row(t, status="excluded", reason_code="vertex", reason="r")], [t], {"m/test_x.py::test_other": ["tests/m/x.rs"]}, {}, False)
        self.assertTrue(any(p.startswith("orphan marker") for p in problems))

    def test_update_preserves_hand_edited_fields(self):
        t = _test()
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "inv.toml"
            c.update(path, [t], [], prune=False)
            text = path.read_text().replace('status = "pending"', 'status = "excluded"').replace('reason_code = ""', 'reason_code = "vertex"').replace('reason = ""', 'reason = "kept"')
            path.write_text(text)
            c.update(path, [t], [], prune=False)
            rows = c._load(path)
        self.assertEqual((rows[0]["status"], rows[0]["reason"]), ("excluded", "kept"))


if __name__ == "__main__":
    unittest.main()
