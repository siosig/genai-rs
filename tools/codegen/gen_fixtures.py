#!/usr/bin/env python3
"""Generates `tests/fixtures/converters/**/*.json` golden fixtures by
running each `fixtures_cases.py` case through the real installed
`google-genai` SDK's `_X_to_mldev`/`_X_from_mldev` converter functions.

See specs/001-port-genai-rust/contracts/codegen.md "gen_fixtures.py". DO NOT
hand-edit the generated output; edit `fixtures_cases.py` and re-run
`python tools/codegen/generate.py --only fixtures` instead.
"""

from __future__ import annotations

import importlib
import inspect
import json
import pathlib
import sys

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
TOOLS_DIR = pathlib.Path(__file__).resolve().parent
OUT_DIR = REPO_ROOT / "tests" / "fixtures" / "converters"

if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import fixtures_cases  # noqa: E402  (needs TOOLS_DIR on sys.path first)
import gen_converters as gc  # noqa: E402

# Every installed-SDK module that can define a `_X_to_mldev`/`_X_from_mldev`
# function, keyed the same way as `gen_converters.TARGET_FILES` (module
# stem -> importable `google.genai` submodule name).
SDK_MODULES = {
    module_name: pathlib.Path(filename).stem
    for module_name, filename in gc.TARGET_FILES.items()
}


class CaseError(Exception):
    """Raised when a `fixtures_cases.py` entry doesn't behave as declared
    (an unexpected exception, or a missing expected exception) -- a bug in
    the fixture's own input, not in the SDK under test."""


def resolve_converter(python_name: str):
    """Finds the callable `_X_to_mldev`/`_X_from_mldev` function object in
    the installed SDK by its exact Python name, searching every module
    `gen_converters.TARGET_FILES` knows about."""
    for stem in SDK_MODULES.values():
        try:
            mod = importlib.import_module(f"google.genai.{stem}")
        except ImportError:
            continue
        fn = getattr(mod, python_name, None)
        if fn is not None:
            return fn
    raise LookupError(
        f"gen_fixtures.py: no installed SDK module defines `{python_name}`"
        " (checked google.genai.{"
        + ", ".join(sorted(SDK_MODULES.values()))
        + "})"
    )


def normalize(value):
    """Mirrors the SDK's own request-body normalization -- every SDK method
    (see e.g. `models.py`'s `generate_content`/`embed_content`) calls
    `_common.convert_to_dict()` then `_common.encode_unserializable_types()`
    on a `_to_mldev` converter's return value before JSON-serializing it as
    the request body. Some converters (e.g.
    `_EmbedContentParametersPrivate_to_mldev`) intentionally leave a nested
    `pydantic.BaseModel` un-flattened, relying on this later step -- so a
    fixture's `expected` value has to go through the same normalization to
    match the plain-JSON shape the Rust converters (which never leave a
    model object un-flattened) produce."""
    from google.genai import _common  # noqa: PLC0415

    return _common.encode_unserializable_types(_common.convert_to_dict(value))


def run_case(case: dict, client) -> dict:
    fn = resolve_converter(case["converter"])
    params = inspect.signature(fn).parameters
    args = [case["input"], None]
    if "root_object" in params:
        args.append(case["input"])  # root_object == the top-level Parameters input
    needs_client = "api_client" in params

    try:
        out = fn(client, *args) if needs_client else fn(*args)
    except Exception as exc:  # noqa: BLE001 -- deliberately broad; see below
        if "expected_error" not in case:
            raise CaseError(
                f"case `{case['name']}` ({case['converter']}) raised unexpectedly: "
                f"{type(exc).__name__}: {exc}"
            ) from exc
        return {
            "input": case["input"],
            "expected": None,
            "expected_error": case["expected_error"],
        }

    if "expected_error" in case:
        raise CaseError(
            f"case `{case['name']}` ({case['converter']}) was expected to raise "
            f"(`{case['expected_error']}`) but returned {out!r}"
        )
    return {
        "input": case["input"],
        "expected": normalize(out),
        "expected_error": None,
    }


def main() -> None:
    import google.genai  # noqa: PLC0415
    from google.genai._api_client import BaseApiClient  # noqa: PLC0415

    sdk_dir = pathlib.Path(google.genai.__file__).parent
    known_functions = gc.build_known_functions(sdk_dir)
    client = BaseApiClient(api_key="test", vertexai=False)

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    written = 0
    for case in fixtures_cases.CASES:
        converter = case["converter"]
        module = known_functions.get(gc.to_snake(converter))
        if module is None:
            print(
                f"gen_fixtures.py: `{converter}` (case `{case['name']}`) is not a"
                " known `_to_mldev`/`_from_mldev` converter -- skipping",
                file=sys.stderr,
            )
            continue

        record = run_case(case, client)

        out_dir = OUT_DIR / module
        out_dir.mkdir(parents=True, exist_ok=True)
        out_path = out_dir / f"{case['name']}.json"
        payload = {"converter": converter, **record}
        out_path.write_text(
            json.dumps(payload, indent=2, sort_keys=False) + "\n", encoding="utf-8"
        )
        written += 1

    print(f"gen_fixtures.py: wrote {written}/{len(fixtures_cases.CASES)} fixtures", file=sys.stderr)


if __name__ == "__main__":
    main()
