"""Golden-fixture test cases for `gen_fixtures.py`.

Each entry names a real `google.genai` `_X_to_mldev`/`_X_from_mldev`
converter function (by its exact Python name, e.g.
`"_GenerateContentParameters_to_mldev"`) and a snake_case input dict to feed
it. `gen_fixtures.py` runs every case through the real installed SDK and
records the actual output (or, for a case that sets `expected_error`,
verifies the SDK raises and records that pre-supplied string instead --
Python's own `ValueError` message doesn't match this crate's
`Error::UnsupportedByBackend` `Display` wording, so the *expected* text has
to be authored here in this crate's own words, not captured from Python).

This is not an attempt at exhaustive converter coverage (see
specs/001-port-genai-rust/contracts/codegen.md "gen_fixtures.py"): it picks
one representative "Parameters"-level converter per already-implemented
method across models/chats/files/caches/tunings/batches/operations/
file_search_stores/auth_tokens, each with a "kitchen sink" case (every
optional field set), a "minimal" case (only required fields), and -- for a
handful of converters that have a Vertex-AI-only field -- an error case.

See tools/codegen/gen_converters.py's `render_dispatch_fn` for how a
`converter` name here is resolved to a generated Rust function, and
src/error.rs's `Error::UnsupportedByBackend` `Display` impl
(`field \\`{field}\\` is only supported by the {backend} backend`) for the
`expected_error` wording used below.
"""

from __future__ import annotations

CASES: list[dict] = [
    # -- models.generate_content --------------------------------------
    {
        "name": "generate_content_kitchen_sink",
        "converter": "_GenerateContentParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
            "config": {
                "temperature": 0.7,
                "top_p": 0.9,
                "top_k": 40,
                "candidate_count": 2,
                "max_output_tokens": 256,
                "stop_sequences": ["STOP"],
                "system_instruction": {
                    "role": "user",
                    "parts": [{"text": "Be nice"}],
                },
                "safety_settings": [
                    {
                        "category": "HARM_CATEGORY_HARASSMENT",
                        "threshold": "BLOCK_ONLY_HIGH",
                    }
                ],
                "tools": [
                    {
                        "function_declarations": [
                            {
                                "name": "get_weather",
                                "description": "gets the weather",
                                "parameters": {
                                    "type": "OBJECT",
                                    "properties": {
                                        "location": {"type": "STRING"}
                                    },
                                },
                            }
                        ]
                    }
                ],
                "response_mime_type": "application/json",
            },
        },
    },
    {
        "name": "generate_content_minimal",
        "converter": "_GenerateContentParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
        },
    },
    {
        "name": "generate_content_vertex_only_labels",
        "converter": "_GenerateContentParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "config": {"labels": {"team": "x"}},
        },
        "expected_error": "field `labels` is only supported by the Vertex AI backend",
    },
    # -- chats.send (delegates to models.generate_content; chats.py has no
    #    `_to_mldev`/`_from_mldev` converters of its own) -----------------
    {
        "name": "chats_send_basic",
        "converter": "_GenerateContentParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "continue our chat"}]}],
        },
    },
    # -- models.embed_content -------------------------------------------
    {
        # `contents` are given as structured Content dicts (`{"role":
        # ..., "parts": [...]}`), not bare strings: `_transformers.py`'s
        # real `t_content` would coerce a bare string into a `UserContent`
        # via pydantic's `parts` validator, a str->Content coercion this
        # crate's `crate::transformers::t_content` doesn't yet perform
        # (it's currently a passthrough) -- out of scope for this golden-
        # fixture test suite to fix, so the input here is pre-normalized
        # to exercise `_EmbedContentParametersPrivate_to_mldev` /
        # `_EmbedContentConfig_to_mldev` without depending on that gap.
        "name": "embed_content_kitchen_sink",
        "converter": "_EmbedContentParametersPrivate_to_mldev",
        "input": {
            "model": "text-embedding-004",
            "contents": [
                {"role": "user", "parts": [{"text": "What is your name?"}]},
                {"role": "user", "parts": [{"text": "What is your favorite color?"}]},
            ],
            "config": {
                "task_type": "RETRIEVAL_DOCUMENT",
                "title": "my title",
                "output_dimensionality": 64,
            },
        },
    },
    {
        "name": "embed_content_minimal",
        "converter": "_EmbedContentParametersPrivate_to_mldev",
        "input": {
            "model": "text-embedding-004",
            "contents": [{"role": "user", "parts": [{"text": "hello"}]}],
        },
    },
    # -- models.count_tokens ----------------------------------------------
    {
        "name": "count_tokens_minimal",
        "converter": "_CountTokensParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
        },
    },
    {
        "name": "count_tokens_vertex_only_system_instruction",
        "converter": "_CountTokensParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "config": {
                "system_instruction": {"role": "user", "parts": [{"text": "x"}]}
            },
        },
        "expected_error": "field `system_instruction` is only supported by the Vertex AI backend",
    },
    # -- files.upload / get / list / delete --------------------------------
    {
        "name": "files_upload_minimal",
        "converter": "_CreateFileParameters_to_mldev",
        "input": {"file": {"display_name": "my file"}},
    },
    {
        "name": "files_get_minimal",
        "converter": "_GetFileParameters_to_mldev",
        "input": {"name": "files/abc123"},
    },
    {
        "name": "files_list_minimal",
        "converter": "_ListFilesParameters_to_mldev",
        "input": {"config": {"page_size": 10}},
    },
    {
        "name": "files_delete_minimal",
        "converter": "_DeleteFileParameters_to_mldev",
        "input": {"name": "files/abc123"},
    },
    # -- caches.create / get / update ---------------------------------------
    {
        "name": "caches_create_kitchen_sink",
        "converter": "_CreateCachedContentParameters_to_mldev",
        "input": {
            "model": "models/gemini-2.0-flash-001",
            "config": {
                "contents": [{"role": "user", "parts": [{"text": "cache me"}]}],
                "system_instruction": {"role": "user", "parts": [{"text": "sys"}]},
                "ttl": "3600s",
                "display_name": "my-cache",
            },
        },
    },
    {
        "name": "caches_create_minimal",
        "converter": "_CreateCachedContentParameters_to_mldev",
        "input": {
            "model": "models/gemini-2.0-flash-001",
            "config": {"contents": [{"role": "user", "parts": [{"text": "hi"}]}]},
        },
    },
    {
        "name": "caches_create_vertex_only_kms_key_name",
        "converter": "_CreateCachedContentParameters_to_mldev",
        "input": {
            "model": "models/gemini-2.0-flash-001",
            "config": {
                "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
                "kms_key_name": "projects/x/locations/y/keyRings/z/cryptoKeys/k",
            },
        },
        "expected_error": "field `kms_key_name` is only supported by the Vertex AI backend",
    },
    {
        "name": "caches_get_minimal",
        "converter": "_GetCachedContentParameters_to_mldev",
        "input": {"name": "cachedContents/abc123"},
    },
    {
        "name": "caches_update_minimal",
        "converter": "_UpdateCachedContentParameters_to_mldev",
        "input": {"name": "cachedContents/abc123", "config": {"ttl": "7200s"}},
    },
    # -- tunings.tune / get --------------------------------------------------
    {
        "name": "tunings_tune_minimal",
        "converter": "_CreateTuningJobParametersPrivate_to_mldev",
        "input": {
            "base_model": "models/gemini-2.0-flash-001",
            "training_dataset": {
                "examples": [{"text_input": "in", "output": "out"}]
            },
        },
    },
    {
        "name": "tunings_get_minimal",
        "converter": "_GetTuningJobParameters_to_mldev",
        "input": {"name": "tunedModels/abc123"},
    },
    # -- batches.create / get / list -----------------------------------------
    {
        "name": "batches_create_minimal",
        "converter": "_CreateBatchJobParameters_to_mldev",
        "input": {
            "model": "models/gemini-2.0-flash-001",
            "src": {"file_name": "files/batch-input"},
        },
    },
    {
        "name": "batches_create_vertex_only_dest",
        "converter": "_CreateBatchJobParameters_to_mldev",
        "input": {
            "model": "models/gemini-2.0-flash-001",
            "src": {"file_name": "files/batch-input"},
            "config": {"dest": "files/batch-output"},
        },
        "expected_error": "field `dest` is only supported by the Vertex AI backend",
    },
    {
        "name": "batches_get_minimal",
        "converter": "_GetBatchJobParameters_to_mldev",
        "input": {"name": "batches/abc123"},
    },
    {
        "name": "batches_list_minimal",
        "converter": "_ListBatchJobsParameters_to_mldev",
        "input": {"config": {"page_size": 5}},
    },
    # -- operations.get -------------------------------------------------------
    {
        "name": "operations_get_minimal",
        "converter": "_GetOperationParameters_to_mldev",
        "input": {"operation_name": "operations/abc123"},
    },
    # -- file_search_stores.create ---------------------------------------------
    {
        "name": "file_search_stores_create_minimal",
        "converter": "_CreateFileSearchStoreParameters_to_mldev",
        "input": {"config": {"display_name": "my-store"}},
    },
    # -- auth_tokens.create ------------------------------------------------------
    {
        "name": "auth_tokens_create_kitchen_sink",
        "converter": "_CreateAuthTokenParameters_to_mldev",
        "input": {
            "config": {
                "uses": 1,
                "expire_time": "2026-01-01T00:00:00Z",
                "live_connect_constraints": {
                    "model": "gemini-2.0-flash-live-001"
                },
            }
        },
    },
    {
        "name": "auth_tokens_create_minimal",
        "converter": "_CreateAuthTokenParameters_to_mldev",
        "input": {},
    },
]
