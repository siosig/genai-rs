"""Builds `tools/codegen/ledger.toml`: every upstream symbol, its fingerprint,
and the Rust item that ports it.

The Rust counterpart is *derived* from naming rules (see
specs/008-python-genai-rust-conformance/contracts/ledger.md), never listed by
hand, so it cannot drift. What the rules cannot place needs an entry in
`deviations.toml` with a reason; otherwise the symbol is `unmapped` and
`check_ledger.py` fails.
"""

from __future__ import annotations

import ast
import glob
import json
import pathlib
import re
import sys
import tomllib

import symbols as sym
import upstream

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
TOOLS_DIR = REPO_ROOT / "tools" / "codegen"
LEDGER_PATH = TOOLS_DIR / "ledger.toml"
MODULE_MAP_PATH = TOOLS_DIR / "module_map.toml"
DEVIATIONS_PATH = TOOLS_DIR / "deviations.toml"
CONVERTERS_DIR = "src/converters/generated"
TYPES_GENERATED = "src/types/generated"

STATUS_PORTED = "ported"
STATUS_GENERATED = "generated"
STATUS_MERGED = "merged"
STATUS_OUT_OF_SCOPE = "out_of_scope"
STATUS_UNMAPPED = "unmapped"

CONVERTER_RE = re.compile(r"^_+(?P<type>.+)_(?P<dir>to|from)_(?P<backend>mldev|vertex)$")
SNAKE_RE_1 = re.compile(r"(.)([A-Z][a-z]+)")
SNAKE_RE_2 = re.compile(r"([a-z0-9])([A-Z])")

BACKEND_VARIANT_RE = re.compile(r"^_(?P<name>[a-z0-9_]+?)_(?P<backend>mldev|vertex)$")
ASYNC_PREFIX = "Async"

PY_ONLY_DECORATORS = (
    "field_validator",
    "model_validator",
    "field_serializer",
    "model_serializer",
    "computed_field",
    "pydantic.",
    "overload",
)


def _async_twin(symbol: sym.Symbol, all_names: set[tuple[str, str]]) -> str | None:
    """Returns the sync twin of an `Async<X>` class or method, if one exists.

    Python ships a sync and an async copy of each resource; Rust has one async
    API plus generated blocking wrappers, so the async copy is merged.
    """
    head, _, tail = symbol.qualname.partition(".")
    if not head.startswith(ASYNC_PREFIX) or head == ASYNC_PREFIX:
        return None
    twin = head[len(ASYNC_PREFIX) :]
    if (symbol.module, twin) not in all_names:
        return None
    return f"{twin}.{tail}" if tail else twin


def to_snake(name: str) -> str:
    """snake_case of a Python or Rust-style identifier, minus leading `_`."""
    name = name.lstrip("_")
    name = SNAKE_RE_1.sub(r"\1_\2", name)
    return SNAKE_RE_2.sub(r"\1_\2", name).lower()


def to_pascal(name: str) -> str:
    """PascalCase class name minus leading `_`."""
    return name.lstrip("_")


def _load_toml(path: pathlib.Path) -> dict:
    if not path.exists():
        return {}
    with path.open("rb") as handle:
        return tomllib.load(handle)


class RustIndex:
    """Lazy, cached text of repo-relative Rust files and globs."""

    def __init__(self, root: pathlib.Path) -> None:
        self.root = root
        self._cache: dict[str, str] = {}

    def expand(self, patterns: list[str]) -> list[str]:
        out: list[str] = []
        for pattern in patterns:
            matches = glob.glob(str(self.root / pattern), recursive=True)
            out.extend(
                sorted(
                    str(pathlib.Path(m).relative_to(self.root))
                    for m in matches
                    if m.endswith(".rs")
                )
            )
        return out

    def text(self, rel: str) -> str:
        if rel not in self._cache:
            self._cache[rel] = (self.root / rel).read_text(encoding="utf-8")
        return self._cache[rel]

    def find(self, files: list[str], regex: str) -> str | None:
        """Returns the first file in `files` matching `regex`, else None."""
        pattern = re.compile(regex, re.MULTILINE)
        for rel in files:
            if pattern.search(self.text(rel)):
                return rel
        return None


def _rust_regex(kind: str, qualname: str) -> tuple[str, str]:
    """Returns (display name, regex) for the Rust counterpart of a symbol."""
    last = qualname.split(".")[-1]
    if kind in (sym.KIND_FUNCTION, sym.KIND_METHOD):
        name = to_snake(last)
        return name, rf"\bfn\s+{re.escape(name)}\b"
    if kind == sym.KIND_CLASS:
        name = to_pascal(last)
        return name, rf"\b(?:struct|enum|trait|type)\s+{re.escape(name)}\b"
    name = last.lstrip("_")
    return name, rf"\b(?:const|static)\s+{re.escape(name)}\b"


def _python_only_reason(symbol: sym.Symbol, node_source: str) -> str | None:
    """Auto-classifies symbols that exist only because Python needs them."""
    last = symbol.qualname.split(".")[-1]
    if symbol.kind == sym.KIND_METHOD and last.startswith("__") and last.endswith("__"):
        return "Python dunder method; no Rust counterpart is needed."
    if symbol.kind == sym.KIND_CLASS and "TypedDict" in node_source.split(":", 1)[0]:
        return "TypedDict variant of a pydantic model; Rust uses the struct itself."
    if symbol.kind == sym.KIND_METHOD and any(d in node_source.split("def ", 1)[0] for d in PY_ONLY_DECORATORS):
        return "pydantic validator/serializer hook; serde covers this in Rust."
    if symbol.module == "types" and symbol.kind == sym.KIND_METHOD and last.startswith("model_"):
        return "pydantic model_* hook; serde covers this in Rust."
    return None


def build_ledger(version: str, package_dir: pathlib.Path, root: pathlib.Path = REPO_ROOT) -> list[dict]:
    """Returns the ledger entries (dicts) for the SDK sources at `package_dir`."""
    module_map = {m["upstream"]: m for m in _load_toml(MODULE_MAP_PATH).get("module", [])}
    exact_deviations: dict[tuple[str, str], dict] = {}
    pattern_deviations: list[tuple[re.Pattern[str], re.Pattern[str], dict]] = []
    for d in _load_toml(DEVIATIONS_PATH).get("deviation", []):
        if "qualname" in d:
            exact_deviations[(d["module"], d["qualname"])] = d
        else:
            pattern_deviations.append(
                (re.compile(d.get("module_regex", re.escape(d["module"]))), re.compile(d["qualname_regex"]), d)
            )

    def find_deviation(module: str, qualname: str) -> dict | None:
        found = exact_deviations.get((module, qualname))
        if found is not None:
            return found
        for module_re, qual_re, d in pattern_deviations:
            if module_re.fullmatch(module) and qual_re.fullmatch(qualname):
                return d
        return None
    rust = RustIndex(root)
    all_symbols = sym.extract_symbols(package_dir)
    by_class: dict[tuple[str, str], set[str]] = {}
    for s in all_symbols:
        if s.kind == sym.KIND_METHOD:
            cls, _, meth = s.qualname.partition(".")
            by_class.setdefault((s.module, cls), set()).add(meth)

    all_names = {(x.module, x.qualname) for x in all_symbols}
    converter_files = rust.expand([f"{CONVERTERS_DIR}/*.rs"])
    entries: list[dict] = []
    for s in all_symbols:
        entry = {
            "module": s.module,
            "qualname": s.qualname,
            "kind": s.kind,
            "public": s.public,
            "fingerprint": s.fingerprint,
            "status": STATUS_UNMAPPED,
            "rust": "",
            "reason_code": "",
            "reason": "",
        }
        entries.append(entry)
        mod = module_map.get(s.module)
        deviation = find_deviation(s.module, s.qualname)
        last = s.qualname.split(".")[-1]

        if deviation is not None:
            kind = deviation["kind"]
            entry["reason"] = deviation["reason"]
            entry["reason_code"] = deviation.get("reason_code", "")
            if kind == "not_ported":
                entry["status"] = STATUS_OUT_OF_SCOPE
            elif kind == "renamed":
                target_file, _, target_name = deviation["rust"].partition("::")
                exists = (root / target_file).is_file() and re.search(
                    rf"\b(?:fn|struct|enum|trait|type|const|static)\s+{re.escape(target_name)}\b",
                    rust.text(target_file),
                )
                entry["status"] = STATUS_PORTED if exists else STATUS_UNMAPPED
                entry["rust"] = deviation["rust"]
                if not exists:
                    entry["reason"] = f"stale deviation: {deviation['rust']} not found"
            elif kind == "merged":
                entry["status"] = STATUS_MERGED
                entry["rust"] = deviation.get("rust", "")
            elif kind == "generated":
                entry["status"] = STATUS_GENERATED
                entry["rust"] = deviation.get("rust", "")
            continue
        if mod is None:
            continue
        if mod.get("out_of_scope"):
            entry["status"] = STATUS_OUT_OF_SCOPE
            entry["reason_code"] = mod["reason_code"]
            entry["reason"] = mod["reason"]
            continue

        files = rust.expand(mod.get("rust", []))
        match = CONVERTER_RE.match(last) if s.kind == sym.KIND_FUNCTION else None
        if match and mod.get("converters"):
            if match["backend"] == "vertex":
                entry["status"] = STATUS_OUT_OF_SCOPE
                entry["reason_code"] = "vertex"
                entry["reason"] = "Vertex AI converter; Vertex AI is out of scope."
                continue
            name = f"{to_snake(match['type'])}_{match['dir']}_mldev"
            found = rust.find(converter_files, rf"\bfn\s+{re.escape(name)}\b")
            if found:
                entry["status"] = STATUS_GENERATED
                entry["rust"] = f"{found}::{name}"
                continue
        if s.module == "types" and s.kind == sym.KIND_CLASS:
            generated = rust.expand([f"{TYPES_GENERATED}/*.rs"])
            name = to_pascal(last)
            found = rust.find(generated, rf"\b(?:struct|enum)\s+{re.escape(name)}\b")
            if found:
                entry["status"] = STATUS_GENERATED
                entry["rust"] = f"{found}::{name}"
                continue

        source_head = s.source
        reason = _python_only_reason(s, source_head)
        if reason is not None:
            entry["status"] = STATUS_OUT_OF_SCOPE
            entry["reason_code"] = "python_only"
            entry["reason"] = reason
            continue

        if s.kind == sym.KIND_METHOD and last.startswith("_"):
            cls, _, meth = s.qualname.partition(".")
            siblings = by_class.get((s.module, cls), set())
            if meth.lstrip("_") in siblings:
                entry["status"] = STATUS_MERGED
                entry["rust"] = f"{cls}.{meth.lstrip('_')}"
                continue
            backend = BACKEND_VARIANT_RE.match(meth)
            if backend and backend["name"] in siblings:
                if backend["backend"] == "vertex":
                    entry["status"] = STATUS_OUT_OF_SCOPE
                    entry["reason_code"] = "vertex"
                    entry["reason"] = "Vertex AI variant of a public method; Vertex AI is out of scope."
                else:
                    entry["status"] = STATUS_MERGED
                    entry["rust"] = f"{cls}.{backend['name']}"
                continue

        if s.kind == sym.KIND_FUNCTION and last.endswith("_async") and (s.module, s.qualname[: -len("_async")]) in all_names:
            entry["status"] = STATUS_MERGED
            entry["rust"] = s.qualname[: -len("_async")]
            continue

        async_twin = _async_twin(s, all_names)
        if async_twin is not None:
            entry["status"] = STATUS_MERGED
            entry["rust"] = async_twin
            continue

        name, regex = _rust_regex(s.kind, s.qualname)
        found = rust.find(files, regex)
        if found:
            entry["status"] = STATUS_PORTED
            entry["rust"] = f"{found}::{name}"
    return entries


def _toml_value(value: object) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    return json.dumps(value, ensure_ascii=False)


def render(version: str, entries: list[dict]) -> str:
    """Renders the ledger as deterministic TOML (definition order preserved)."""
    lines = [
        "# @generated by tools/codegen/gen_ledger.py. DO NOT EDIT.",
        "# Change module_map.toml / deviations.toml, then re-run generate.py.",
        f"upstream_version = {json.dumps(version)}",
        "",
    ]
    fields = ["module", "qualname", "kind", "public", "fingerprint", "status", "rust", "reason_code", "reason"]
    for entry in entries:
        lines.append("[[symbol]]")
        lines.extend(f"{key} = {_toml_value(entry[key])}" for key in fields)
        lines.append("")
    return "\n".join(lines)


def main() -> None:
    upstream.assert_supported_python()
    version = upstream.assert_supported_version()
    import google.genai  # noqa: PLC0415

    package_dir = pathlib.Path(google.genai.__file__).parent
    entries = build_ledger(version, package_dir)
    LEDGER_PATH.write_text(render(version, entries), encoding="utf-8")
    unmapped = sum(1 for e in entries if e["status"] == STATUS_UNMAPPED)
    print(f"gen_ledger.py: wrote {len(entries)} symbols ({unmapped} unmapped)", file=sys.stderr)


if __name__ == "__main__":
    main()
