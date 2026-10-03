"""Fetches upstream `google-genai` sources that the published wheel lacks.

The wheel ships no tests, and sync diffs need the *previous* release's source.
Everything lands under `target/upstream-src/` (git-ignored via `/target/`):

- `v<version>/`: a shallow `git clone` of the upstream tag, tests included;
- `wheel-<version>/`: an unzipped wheel, used as the old/new side of a diff.

`verify_matches_wheel` guards the invariant the test tooling relies on: the
checked-out tag and the installed wheel contain byte-identical SDK sources.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import pathlib
import subprocess
import sys
import tempfile
import zipfile

from upstream import PINNED_VERSION

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
CACHE_DIR = REPO_ROOT / "target" / "upstream-src"
UPSTREAM_GIT_URL = "https://github.com/googleapis/python-genai"
PACKAGE_SUBDIR = pathlib.Path("google") / "genai"
TESTS_DIRNAME = "tests"


def ensure_checkout(version: str) -> pathlib.Path:
    """Returns a shallow checkout of the upstream tag `v<version>`."""
    target = CACHE_DIR / f"v{version}"
    if (target / PACKAGE_SUBDIR).is_dir():
        return target
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [
            "git",
            "clone",
            "--quiet",
            "--depth",
            "1",
            "--branch",
            f"v{version}",
            UPSTREAM_GIT_URL,
            str(target),
        ],
        check=True,
        # The public repo needs no credentials; ignore global config, which
        # may rewrite https URLs to ssh (`url.<base>.insteadOf`).
        env={**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_TERMINAL_PROMPT": "0"},
        cwd=CACHE_DIR,
    )
    return target


def _sdk_sources(root: pathlib.Path) -> dict[str, str]:
    """Maps each SDK source path (relative, tests excluded) to its sha256."""
    package = root / PACKAGE_SUBDIR
    digests: dict[str, str] = {}
    for path in sorted(package.rglob("*.py")):
        rel = path.relative_to(package)
        if rel.parts[0] == TESTS_DIRNAME or "__pycache__" in rel.parts:
            continue
        digests[rel.as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
    return digests


def verify_matches_wheel(checkout: pathlib.Path) -> None:
    """Exits unless the checkout's SDK sources equal the installed wheel's."""
    import google.genai  # noqa: PLC0415

    installed_root = pathlib.Path(google.genai.__file__).resolve().parents[2]
    expected = _sdk_sources(installed_root)
    actual = _sdk_sources(checkout)
    mismatched = sorted(
        name
        for name in expected.keys() | actual.keys()
        if expected.get(name) != actual.get(name)
    )
    if mismatched:
        listed = "\n  ".join(mismatched[:20])
        raise SystemExit(
            f"upstream_src: {len(mismatched)} SDK file(s) differ between the"
            f" tag checkout and the installed wheel:\n  {listed}"
        )


def wheel_source(version: str) -> pathlib.Path:
    """Returns a directory containing `google/genai` for `version`'s wheel."""
    target = CACHE_DIR / f"wheel-{version}"
    if (target / PACKAGE_SUBDIR).is_dir():
        return target
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(
            [
                sys.executable,
                "-m",
                "pip",
                "download",
                f"google-genai=={version}",
                "--no-deps",
                "--quiet",
                "-d",
                tmp,
            ],
            check=True,
        )
        wheels = sorted(pathlib.Path(tmp).glob("google_genai-*.whl"))
        if not wheels:
            raise SystemExit(f"upstream_src: no wheel downloaded for {version}")
        with zipfile.ZipFile(wheels[0]) as archive:
            archive.extractall(target)
    return target


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--ensure",
        action="store_true",
        help="clone the pinned tag and verify it against the installed wheel",
    )
    args = parser.parse_args()
    if args.ensure:
        checkout = ensure_checkout(PINNED_VERSION)
        verify_matches_wheel(checkout)
        print(checkout)


if __name__ == "__main__":
    main()
