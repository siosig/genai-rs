"""Generates the oracle corpus for upstream's table-driven tests.

Upstream runs each `pytest_helper.TestTableItem` against recorded replays that
are not published. Instead, every in-scope case is executed once against the
real Python SDK with an in-process capturing HTTP transport, and the requests
the SDK produced are written to `tests/fixtures/upstream/<dir>/<stem>.json`.
The Rust test `tests/upstream_table` replays the same parameters through the
crate and compares the requests (see contracts/upstream-tests.md).

Run with the codegen venv. The upstream checkout supplies `google.genai` (and
its `tests` package) for this process, and is verified equal to the wheel.
"""

from __future__ import annotations

import base64
import collections
import datetime
import enum
import importlib
import json
import os
import pathlib
import re
import sys
import tempfile
import tomllib
import warnings

import upstream
import upstream_src

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT_DIR = REPO_ROOT / "tests" / "fixtures" / "upstream"
RESPONSES_PATH = REPO_ROOT / "tools" / "codegen" / "upstream_cases_responses.toml"
REPORT_PATH = REPO_ROOT / "target" / "upstream-cases-report.json"
CUSTOM_PATH = OUT_DIR / "_custom.json"
TESTS_PACKAGE = "google.genai.tests"
STREAM_SUFFIX = "_stream"
UPLOAD_METHOD_MARKER = "upload"
SSE_BODY = "data: {}\n\n"
UTC_OFFSET_TIMESTAMP = re.compile(r"^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?)\+00:00$")
PYTHON_BOOLEAN_TEXT = {"True": "true", "False": "false"}
VERTEX_ONLY_METHODS = frozenset(
    {
        "models.edit_image",
        "models.upscale_image",
        "models.recontext_image",
        "models.segment_image",
        "tunings.validate_reward",
    }
)

REASON_CUSTOM = "test_method is not a client method path"
REASON_UPLOAD = "resumable upload needs a multi-step server; ported by hand"
REASON_VERTEX = "case is Vertex AI specific"
REASON_NOT_JSON = "parameters are not JSON-serializable (Python objects)"


class _NotJson(Exception):
    pass


def _jsonable(value):
    """Converts a table parameter to JSON, as the Rust runner will read it."""
    import pydantic  # noqa: PLC0415

    if isinstance(value, pydantic.BaseModel):
        return value.model_dump(mode="json", by_alias=True, exclude_none=True)
    if isinstance(value, enum.Enum):
        return value.value
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    if isinstance(value, bytes):
        return base64.urlsafe_b64encode(value).decode("ascii")
    if isinstance(value, (datetime.datetime, datetime.date)):
        return value.isoformat()
    if isinstance(value, pathlib.PurePath):
        return str(value)
    if isinstance(value, dict):
        return {str(k): _jsonable(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [_jsonable(v) for v in value]
    raise _NotJson(type(value).__name__)


def _setup_environment() -> None:
    checkout = upstream_src.ensure_checkout(upstream.PINNED_VERSION)
    upstream_src.verify_matches_wheel(checkout)
    # `verify_matches_wheel` imported the wheel's `google.genai`; drop it so the
    # checkout (which also ships the `tests` package) is what gets imported.
    for name in [m for m in sys.modules if m == "google" or m.startswith("google.")]:
        del sys.modules[name]
    sys.path.insert(0, str(checkout))
    importlib.invalidate_caches()
    os.environ["GOOGLE_GENAI_REPLAYS_DIRECTORY"] = tempfile.mkdtemp()
    os.environ["UNITTEST_ON_FORGE"] = "1"
    warnings.simplefilter("ignore")


def _collect_tables(checkout: pathlib.Path) -> list[dict]:
    """Imports each table-driven upstream test module and captures its table."""
    import importlib  # noqa: PLC0415

    helper = importlib.import_module(f"{TESTS_PACKAGE}.pytest_helper")
    import pytest  # noqa: PLC0415

    captured: list[dict] = []
    current: dict = {}

    root = checkout / "google" / "genai" / "tests"

    def capturing_setup(*, file, globals_for_file=None, test_method=None, test_table=None, http_options=None):
        # `file` is the test module that *declares* the table; the module being
        # imported may only re-export it, so key the table by its declaring file.
        declared = pathlib.Path(file).resolve().relative_to(root.resolve())
        owner = {
            "source": declared.as_posix(),
            "dir": declared.parent.as_posix(),
            "stem": declared.stem.removeprefix("test_"),
        }
        if any(entry["source"] == owner["source"] for entry in captured):
            return pytest.mark.parametrize("use_vertex, replays_prefix, http_options", [(False, "", None)])
        captured.append({**owner, "test_method": test_method, "table": list(test_table or [])})
        return pytest.mark.parametrize("use_vertex, replays_prefix, http_options", [(False, "", None)])

    helper.setup = capturing_setup
    for path in sorted(root.rglob("test_*.py")):
        if "pytest_helper.setup" not in path.read_text(encoding="utf-8"):
            continue
        rel = path.relative_to(root)
        current.clear()
        current.update({"source": rel.as_posix(), "dir": rel.parent.as_posix(), "stem": rel.stem.removeprefix("test_")})
        importlib.import_module(f"{TESTS_PACKAGE}.{'.'.join(rel.with_suffix('').parts)}")
    return captured


def _canned_responses() -> dict:
    if not RESPONSES_PATH.exists():
        return {}
    with RESPONSES_PATH.open("rb") as handle:
        return tomllib.load(handle).get("response", {})


def _make_client(transport):
    import httpx  # noqa: PLC0415
    from google import genai  # noqa: PLC0415
    from google.genai import types  # noqa: PLC0415

    return genai.Client(
        api_key="test-key",
        http_options=types.HttpOptions(
            httpx_client=httpx.Client(transport=transport),
            httpx_async_client=httpx.AsyncClient(transport=httpx.MockTransport(lambda r: httpx.Response(500))),
        ),
    )


def _canonical(value):
    """Collapses spellings the API treats as identical so only real divergences
    fail the comparison: Python's `isoformat()` writes `+00:00` where Rust (and
    the API) write `Z`, and Python stringifies booleans in query strings as
    `True`/`False`."""
    if isinstance(value, str):
        match = UTC_OFFSET_TIMESTAMP.match(value)
        return f"{match.group(1)}Z" if match else value
    if isinstance(value, dict):
        return {k: _canonical(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_canonical(v) for v in value]
    return value


class _Capture:
    """httpx transport that records requests and answers with a canned body."""

    def __init__(self, body, stream: bool) -> None:
        import httpx  # noqa: PLC0415

        self.requests: list[dict] = []
        self._body = body
        self._stream = stream
        self.handle_request = self._handle
        self._httpx = httpx

    def _handle(self, request):
        raw = request.read()
        body = None
        if raw:
            try:
                body = json.loads(raw)
            except ValueError:
                body = {"_raw": base64.b64encode(raw).decode("ascii")}
        query = sorted([k, PYTHON_BOOLEAN_TEXT.get(v, v)] for k, v in request.url.params.multi_items())
        self.requests.append(
            {"method": request.method, "path": request.url.path, "query": query, "body": _canonical(body)}
        )
        if self._stream:
            return self._httpx.Response(200, headers={"content-type": "text/event-stream"}, content=SSE_BODY.encode())
        return self._httpx.Response(200, json=self._body)


def _resolve(client, dotted: str):
    target = client
    for part in dotted.split("."):
        target = getattr(target, part)
    return target


def _run_case(test_method: str, item, responses: dict) -> tuple[dict | None, str]:
    """Returns (case record, skip reason). Exactly one of them is non-empty."""
    if "vertex" in item.name.lower():
        return None, REASON_VERTEX
    try:
        kwargs = {k: _jsonable(v) for k, v in vars(item.parameters).items()}
    except _NotJson as exc:
        return None, f"{REASON_NOT_JSON}: {exc}"
    stream = test_method.endswith(STREAM_SUFFIX)
    body = responses.get(test_method, {}).get("body", {})
    capture = _Capture(body, stream)
    import httpx  # noqa: PLC0415

    client = _make_client(httpx.MockTransport(capture._handle))
    error = None
    try:
        method = _resolve(client, test_method)
        result = method(**vars(item.parameters).copy())
        if hasattr(result, "__iter__") and not isinstance(result, (dict, str, bytes)) and stream:
            list(result)
    except Exception as exc:  # noqa: BLE001 -- the error text is the expectation
        error = f"{type(exc).__name__}: {exc}"
    expected_error = item.exception_if_mldev
    if error is not None and not expected_error:
        return None, f"unexpected Python error: {error[:200]}"
    # An mldev error that Python did not raise comes from the server (upstream's
    # replays recorded it); only the request can be compared, so the expected
    # text is kept as `server_error` for documentation.
    server_error = expected_error if error is None and expected_error else None
    if error is not None and expected_error not in error:
        return None, f"error text mismatch: {error[:200]}"
    record = {
        "name": item.name,
        "parameters": kwargs,
        "response_body": body,
        "expect": {
            "requests": capture.requests,
            "error_contains": expected_error if error is not None else None,
            "server_error": server_error,
        },
    }
    return record, ""


def main() -> None:
    upstream.assert_supported_python()
    _setup_environment()
    checkout = upstream_src.ensure_checkout(upstream.PINNED_VERSION)
    tables = _collect_tables(checkout)
    responses = _canned_responses()

    from google import genai  # noqa: PLC0415
    import httpx  # noqa: PLC0415

    probe = _make_client(httpx.MockTransport(lambda r: httpx.Response(500)))
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    report: dict[str, dict] = collections.defaultdict(dict)
    custom: list[dict] = []
    written = 0
    for entry in tables:
        method = entry["test_method"]
        source = entry["source"]
        if method in VERTEX_ONLY_METHODS:
            for item in entry["table"]:
                report[source][item.name] = f"skipped: {REASON_VERTEX}"
            continue
        if method is None or "." not in method or UPLOAD_METHOD_MARKER in method:
            reason = REASON_UPLOAD if method and UPLOAD_METHOD_MARKER in method else REASON_CUSTOM
            for item in entry["table"]:
                custom.append({"file": source, "case": item.name, "reason": reason})
                report[source][item.name] = f"skipped: {reason}"
            continue
        try:
            _resolve(probe, method)
        except AttributeError:
            for item in entry["table"]:
                custom.append({"file": source, "case": item.name, "reason": REASON_CUSTOM})
                report[source][item.name] = f"skipped: {REASON_CUSTOM}"
            continue
        cases = []
        for item in entry["table"]:
            record, reason = _run_case(method, item, responses)
            if record is None:
                report[source][item.name] = f"skipped: {reason}"
                continue
            report[source][item.name] = "included"
            cases.append(record)
        if cases:
            out = OUT_DIR / entry["dir"] / f"{entry['stem']}.json"
            out.parent.mkdir(parents=True, exist_ok=True)
            payload = {
                "upstream_version": upstream.PINNED_VERSION,
                "source": source,
                "test_method": method,
                "cases": cases,
            }
            out.write_text(json.dumps(payload, indent=2, sort_keys=True, ensure_ascii=True) + "\n", encoding="utf-8")
            written += len(cases)
    CUSTOM_PATH.write_text(json.dumps(custom, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    REPORT_PATH.parent.mkdir(parents=True, exist_ok=True)
    REPORT_PATH.write_text(json.dumps(report, indent=2, sort_keys=True), encoding="utf-8")
    skipped = sum(1 for cases in report.values() for status in cases.values() if status != "included")
    print(f"gen_upstream_cases.py: wrote {written} cases, skipped {skipped}", file=sys.stderr)


if __name__ == "__main__":
    main()
