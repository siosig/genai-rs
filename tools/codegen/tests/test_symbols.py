"""Unit tests for symbols.py (run: python -m unittest discover tools/codegen/tests)."""

import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

import symbols  # noqa: E402


def _extract(source: str) -> list[symbols.Symbol]:
    with tempfile.TemporaryDirectory() as tmp:
        pkg = pathlib.Path(tmp)
        (pkg / "mod.py").write_text(source, encoding="utf-8")
        return symbols.extract_symbols(pkg)


def _fp(source: str, qualname: str) -> str:
    return next(s.fingerprint for s in _extract(source) if s.qualname == qualname)


class FingerprintTests(unittest.TestCase):
    def test_docstring_change_keeps_fingerprint(self):
        a = 'def f(x):\n    """one"""\n    return x\n'
        b = 'def f(x):\n    """two, longer"""\n    return x\n'
        self.assertEqual(_fp(a, "f"), _fp(b, "f"))

    def test_comment_and_blank_line_change_keeps_fingerprint(self):
        a = "def f(x):\n    return x\n"
        b = "def f(x):\n\n    # note\n    return x  # trailing\n"
        self.assertEqual(_fp(a, "f"), _fp(b, "f"))

    def test_import_reorder_keeps_fingerprint(self):
        a = "import os\nimport sys\n\ndef f():\n    return os.name\n"
        b = "import sys\nimport os\n\ndef f():\n    return os.name\n"
        self.assertEqual(_fp(a, "f"), _fp(b, "f"))

    def test_default_value_change_changes_fingerprint(self):
        a = "def f(x=1):\n    return x\n"
        b = "def f(x=2):\n    return x\n"
        self.assertNotEqual(_fp(a, "f"), _fp(b, "f"))

    def test_decorator_change_changes_fingerprint(self):
        a = "def f():\n    return 1\n"
        b = "@staticmethod\ndef f():\n    return 1\n"
        self.assertNotEqual(_fp(a, "f"), _fp(b, "f"))

    def test_symbol_order_is_file_order(self):
        source = "def zeta():\n    pass\n\ndef alpha():\n    pass\n\nclass K:\n    def m(self):\n        pass\n"
        names = [s.qualname for s in _extract(source)]
        self.assertEqual(names, ["zeta", "alpha", "K", "K.m"])

    def test_private_names_are_not_public(self):
        pub = {s.qualname: s.public for s in _extract("def a():\n    pass\n\ndef _b():\n    pass\n")}
        self.assertEqual(pub, {"a": True, "_b": False})

    def test_all_restricts_public_names(self):
        source = '__all__ = ["a"]\n\ndef a():\n    pass\n\ndef b():\n    pass\n'
        pub = {s.qualname: s.public for s in _extract(source)}
        self.assertEqual(pub, {"a": True, "b": False})


if __name__ == "__main__":
    unittest.main()
