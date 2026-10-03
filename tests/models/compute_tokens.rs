//! Ports of `models/test_compute_tokens.py` plain functions. The Gemini
//! Developer API has no `computeTokens`; upstream asserts it raises.

use gemini_genai::{Error, types::Content};

use super::common::test_client;
use super::support::TestResult;

// upstream-test: models/test_compute_tokens.py::test_async
#[tokio::test]
async fn test_async() -> TestResult {
    let client = test_client("http://127.0.0.1:1".to_owned());
    let result = client
        .models()
        .compute_tokens(
            "gemini-2.5-flash",
            vec![Content::from("Tell me a story in 300 words.")],
            None,
        )
        .await;
    assert!(
        matches!(result, Err(Error::UnsupportedMethod(_))),
        "compute_tokens must fail on the Developer API, got {result:?}"
    );
    Ok(())
}
