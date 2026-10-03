//! Port of `transformers/test_t_batch.py` (Gemini Developer API cases; the
//! `vertex_client` tests are excluded by rule).

use gemini_genai::__test_support::transformers as t;
use serde_json::{Value, json};

mod batch_job_source {
    use super::*;

    // upstream-test: transformers/test_t_batch.py::TestBatchJobSource::test_batch_job_source_str
    #[test]
    fn test_batch_job_source_str() {
        let cases = [
            (
                "gs://bucket/path/to/data.jsonl",
                json!({"format": "jsonl", "gcs_uri": ["gs://bucket/path/to/data.jsonl"]}),
            ),
            (
                "bq://project.dataset.table",
                json!({"format": "bigquery", "bigquery_uri": "bq://project.dataset.table"}),
            ),
            ("files/data.csv", json!({"file_name": "files/data.csv"})),
        ];
        for (src, expected) in cases {
            assert_eq!(
                t::t_batch_job_source(json!(src)).unwrap(),
                expected,
                "{src}"
            );
        }
    }

    // upstream-test: transformers/test_t_batch.py::TestBatchJobSource::test_batch_job_source_str_unsupported
    #[test]
    fn test_batch_job_source_str_unsupported() {
        for src in ["http://example.com/data", "invalid-path"] {
            let err = t::t_batch_job_source(json!(src)).unwrap_err();
            assert!(
                err.to_string().contains("Unsupported source"),
                "{src}: {err}"
            );
        }
    }

    // upstream-test: transformers/test_t_batch.py::TestBatchJobSource::test_batch_job_source_list_empty
    #[test]
    fn test_batch_job_source_list_empty() {
        assert_eq!(
            t::t_batch_job_source(json!([])).unwrap(),
            json!({"inlined_requests": []})
        );
    }

    // upstream-test: transformers/test_t_batch.py::TestBatchJobSource::test_batch_job_source_list_with_items
    #[test]
    fn test_batch_job_source_list_with_items() {
        let inlined = json!([
            {"contents": [{"parts": [{"text": "item1"}]}]},
            {"contents": [{"parts": [{"text": "item2"}]}]},
        ]);
        assert_eq!(
            t::t_batch_job_source(inlined.clone()).unwrap(),
            json!({"inlined_requests": inlined})
        );
    }

    // upstream-test: transformers/test_t_batch.py::TestBatchJobSource::test_batch_job_source_dict
    #[test]
    fn test_batch_job_source_dict() {
        // Only the Developer API half of the upstream test applies; its
        // Vertex half (`gcs_uri`) is rejected on this backend.
        let src = json!({
            "inlined_requests": [{
                "contents": [{"parts": [{"text": "Hello!"}], "role": "user"}],
            }]
        });
        assert_eq!(t::t_batch_job_source(src.clone()).unwrap(), src);
        assert!(
            t::t_batch_job_source(json!({
                "gcs_uri": ["gs://test/file.jsonl"],
                "format": "jsonl",
            }))
            .is_err()
        );
    }

    // upstream-test: transformers/test_t_batch.py::TestBatchJobSource::test_batch_job_source_mldev_valid_file_name
    #[test]
    fn test_batch_job_source_mldev_valid_file_name() {
        let src = json!({"file_name": "files/my_data.csv"});
        assert_eq!(t::t_batch_job_source(src.clone()).unwrap(), src);
    }

    // upstream-test: transformers/test_t_batch.py::TestBatchJobSource::test_batch_job_source_mldev_invalid_both_set
    #[test]
    fn test_batch_job_source_mldev_invalid_both_set() {
        let err = t::t_batch_job_source(json!({
            "inlined_requests": [{}],
            "file_name": "files/data.csv",
        }))
        .unwrap_err();
        assert!(
            err.to_string().contains("`inlined_requests`, `file_name`,"),
            "unexpected error: {err}"
        );
    }

    // upstream-test: transformers/test_t_batch.py::TestBatchJobSource::test_batch_job_source_mldev_invalid_neither_set
    #[test]
    fn test_batch_job_source_mldev_invalid_neither_set() {
        let err = t::t_batch_job_source(json!({"gcs_uri": ["gs://temp"]})).unwrap_err();
        assert!(
            err.to_string().contains("`inlined_requests`, `file_name`,"),
            "unexpected error: {err}"
        );
    }
}

mod batch_job_destination {
    use super::*;

    // upstream-test: transformers/test_t_batch.py::TestBatchJobDestination::test_valid_destinations
    #[test]
    fn test_valid_destinations() {
        let cases = [
            (
                "gs://bucket/path/to/output",
                json!({"format": "jsonl", "gcs_uri": "gs://bucket/path/to/output"}),
            ),
            (
                "bq://project.dataset.output_table",
                json!({"format": "bigquery", "bigquery_uri": "bq://project.dataset.output_table"}),
            ),
        ];
        for (dest, expected) in cases {
            assert_eq!(
                t::t_batch_job_destination(json!(dest)).unwrap(),
                expected,
                "{dest}"
            );
        }
    }

    // upstream-test: transformers/test_t_batch.py::TestBatchJobDestination::test_unsupported_destination
    #[test]
    fn test_unsupported_destination() {
        for dest in ["local/path/output.jsonl", "http://some.url"] {
            let err = t::t_batch_job_destination(json!(dest)).unwrap_err();
            assert!(
                err.to_string().contains("Unsupported destination"),
                "{dest}: {err}"
            );
        }
    }
}

mod batch_job_name {
    use super::*;

    // upstream-test: transformers/test_t_batch.py::TestBatchJobName::test_mldev_valid_name
    #[test]
    fn test_mldev_valid_name() {
        assert_eq!(
            t::t_batch_job_name(json!("batches/my-job-123")).unwrap(),
            json!("my-job-123")
        );
    }

    // upstream-test: transformers/test_t_batch.py::TestBatchJobName::test_mldev_invalid_name
    #[test]
    fn test_mldev_invalid_name() {
        for name in [
            "my-job-123",
            "batches/my-job/suffix",
            "batches/",
            "batches/my-job-123/",
        ] {
            let err = t::t_batch_job_name(json!(name)).unwrap_err();
            assert!(
                err.to_string().contains("Invalid batch job name"),
                "{name}: {err}"
            );
        }
    }
}

mod job_state {
    use super::*;

    // upstream-test: transformers/test_t_batch.py::TestJobState::test_job_state_mapping
    #[test]
    fn test_job_state_mapping() {
        let cases = [
            ("BATCH_STATE_UNSPECIFIED", "JOB_STATE_UNSPECIFIED"),
            ("BATCH_STATE_PENDING", "JOB_STATE_PENDING"),
            ("BATCH_STATE_SUCCEEDED", "JOB_STATE_SUCCEEDED"),
            ("BATCH_STATE_FAILED", "JOB_STATE_FAILED"),
            ("BATCH_STATE_CANCELLED", "JOB_STATE_CANCELLED"),
            ("BATCH_STATE_EXPIRED", "JOB_STATE_EXPIRED"),
            ("BATCH_STATE_RUNNING", "JOB_STATE_RUNNING"),
            ("BATCH_STATE_FOOBAR", "BATCH_STATE_FOOBAR"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                t::t_job_state(Value::String(input.to_owned())).unwrap(),
                json!(expected),
                "{input}"
            );
        }
    }
}
