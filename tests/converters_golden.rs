//! Golden-fixture tests for the generated `_X_to_mldev`/`_X_from_mldev`
//! converters (`src/converters/generated/`): every JSON file under
//! `tests/fixtures/converters/**/*.json` (produced by
//! `tools/codegen/gen_fixtures.py` from `tools/codegen/fixtures_cases.py`,
//! by running the real installed `google-genai` Python SDK) is replayed
//! through the matching Rust converter via
//! [`google_genai::converters::dispatch_converter`], and the result is
//! asserted against the fixture's `expected`/`expected_error`.
//!
//! See specs/001-port-genai-rust/contracts/codegen.md "`gen_fixtures.py`".

use std::path::PathBuf;

use google_genai::converters::dispatch_converter;
use rstest::rstest;
use serde::Deserialize;
use serde_json::Value;

/// Mirrors the JSON shape `gen_fixtures.py` writes: `{"converter": ...,
/// "input": ..., "expected": ... | null, "expected_error": "..." | null}`.
/// The `converter` field (the Python function's exact name, e.g.
/// `"_GenerateContentParameters_to_mldev"`) is this repo's own addition on
/// top of the fixture shape sketched in contracts/codegen.md, needed so
/// this test can look up the matching Rust function without having to
/// infer it from the file's path/name.
#[derive(Debug, Deserialize)]
struct Fixture {
    converter: String,
    input: Value,
    expected: Option<Value>,
    expected_error: Option<String>,
}

#[rstest]
fn converter_matches_python_golden_output(
    #[files("tests/fixtures/converters/**/*.json")] path: PathBuf,
) {
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read fixture {}: {err}", path.display()));
    let fixture: Fixture = serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("failed to parse fixture {}: {err}", path.display()));

    let result = dispatch_converter(&fixture.converter, &fixture.input);

    match (&fixture.expected, &fixture.expected_error) {
        (Some(expected), None) => match result {
            Ok(actual) => assert_eq!(
                &actual,
                expected,
                "{}: converter `{}` output mismatch",
                path.display(),
                fixture.converter
            ),
            Err(err) => panic!(
                "{}: converter `{}` was expected to succeed but returned Err: {err}",
                path.display(),
                fixture.converter
            ),
        },
        (None, Some(expected_error)) => match result {
            Err(err) => {
                let message = err.to_string();
                assert!(
                    message.contains(expected_error.as_str()),
                    "{}: converter `{}` error `{message}` did not contain expected substring `{expected_error}`",
                    path.display(),
                    fixture.converter
                );
            }
            Ok(actual) => panic!(
                "{}: converter `{}` was expected to fail with `{expected_error}` but succeeded with {actual:?}",
                path.display(),
                fixture.converter
            ),
        },
        (None, None) | (Some(_), Some(_)) => panic!(
            "{}: fixture must set exactly one of `expected`/`expected_error`",
            path.display()
        ),
    }
}
