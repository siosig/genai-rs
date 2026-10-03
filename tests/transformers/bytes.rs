//! Port of `transformers/test_bytes.py`.

use gemini_genai::__test_support::base_transformers as t;
use serde_json::json;

// upstream-test: transformers/test_bytes.py::test_t_bytes
#[test]
fn test_t_bytes() {
    assert_eq!(t::t_bytes(json!("string")).unwrap(), json!("string"));
}
