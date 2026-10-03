"""Extracts and fingerprints the symbols of an upstream `google.genai` tree.

A symbol is a module-level function, class, upper-case constant, or a method of
a module-level class. Its fingerprint is a hash of the normalized AST, so a
change to comments, docstrings, whitespace or import order never counts, while
a change to a default value, decorator, annotation or body does.

`_gaos/` (a Speakeasy-generated sub-SDK) and `tests/` are not part of the
ledger; `gen_gaos.py` covers the former, `upstream_tests.py` the latter.
"""

from __future__ import annotations

import ast
import dataclasses
import hashlib
import pathlib

FINGERPRINT_LENGTH = 16
EXCLUDED_TOP_LEVEL_DIRS = frozenset({"_gaos", "tests", "__pycache__"})

KIND_FUNCTION = "function"
KIND_CLASS = "class"
KIND_METHOD = "method"
KIND_CONST = "const"


@dataclasses.dataclass(frozen=True)
class Symbol:
    """One upstream symbol with its normalized-AST fingerprint."""

    module: str
    qualname: str
    kind: str
    public: bool
    fingerprint: str
    lineno: int
    source: str


class _DocstringStripper(ast.NodeTransformer):
    """Removes the leading docstring of modules, classes and functions."""

    def _strip(self, node: ast.AST) -> ast.AST:
        self.generic_visit(node)
        body = getattr(node, "body", None)
        if (
            body
            and isinstance(body[0], ast.Expr)
            and isinstance(body[0].value, ast.Constant)
            and isinstance(body[0].value.value, str)
        ):
            node.body = body[1:] or [ast.Pass()]
        return node

    visit_Module = _strip
    visit_ClassDef = _strip
    visit_FunctionDef = _strip
    visit_AsyncFunctionDef = _strip


def normalize(node: ast.AST) -> str:
    """Returns comment-, docstring- and format-independent source for `node`."""
    stripped = _DocstringStripper().visit(ast.fix_missing_locations(_copy(node)))
    return ast.unparse(stripped)


def _copy(node: ast.AST) -> ast.AST:
    return ast.parse(ast.unparse(node)).body[0]


def fingerprint(node: ast.AST) -> str:
    """Returns the 16-hex-digit fingerprint of `node`."""
    digest = hashlib.sha256(normalize(node).encode("utf-8")).hexdigest()
    return digest[:FINGERPRINT_LENGTH]


def _is_const_name(name: str) -> bool:
    stripped = name.lstrip("_")
    return bool(stripped) and stripped.upper() == stripped and stripped[0].isalpha()


def _all_names(tree: ast.Module) -> set[str] | None:
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(
            isinstance(t, ast.Name) and t.id == "__all__" for t in node.targets
        ):
            try:
                return {str(item) for item in ast.literal_eval(node.value)}
            except (ValueError, SyntaxError):
                return None
    return None


def _module_symbols(module: str, tree: ast.Module) -> list[Symbol]:
    exported = _all_names(tree)
    module_private = module.split(".")[-1].startswith("_")
    out: list[Symbol] = []

    def emit(node: ast.AST, qualname: str, kind: str) -> None:
        top = qualname.split(".")[0]
        private = module_private or any(p.startswith("_") for p in qualname.split("."))
        if exported is not None and top not in exported:
            private = True
        out.append(
            Symbol(
                module=module,
                qualname=qualname,
                kind=kind,
                public=not private,
                fingerprint=fingerprint(node),
                lineno=node.lineno,
                source=normalize(node),
            )
        )

    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            emit(node, node.name, KIND_FUNCTION)
        elif isinstance(node, ast.ClassDef):
            emit(node, node.name, KIND_CLASS)
            for member in node.body:
                if isinstance(member, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    emit(member, f"{node.name}.{member.name}", KIND_METHOD)
        elif isinstance(node, (ast.Assign, ast.AnnAssign)):
            targets = node.targets if isinstance(node, ast.Assign) else [node.target]
            for target in targets:
                if isinstance(target, ast.Name) and _is_const_name(target.id):
                    emit(node, target.id, KIND_CONST)
    return out


def extract_symbols(package_dir: pathlib.Path) -> list[Symbol]:
    """Returns every symbol of the `google/genai` package at `package_dir`.

    Modules are sorted by name; symbols keep file order, so a reordering
    upstream is visible in the ledger.
    """
    files = sorted(
        path
        for path in package_dir.rglob("*.py")
        if path.relative_to(package_dir).parts[0] not in EXCLUDED_TOP_LEVEL_DIRS
        and "__pycache__" not in path.parts
    )
    symbols: list[Symbol] = []
    for path in files:
        rel = path.relative_to(package_dir).with_suffix("")
        module = ".".join(rel.parts)
        if module.endswith(".__init__"):
            module = module[: -len(".__init__")]
        tree = ast.parse(path.read_text(encoding="utf-8"))
        symbols.extend(_module_symbols(module, tree))
    return symbols
