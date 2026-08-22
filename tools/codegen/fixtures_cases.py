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

Coverage follows spec task T030's shape: for each covered converter, a
"kitchen sink" case (as many optional fields set as the type allows), a
"minimal" case (only what is required), and -- where the converter has
one -- a Vertex-AI-only field case that must be rejected. Every
`_to_mldev`/`_from_mldev` converter reachable from an implemented method
across models (generate_content/embed_content/count_tokens/generate_images/
generate_videos/get/list/update/delete), chats, files, caches, tunings,
batches (including create_embeddings), operations, documents,
file_search_stores, the Live API and auth_tokens is exercised at least
once, prioritising the ones that call a hand-written `t_*` transformer
(`src/transformers.rs`), since those are ported by hand rather than
transpiled and are therefore the highest-divergence-risk spots.

See tools/codegen/gen_converters.py's `render_dispatch_fn` for how a
`converter` name here is resolved to a generated Rust function, and
src/error.rs's `Error::UnsupportedByBackend` `Display` impl
(`field \\`{field}\\` is only supported by the {backend} backend`) for the
`expected_error` wording used below.

Known Rust-vs-Python gaps deliberately *not* exercised here (each is
called out again at the case that steers around it, and each was
confirmed with a throwaway probe fixture):

1. `t_speech_config` / `t_live_speech_config` / `t_schema` /
   `t_contents_for_embed` camelize their result, but Python's final
   `_common.convert_to_dict` normalization is a plain
   `model_dump(exclude_none=True)` -- **without** `by_alias` -- so Python
   emits `voice_config`/`prebuilt_voice_config`/`voice_name`,
   `min_length`/`property_ordering`, and `inline_data`/`file_data` in
   `snake_case` where this crate emits `camelCase`.
2. Python's `process_schema` auto-populates `property_ordering` from a
   multi-property `properties` dict's insertion order; this crate's
   `t_schema` deliberately does not (its `Schema::properties` is an
   unordered `HashMap`).
3. `gen_converters.py` does not transpile the `_X_to_mldev_enum_validate`
   helpers, so this crate accepts `SafetyFilterLevel.BLOCK_NONE`,
   `PersonGeneration.ALLOW_ALL` and
   `VideoGenerationReferenceType.STYLE`, which Python rejects for mldev.
4. `t_audio_blob`/`t_image_blob` read `"mimeType"` but receive
   `"mime_type"`, so they reject every blob (see
   `live_send_realtime_input_text` below).

Same class of thing as the `embed_content` note further down: documented
rather than papered over.
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
    # ================================================================
    # models.generate_content -- extra coverage for the hand-written
    # `t_*` transformers reached through `_GenerateContentConfig_to_mldev`
    # (`t_schema`, `t_json_schema`, `t_cached_content_name`, `t_tools`/
    # `t_tool`), plus more Vertex-only rejection paths.
    # ================================================================
    {
        # `response_schema` deliberately keeps every object to a *single*
        # property and uses only single-word `Schema` field names: Python's
        # `process_schema` auto-populates `property_ordering` from a
        # multi-property `properties` dict's insertion order (this crate's
        # `t_schema` deliberately does not -- see its doc comment: Rust's
        # `Schema::properties` is a `HashMap` with no stable order), and
        # Python's final `_common.convert_to_dict` normalization does a
        # plain `model_dump(exclude_none=True)` *without* `by_alias`, so a
        # multi-word `Schema` field (`min_length`, `property_ordering`, ...)
        # comes out `snake_case` from Python but `camelCase` from this
        # crate's `t_schema`. Both are accepted by the API (proto3 JSON
        # accepts either spelling) but they are not byte-identical, so a
        # fixture using them would not be a fair comparison.
        "name": "generate_content_response_schema",
        "converter": "_GenerateContentParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "who?"}]}],
            "config": {
                "response_mime_type": "application/json",
                "response_schema": {
                    "type": "OBJECT",
                    "properties": {
                        "answer": {
                            "type": "ARRAY",
                            "items": {"type": "STRING"},
                        }
                    },
                    "required": ["answer"],
                },
            },
        },
    },
    {
        "name": "generate_content_response_json_schema",
        "converter": "_GenerateContentParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "who?"}]}],
            "config": {
                "response_json_schema": {
                    "type": "object",
                    "properties": {"answer": {"type": "string"}},
                }
            },
        },
    },
    {
        "name": "generate_content_cached_content",
        "converter": "_GenerateContentParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "config": {"cached_content": "abc123"},
        },
    },
    {
        "name": "generate_content_tools_search_and_functions",
        "converter": "_GenerateContentParameters_to_mldev",
        "input": {
            "model": "tunedModels/my-tuned-model",
            "contents": [
                {"role": "user", "parts": [{"text": "hi"}]},
                {"role": "model", "parts": [{"text": "hello"}]},
            ],
            "config": {
                "tools": [
                    {"google_search": {}},
                    {"url_context": {}},
                    {
                        "function_declarations": [
                            {
                                "name": "lookup",
                                "description": "looks something up",
                                "parameters": {
                                    "type": "OBJECT",
                                    "properties": {"q": {"type": "STRING"}},
                                },
                            }
                        ]
                    },
                ],
                "tool_config": {
                    "function_calling_config": {
                        "mode": "ANY",
                        "allowed_function_names": ["lookup"],
                    }
                },
                "thinking_config": {
                    "include_thoughts": True,
                    "thinking_budget": 1024,
                },
                "response_modalities": ["TEXT"],
                "seed": 7,
                "presence_penalty": 0.1,
                "frequency_penalty": 0.2,
                "logprobs": 3,
                "response_logprobs": True,
                "media_resolution": "MEDIA_RESOLUTION_LOW",
            },
        },
    },
    {
        "name": "generate_content_vertex_only_audio_timestamp",
        "converter": "_GenerateContentParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "config": {"audio_timestamp": True},
        },
        "expected_error": "field `audio_timestamp` is only supported by the Vertex AI backend",
    },
    {
        "name": "generate_content_vertex_only_routing_config",
        "converter": "_GenerateContentParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "config": {"routing_config": {"auto_mode": {"model_routing_preference": "BALANCED"}}},
        },
        "expected_error": "field `routing_config` is only supported by the Vertex AI backend",
    },
    {
        "name": "generate_content_vertex_only_tool_retrieval",
        "converter": "_GenerateContentParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "config": {"tools": [{"retrieval": {"vertex_ai_search": {"datastore": "ds"}}}]},
        },
        "expected_error": "field `retrieval` is only supported by the Vertex AI backend",
    },
    {
        "name": "generate_content_vertex_only_safety_setting_method",
        "converter": "_GenerateContentParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "config": {
                "safety_settings": [
                    {
                        "category": "HARM_CATEGORY_HARASSMENT",
                        "threshold": "BLOCK_ONLY_HIGH",
                        "method": "SEVERITY",
                    }
                ]
            },
        },
        "expected_error": "field `method` is only supported by the Vertex AI backend",
    },
    # -- models.generate_content response side --------------------------
    {
        "name": "generate_content_response_from_mldev_kitchen_sink",
        "converter": "_GenerateContentResponse_from_mldev",
        "input": {
            "sdkHttpResponse": {"headers": {"content-type": "application/json"}},
            "candidates": [
                {
                    "content": {"role": "model", "parts": [{"text": "hello"}]},
                    "finishReason": "STOP",
                    "index": 0,
                    "avgLogprobs": -0.25,
                    "tokenCount": 12,
                    "citationMetadata": {
                        "citationSources": [
                            {
                                "startIndex": 0,
                                "endIndex": 5,
                                "uri": "https://example.com",
                                "license": "MIT",
                            }
                        ]
                    },
                    "safetyRatings": [
                        {
                            "category": "HARM_CATEGORY_HARASSMENT",
                            "probability": "NEGLIGIBLE",
                        }
                    ],
                }
            ],
            "modelVersion": "gemini-2.0-flash-001",
            "promptFeedback": {"blockReason": "SAFETY"},
            "usageMetadata": {
                "promptTokenCount": 5,
                "candidatesTokenCount": 7,
                "totalTokenCount": 12,
            },
            "responseId": "resp-1",
        },
    },
    {
        "name": "generate_content_response_from_mldev_minimal",
        "converter": "_GenerateContentResponse_from_mldev",
        "input": {"candidates": [{"content": {"role": "model", "parts": [{"text": "hi"}]}}]},
    },
    {
        "name": "candidate_from_mldev_minimal",
        "converter": "_Candidate_from_mldev",
        "input": {
            "content": {"role": "model", "parts": [{"text": "hi"}]},
            "finishReason": "MAX_TOKENS",
            "index": 1,
        },
    },
    {
        "name": "citation_metadata_from_mldev_minimal",
        "converter": "_CitationMetadata_from_mldev",
        "input": {
            "citationSources": [
                {"startIndex": 1, "endIndex": 4, "uri": "https://example.com"}
            ]
        },
    },
    # -- models.count_tokens / embed_content response converters ---------
    {
        "name": "count_tokens_response_from_mldev",
        "converter": "_CountTokensResponse_from_mldev",
        "input": {
            "sdkHttpResponse": {"headers": {"x": "y"}},
            "totalTokens": 31,
            "cachedContentTokenCount": 7,
        },
    },
    {
        "name": "embed_content_response_from_mldev",
        "converter": "_EmbedContentResponse_from_mldev",
        "input": {
            "sdkHttpResponse": {"headers": {"x": "y"}},
            "embeddings": [{"values": [0.1, 0.2, 0.3]}],
            "metadata": {"billableCharacterCount": 12},
        },
    },
    {
        "name": "embed_content_vertex_only_mime_type",
        "converter": "_EmbedContentParametersPrivate_to_mldev",
        "input": {
            "model": "text-embedding-004",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "config": {"mime_type": "text/plain"},
        },
        "expected_error": "field `mime_type` is only supported by the Vertex AI backend",
    },
    # ================================================================
    # models.get / list / update / delete (`t_model`, `t_models_url`,
    # `t_extract_models`)
    # ================================================================
    {
        "name": "models_get_minimal",
        "converter": "_GetModelParameters_to_mldev",
        "input": {"model": "gemini-2.0-flash"},
    },
    {
        "name": "models_get_already_prefixed",
        "converter": "_GetModelParameters_to_mldev",
        "input": {"model": "models/gemini-2.0-flash"},
    },
    {
        "name": "models_list_kitchen_sink",
        "converter": "_ListModelsParameters_to_mldev",
        "input": {
            "config": {
                "page_size": 20,
                "page_token": "tok",
                "filter": "displayName=foo",
                "query_base": True,
            }
        },
    },
    {
        "name": "models_list_tuned_models_url",
        "converter": "_ListModelsParameters_to_mldev",
        "input": {"config": {"query_base": False}},
    },
    {
        "name": "models_list_minimal",
        "converter": "_ListModelsParameters_to_mldev",
        "input": {},
    },
    {
        "name": "models_list_response_from_mldev",
        "converter": "_ListModelsResponse_from_mldev",
        "input": {
            "nextPageToken": "tok",
            "models": [
                {
                    "name": "models/gemini-2.0-flash",
                    "displayName": "Gemini 2.0 Flash",
                    "description": "fast",
                    "version": "001",
                    "inputTokenLimit": 1048576,
                    "outputTokenLimit": 8192,
                    "supportedGenerationMethods": ["generateContent"],
                    "temperature": 1.0,
                    "maxTemperature": 2.0,
                    "topP": 0.95,
                    "topK": 40,
                }
            ],
        },
    },
    {
        "name": "models_list_response_from_mldev_tuned",
        "converter": "_ListModelsResponse_from_mldev",
        "input": {"tunedModels": [{"name": "tunedModels/abc", "displayName": "mine"}]},
    },
    {
        "name": "model_from_mldev_minimal",
        "converter": "_Model_from_mldev",
        "input": {"name": "models/gemini-2.0-flash", "version": "001"},
    },
    {
        "name": "models_update_kitchen_sink",
        "converter": "_UpdateModelParameters_to_mldev",
        "input": {
            "model": "tunedModels/abc123",
            "config": {
                "display_name": "renamed",
                "description": "new description",
                "default_checkpoint_id": "ckpt-2",
            },
        },
    },
    {
        "name": "models_delete_minimal",
        "converter": "_DeleteModelParameters_to_mldev",
        "input": {"model": "tunedModels/abc123"},
    },
    {
        "name": "models_delete_response_from_mldev",
        "converter": "_DeleteModelResponse_from_mldev",
        "input": {"sdkHttpResponse": {"headers": {"x": "y"}}},
    },
    # ================================================================
    # models.generate_images (previously zero coverage)
    # ================================================================
    {
        # This case uses accepted values; the rejected ones
        # (`BLOCK_NONE`/`ALLOW_ALL`) get their own cases below, now that
        # `gen_converters.py` transpiles the `_X_to_mldev_enum_validate`
        # guards.
        "name": "generate_images_kitchen_sink",
        "converter": "_GenerateImagesParameters_to_mldev",
        "input": {
            "model": "imagen-3.0-generate-002",
            "prompt": "a red bicycle",
            "config": {
                "number_of_images": 2,
                "aspect_ratio": "16:9",
                "guidance_scale": 7.5,
                "safety_filter_level": "BLOCK_ONLY_HIGH",
                "person_generation": "ALLOW_ADULT",
                "include_safety_attributes": True,
                "include_rai_reason": True,
                "language": "en",
                "output_mime_type": "image/jpeg",
                "output_compression_quality": 80,
                "image_size": "2K",
            },
        },
    },
    {
        # Python: `_SafetyFilterLevel_to_mldev_enum_validate` raises for
        # `BLOCK_NONE`, which only the Vertex AI backend accepts.
        "name": "generate_images_vertex_only_safety_filter_level_value",
        "converter": "_GenerateImagesParameters_to_mldev",
        "input": {
            "model": "imagen-3.0-generate-002",
            "prompt": "a red bicycle",
            "config": {"safety_filter_level": "BLOCK_NONE"},
        },
        "expected_error": "field `safety_filter_level` is only supported by the Vertex AI backend",
    },
    {
        # Python: `_PersonGeneration_to_mldev_enum_validate` raises for
        # `ALLOW_ALL`.
        "name": "generate_images_vertex_only_person_generation_value",
        "converter": "_GenerateImagesParameters_to_mldev",
        "input": {
            "model": "imagen-3.0-generate-002",
            "prompt": "a red bicycle",
            "config": {"person_generation": "ALLOW_ALL"},
        },
        "expected_error": "field `person_generation` is only supported by the Vertex AI backend",
    },
    {
        "name": "generate_images_minimal",
        "converter": "_GenerateImagesParameters_to_mldev",
        "input": {"model": "imagen-3.0-generate-002", "prompt": "a cat"},
    },
    {
        "name": "generate_images_vertex_only_seed",
        "converter": "_GenerateImagesParameters_to_mldev",
        "input": {
            "model": "imagen-3.0-generate-002",
            "prompt": "a cat",
            "config": {"seed": 42},
        },
        "expected_error": "field `seed` is only supported by the Vertex AI backend",
    },
    {
        "name": "generate_images_vertex_only_negative_prompt",
        "converter": "_GenerateImagesParameters_to_mldev",
        "input": {
            "model": "imagen-3.0-generate-002",
            "prompt": "a cat",
            "config": {"negative_prompt": "dogs"},
        },
        "expected_error": "field `negative_prompt` is only supported by the Vertex AI backend",
    },
    {
        "name": "generate_images_vertex_only_add_watermark",
        "converter": "_GenerateImagesParameters_to_mldev",
        "input": {
            "model": "imagen-3.0-generate-002",
            "prompt": "a cat",
            "config": {"add_watermark": True},
        },
        "expected_error": "field `add_watermark` is only supported by the Vertex AI backend",
    },
    {
        # `_GenerateImagesConfig_to_mldev` writes everything it accepts into
        # its *parent* object and returns `{}`, so calling it directly (as
        # the dispatcher does, with `parent_object = None`) can only
        # meaningfully assert its rejection paths, which raise before any
        # parent write happens.
        "name": "generate_images_config_vertex_only_output_gcs_uri",
        "converter": "_GenerateImagesConfig_to_mldev",
        "input": {"output_gcs_uri": "gs://bucket/out"},
        "expected_error": "field `output_gcs_uri` is only supported by the Vertex AI backend",
    },
    {
        "name": "generate_images_config_vertex_only_enhance_prompt",
        "converter": "_GenerateImagesConfig_to_mldev",
        "input": {"enhance_prompt": True},
        "expected_error": "field `enhance_prompt` is only supported by the Vertex AI backend",
    },
    {
        "name": "generate_images_response_kitchen_sink",
        "converter": "_GenerateImagesResponse_from_mldev",
        "input": {
            "sdkHttpResponse": {"headers": {"content-type": "application/json"}},
            "predictions": [
                {
                    "bytesBase64Encoded": "aGVsbG8=",
                    "mimeType": "image/png",
                    "raiFilteredReason": "blocked",
                    "safetyAttributes": {
                        "categories": ["Violence"],
                        "scores": [0.1],
                    },
                    "contentType": "Positive Prompt",
                }
            ],
            "positivePromptSafetyAttributes": {
                "safetyAttributes": {"categories": ["Death"], "scores": [0.2]},
                "contentType": "Positive Prompt",
            },
        },
    },
    {
        "name": "generate_images_response_minimal",
        "converter": "_GenerateImagesResponse_from_mldev",
        "input": {"predictions": [{"bytesBase64Encoded": "aGVsbG8=", "mimeType": "image/png"}]},
    },
    {
        "name": "generated_image_from_mldev_kitchen_sink",
        "converter": "_GeneratedImage_from_mldev",
        "input": {
            "bytesBase64Encoded": "aGVsbG8=",
            "mimeType": "image/png",
            "raiFilteredReason": "blocked",
            "safetyAttributes": {"categories": ["Violence"], "scores": [0.3]},
            "contentType": "Positive Prompt",
        },
    },
    {
        "name": "safety_attributes_from_mldev_minimal",
        "converter": "_SafetyAttributes_from_mldev",
        "input": {
            "safetyAttributes": {"categories": ["Violence"], "scores": [0.3]},
            "contentType": "Positive Prompt",
        },
    },
    {
        "name": "image_from_mldev_minimal",
        "converter": "_Image_from_mldev",
        "input": {"bytesBase64Encoded": "aGVsbG8=", "mimeType": "image/png"},
    },
    {
        "name": "image_to_mldev_minimal",
        "converter": "_Image_to_mldev",
        "input": {"image_bytes": "aGVsbG8=", "mime_type": "image/png"},
    },
    {
        "name": "image_to_mldev_vertex_only_gcs_uri",
        "converter": "_Image_to_mldev",
        "input": {"gcs_uri": "gs://bucket/in.png"},
        "expected_error": "field `gcs_uri` is only supported by the Vertex AI backend",
    },
    {
        "name": "image_config_to_mldev_minimal",
        "converter": "_ImageConfig_to_mldev",
        "input": {"aspect_ratio": "1:1", "image_size": "1K"},
    },
    {
        "name": "image_config_to_mldev_vertex_only_person_generation",
        "converter": "_ImageConfig_to_mldev",
        "input": {"person_generation": "ALLOW_ADULT"},
        "expected_error": "field `person_generation` is only supported by the Vertex AI backend",
    },
    {
        "name": "image_config_to_mldev_vertex_only_output_mime_type",
        "converter": "_ImageConfig_to_mldev",
        "input": {"output_mime_type": "image/jpeg"},
        "expected_error": "field `output_mime_type` is only supported by the Vertex AI backend",
    },
    # ================================================================
    # models.generate_videos + operations (previously zero coverage)
    # ================================================================
    {
        "name": "generate_videos_kitchen_sink",
        "converter": "_GenerateVideosParameters_to_mldev",
        "input": {
            "model": "veo-3.0-generate-001",
            "prompt": "a drone shot of a canyon",
            "config": {
                "number_of_videos": 2,
                "duration_seconds": 8,
                "aspect_ratio": "16:9",
                "resolution": "1080p",
                "person_generation": "allow_adult",
                "negative_prompt": "blurry",
                "enhance_prompt": True,
                "last_frame": {"image_bytes": "aGVsbG8=", "mime_type": "image/png"},
                "reference_images": [
                    {
                        "image": {"image_bytes": "aGVsbG8=", "mime_type": "image/png"},
                        "reference_type": "ASSET",
                    }
                ],
                "webhook_config": {"uris": ["https://example.com/hook"]},
            },
        },
    },
    {
        "name": "generate_videos_minimal",
        "converter": "_GenerateVideosParameters_to_mldev",
        "input": {"model": "veo-3.0-generate-001", "prompt": "a cat"},
    },
    {
        "name": "generate_videos_image_and_video",
        "converter": "_GenerateVideosParameters_to_mldev",
        "input": {
            "model": "veo-3.0-generate-001",
            "image": {"image_bytes": "aGVsbG8=", "mime_type": "image/png"},
            "video": {"uri": "https://example.com/in.mp4", "mime_type": "video/mp4"},
        },
    },
    {
        "name": "generate_videos_from_source",
        "converter": "_GenerateVideosParameters_to_mldev",
        "input": {
            "model": "veo-3.0-generate-001",
            "source": {
                "prompt": "extend this",
                "video": {"video_bytes": "aGVsbG8=", "mime_type": "video/mp4"},
            },
        },
    },
    {
        "name": "generate_videos_vertex_only_fps",
        "converter": "_GenerateVideosParameters_to_mldev",
        "input": {
            "model": "veo-3.0-generate-001",
            "prompt": "a cat",
            "config": {"fps": 24},
        },
        "expected_error": "field `fps` is only supported by the Vertex AI backend",
    },
    {
        "name": "generate_videos_vertex_only_generate_audio",
        "converter": "_GenerateVideosParameters_to_mldev",
        "input": {
            "model": "veo-3.0-generate-001",
            "prompt": "a cat",
            "config": {"generate_audio": True},
        },
        "expected_error": "field `generate_audio` is only supported by the Vertex AI backend",
    },
    {
        "name": "generate_videos_vertex_only_output_gcs_uri",
        "converter": "_GenerateVideosParameters_to_mldev",
        "input": {
            "model": "veo-3.0-generate-001",
            "prompt": "a cat",
            "config": {"output_gcs_uri": "gs://bucket/out"},
        },
        "expected_error": "field `output_gcs_uri` is only supported by the Vertex AI backend",
    },
    {
        "name": "video_generation_reference_image_to_mldev",
        "converter": "_VideoGenerationReferenceImage_to_mldev",
        "input": {
            "image": {"image_bytes": "aGVsbG8=", "mime_type": "image/png"},
            "reference_type": "ASSET",
        },
    },
    {
        "name": "video_to_mldev_minimal",
        "converter": "_Video_to_mldev",
        "input": {
            "uri": "https://example.com/in.mp4",
            "video_bytes": "aGVsbG8=",
            "mime_type": "video/mp4",
        },
    },
    {
        "name": "video_from_mldev_minimal",
        "converter": "_Video_from_mldev",
        "input": {
            "uri": "https://example.com/out.mp4",
            "encodedVideo": "aGVsbG8=",
            "encoding": "video/mp4",
        },
    },
    {
        "name": "generated_video_from_mldev_minimal",
        "converter": "_GeneratedVideo_from_mldev",
        "input": {"video": {"uri": "https://example.com/out.mp4", "encoding": "video/mp4"}},
    },
    {
        "name": "generate_videos_response_from_mldev",
        "converter": "_GenerateVideosResponse_from_mldev",
        "input": {
            "generatedSamples": [
                {"video": {"uri": "https://example.com/out.mp4", "encoding": "video/mp4"}}
            ],
            "raiMediaFilteredCount": 1,
            "raiMediaFilteredReasons": ["policy"],
        },
    },
    {
        "name": "generate_videos_operation_kitchen_sink",
        "converter": "_GenerateVideosOperation_from_mldev",
        "input": {
            "name": "models/veo-3.0-generate-001/operations/abc123",
            "metadata": {"@type": "type.googleapis.com/x", "progressPercent": 50},
            "done": True,
            "response": {
                "generateVideoResponse": {
                    "generatedSamples": [
                        {
                            "video": {
                                "uri": "https://example.com/out.mp4",
                                "encodedVideo": "aGVsbG8=",
                                "encoding": "video/mp4",
                            }
                        }
                    ],
                    "raiMediaFilteredCount": 0,
                }
            },
        },
    },
    {
        "name": "generate_videos_operation_pending",
        "converter": "_GenerateVideosOperation_from_mldev",
        "input": {"name": "models/veo-3.0-generate-001/operations/abc123", "done": False},
    },
    {
        "name": "generate_videos_operation_error",
        "converter": "_GenerateVideosOperation_from_mldev",
        "input": {
            "name": "models/veo-3.0-generate-001/operations/abc123",
            "done": True,
            "error": {"code": 3, "message": "bad request"},
        },
    },
    # ================================================================
    # Live API (previously zero coverage)
    # ================================================================
    {
        # No `speech_config`: `t_live_speech_config` returns a
        # `types.SpeechConfig` *model* which Python's final
        # `convert_to_dict` dumps with `model_dump(exclude_none=True)` --
        # i.e. **without** `by_alias` -- producing `voice_config` /
        # `prebuilt_voice_config` / `voice_name` in `snake_case`, while this
        # crate's `t_live_speech_config` camelizes them. Same class of gap
        # as the `embed_content` note above; see the report for details.
        "name": "live_connect_kitchen_sink",
        "converter": "_LiveConnectParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash-live-001",
            "config": {
                "response_modalities": ["AUDIO"],
                "temperature": 0.4,
                "top_p": 0.8,
                "top_k": 20,
                "max_output_tokens": 512,
                "media_resolution": "MEDIA_RESOLUTION_MEDIUM",
                "seed": 11,
                "thinking_config": {"include_thoughts": False, "thinking_budget": 0},
                "enable_affective_dialog": True,
                "system_instruction": {"role": "user", "parts": [{"text": "be brief"}]},
                "tools": [{"google_search": {}}],
                "session_resumption": {"handle": "handle-1"},
                "input_audio_transcription": {"language_codes": ["en-US"]},
                "output_audio_transcription": {},
                "realtime_input_config": {
                    "automatic_activity_detection": {
                        "disabled": False,
                        "prefix_padding_ms": 20,
                        "silence_duration_ms": 100,
                    },
                    "activity_handling": "START_OF_ACTIVITY_INTERRUPTS",
                    "turn_coverage": "TURN_INCLUDES_ONLY_ACTIVITY",
                },
                "context_window_compression": {
                    "trigger_tokens": 16000,
                    "sliding_window": {"target_tokens": 8000},
                },
                "proactivity": {"proactive_audio": True},
                "history_config": {"initial_history_in_client_content": True},
                "safety_settings": [
                    {
                        "category": "HARM_CATEGORY_HARASSMENT",
                        "threshold": "BLOCK_ONLY_HIGH",
                    }
                ],
                "translation_config": {
                    "echo_target_language": True,
                    "target_language_code": "ja-JP",
                },
            },
        },
    },
    {
        "name": "live_connect_minimal",
        "converter": "_LiveConnectParameters_to_mldev",
        "input": {"model": "gemini-2.0-flash-live-001"},
    },
    {
        "name": "live_connect_vertex_only_explicit_vad_signal",
        "converter": "_LiveConnectParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash-live-001",
            "config": {"explicit_vad_signal": True},
        },
        "expected_error": "field `explicit_vad_signal` is only supported by the Vertex AI backend",
    },
    {
        "name": "live_client_content_kitchen_sink",
        "converter": "_LiveClientContent_to_mldev",
        "input": {
            "turns": [
                {"role": "user", "parts": [{"text": "hello"}]},
                {"role": "model", "parts": [{"text": "hi there"}]},
            ],
            "turn_complete": True,
        },
    },
    {
        "name": "live_client_content_minimal",
        "converter": "_LiveClientContent_to_mldev",
        "input": {"turn_complete": True},
    },
    {
        "name": "live_client_setup_kitchen_sink",
        "converter": "_LiveClientSetup_to_mldev",
        "input": {
            "model": "models/gemini-2.0-flash-live-001",
            "generation_config": {"temperature": 0.2},
            "system_instruction": {"role": "user", "parts": [{"text": "be brief"}]},
            "tools": [{"google_search": {}}],
            "session_resumption": {"handle": "h1"},
            "context_window_compression": {"trigger_tokens": 1000},
            "input_audio_transcription": {},
            "output_audio_transcription": {},
            "proactivity": {"proactive_audio": False},
            "safety_settings": [
                {"category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_NONE"}
            ],
        },
    },
    {
        "name": "live_client_setup_vertex_only_explicit_vad_signal",
        "converter": "_LiveClientSetup_to_mldev",
        "input": {"model": "models/x", "explicit_vad_signal": True},
        "expected_error": "field `explicit_vad_signal` is only supported by the Vertex AI backend",
    },
    {
        "name": "live_client_message_kitchen_sink",
        "converter": "_LiveClientMessage_to_mldev",
        "input": {
            "client_content": {
                "turns": [{"role": "user", "parts": [{"text": "hi"}]}],
                "turn_complete": True,
            },
            "tool_response": {
                "function_responses": [
                    {"id": "call-1", "name": "lookup", "response": {"result": "ok"}}
                ]
            },
        },
    },
    {
        "name": "live_client_realtime_input_text",
        "converter": "_LiveClientRealtimeInput_to_mldev",
        "input": {"text": "hello", "audio_stream_end": True},
    },
    {
        # Only the `text`/`activity_*` fields: an `audio`/`video` `Blob`
        # goes through `t_audio_blob`/`t_image_blob`, and this crate's
        # versions of those look the MIME type up under `"mimeType"` while
        # the value reaching them is spelled `"mime_type"` (which is how
        # `crate::types::Blob` serializes, and what the sibling
        # `_Blob_to_mldev` converter itself reads) -- so they currently
        # reject every blob with `unsupported mime type: None`. A fixture
        # for them would be asserting that bug rather than a match; see the
        # report accompanying this change. `media` is unaffected (`t_blobs`
        # only wraps into a list, it does not inspect the MIME type).
        "name": "live_send_realtime_input_text",
        "converter": "_LiveSendRealtimeInputParameters_to_mldev",
        "input": {"text": "hello", "activity_start": {}, "activity_end": {}},
    },
    {
        "name": "live_send_realtime_input_media",
        "converter": "_LiveSendRealtimeInputParameters_to_mldev",
        "input": {
            "media": [
                {"data": "aGVsbG8=", "mime_type": "audio/pcm;rate=16000"},
                {"data": "aGVsbG8=", "mime_type": "image/jpeg"},
            ]
        },
    },
    {
        "name": "live_server_message_kitchen_sink",
        "converter": "_LiveServerMessage_from_mldev",
        "input": {
            "setupComplete": {"sessionId": "sess-1"},
            "serverContent": {
                "modelTurn": {"role": "model", "parts": [{"text": "hi"}]},
                "turnComplete": True,
            },
            "toolCall": {"functionCalls": [{"id": "c1", "name": "lookup", "args": {}}]},
            "toolCallCancellation": {"ids": ["c1"]},
            "usageMetadata": {"promptTokenCount": 3, "totalTokenCount": 9},
            "goAway": {"timeLeft": "5s"},
            "sessionResumptionUpdate": {"newHandle": "h2", "resumable": True},
            "voiceActivityDetectionSignal": {"vadSignalType": "START_OF_SPEECH"},
            "voiceActivity": {"type": "ACTIVITY_START", "audioOffset": "1.5s"},
        },
    },
    {
        "name": "live_server_message_minimal",
        "converter": "_LiveServerMessage_from_mldev",
        "input": {"serverContent": {"turnComplete": True}},
    },
    {
        "name": "voice_activity_from_mldev_minimal",
        "converter": "_VoiceActivity_from_mldev",
        "input": {"type": "ACTIVITY_END", "audioOffset": "2s"},
    },
    {
        "name": "session_resumption_config_to_mldev_minimal",
        "converter": "_SessionResumptionConfig_to_mldev",
        "input": {"handle": "h1"},
    },
    {
        "name": "session_resumption_config_vertex_only_transparent",
        "converter": "_SessionResumptionConfig_to_mldev",
        "input": {"handle": "h1", "transparent": True},
        "expected_error": "field `transparent` is only supported by the Vertex AI backend",
    },
    {
        "name": "live_music_connect_minimal",
        "converter": "_LiveMusicConnectParameters_to_mldev",
        "input": {"model": "models/lyria-realtime-exp"},
    },
    {
        "name": "live_music_set_config_kitchen_sink",
        "converter": "_LiveMusicSetConfigParameters_to_mldev",
        "input": {
            "music_generation_config": {
                "temperature": 1.1,
                "top_k": 40,
                "seed": 3,
                "guidance": 4.0,
                "bpm": 120,
                "density": 0.5,
                "brightness": 0.7,
                "scale": "C_MAJOR_A_MINOR",
                "mute_bass": False,
                "mute_drums": False,
                "only_bass_and_drums": False,
                "music_generation_mode": "QUALITY",
            }
        },
    },
    {
        "name": "live_music_set_weighted_prompts_minimal",
        "converter": "_LiveMusicSetWeightedPromptsParameters_to_mldev",
        "input": {
            "weighted_prompts": [
                {"text": "minimal techno", "weight": 1.0},
                {"text": "ambient", "weight": 0.5},
            ]
        },
    },
    {
        "name": "live_blob_to_mldev_minimal",
        "converter": "_Blob_to_mldev",
        "input": {"data": "aGVsbG8=", "mime_type": "audio/pcm"},
    },
    {
        "name": "live_blob_to_mldev_vertex_only_display_name",
        "converter": "_Blob_to_mldev",
        "input": {"data": "aGVsbG8=", "mime_type": "audio/pcm", "display_name": "clip"},
        "expected_error": "field `display_name` is only supported by the Vertex AI backend",
    },
    {
        "name": "live_content_to_mldev_minimal",
        "converter": "_Content_to_mldev",
        "input": {
            "role": "user",
            "parts": [
                {"text": "hi"},
                {"inline_data": {"data": "aGVsbG8=", "mime_type": "image/png"}},
                {"file_data": {"file_uri": "files/abc", "mime_type": "image/png"}},
            ],
        },
    },
    {
        "name": "live_function_call_to_mldev_minimal",
        "converter": "_FunctionCall_to_mldev",
        "input": {"id": "c1", "name": "lookup", "args": {"q": "x"}},
    },
    {
        "name": "live_function_call_vertex_only_will_continue",
        "converter": "_FunctionCall_to_mldev",
        "input": {"name": "lookup", "will_continue": True},
        "expected_error": "field `will_continue` is only supported by the Vertex AI backend",
    },
    {
        "name": "live_tool_to_mldev_google_search",
        "converter": "_Tool_to_mldev",
        "input": {"google_search": {}},
    },
    {
        "name": "live_tool_vertex_only_enterprise_web_search",
        "converter": "_Tool_to_mldev",
        "input": {"enterprise_web_search": {}},
        "expected_error": "field `enterprise_web_search` is only supported by the Vertex AI backend",
    },
    {
        "name": "live_auth_config_to_mldev_minimal",
        "converter": "_AuthConfig_to_mldev",
        "input": {"api_key": "secret"},
    },
    {
        "name": "live_auth_config_vertex_only_auth_type",
        "converter": "_AuthConfig_to_mldev",
        "input": {"auth_type": "API_KEY_AUTH"},
        "expected_error": "field `auth_type` is only supported by the Vertex AI backend",
    },
    {
        "name": "live_google_search_vertex_only_exclude_domains",
        "converter": "_GoogleSearch_to_mldev",
        "input": {"exclude_domains": ["example.com"]},
        "expected_error": "field `exclude_domains` is only supported by the Vertex AI backend",
    },
    {
        "name": "live_google_maps_vertex_only_grounding_types",
        "converter": "_GoogleMaps_to_mldev",
        "input": {"grounding_types": ["PLACE_ID"]},
        "expected_error": "field `grounding_types` is only supported by the Vertex AI backend",
    },
    {
        "name": "live_file_data_to_mldev_minimal",
        "converter": "_FileData_to_mldev",
        "input": {"file_uri": "files/abc", "mime_type": "image/png"},
    },
    {
        "name": "live_safety_setting_to_mldev_minimal",
        "converter": "_SafetySetting_to_mldev",
        "input": {"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_ONLY_HIGH"},
    },
    {
        "name": "live_connect_constraints_to_mldev_minimal",
        "converter": "_LiveConnectConstraints_to_mldev",
        "input": {
            "model": "gemini-2.0-flash-live-001",
            "config": {"response_modalities": ["AUDIO"], "temperature": 0.3},
        },
    },
    # ================================================================
    # documents.get / list / delete (previously zero coverage)
    # ================================================================
    {
        "name": "documents_get_minimal",
        "converter": "_GetDocumentParameters_to_mldev",
        "input": {"name": "ragStores/store-1/documents/doc-1"},
    },
    {
        "name": "documents_list_kitchen_sink",
        "converter": "_ListDocumentsParameters_to_mldev",
        "input": {
            "parent": "ragStores/store-1",
            "config": {"page_size": 25, "page_token": "tok"},
        },
    },
    {
        "name": "documents_list_minimal",
        "converter": "_ListDocumentsParameters_to_mldev",
        "input": {"parent": "ragStores/store-1"},
    },
    {
        "name": "documents_list_response_from_mldev",
        "converter": "_ListDocumentsResponse_from_mldev",
        "input": {
            "sdkHttpResponse": {"headers": {"x": "y"}},
            "nextPageToken": "tok",
            "documents": [
                {"name": "ragStores/store-1/documents/doc-1", "displayName": "doc one"}
            ],
        },
    },
    {
        "name": "documents_delete_kitchen_sink",
        "converter": "_DeleteDocumentParameters_to_mldev",
        "input": {
            "name": "ragStores/store-1/documents/doc-1",
            "config": {"force": True},
        },
    },
    {
        "name": "documents_delete_minimal",
        "converter": "_DeleteDocumentParameters_to_mldev",
        "input": {"name": "ragStores/store-1/documents/doc-1"},
    },
    # ================================================================
    # batches.create_embeddings (previously zero coverage) + the rest of
    # the batches surface (`t_batch_job_name`, `t_job_state`,
    # `t_recv_batch_job_destination`, `t_batch_job_source`)
    # ================================================================
    {
        "name": "create_embeddings_batch_job_inlined",
        "converter": "_CreateEmbeddingsBatchJobParameters_to_mldev",
        "input": {
            "model": "text-embedding-004",
            "src": {
                "inlined_requests": {
                    "contents": [
                        {"role": "user", "parts": [{"text": "first"}]},
                        {"role": "user", "parts": [{"text": "second"}]},
                    ],
                    "config": {
                        "task_type": "RETRIEVAL_DOCUMENT",
                        "title": "t",
                        "output_dimensionality": 32,
                    },
                }
            },
            "config": {"display_name": "my-embeddings-batch"},
        },
    },
    {
        "name": "create_embeddings_batch_job_file",
        "converter": "_CreateEmbeddingsBatchJobParameters_to_mldev",
        "input": {
            "model": "text-embedding-004",
            "src": {"file_name": "files/embed-input"},
        },
    },
    {
        "name": "embeddings_batch_job_source_file_name",
        "converter": "_EmbeddingsBatchJobSource_to_mldev",
        "input": {"file_name": "files/embed-input"},
    },
    {
        "name": "embed_content_batch_to_mldev_minimal",
        "converter": "_EmbedContentBatch_to_mldev",
        "input": {"contents": [{"role": "user", "parts": [{"text": "hello"}]}]},
    },
    {
        "name": "embed_content_config_vertex_only_auto_truncate",
        "converter": "_EmbedContentConfig_to_mldev",
        "input": {"auto_truncate": True},
        "expected_error": "field `auto_truncate` is only supported by the Vertex AI backend",
    },
    {
        "name": "batches_create_inlined_requests",
        "converter": "_CreateBatchJobParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "src": {
                "inlined_requests": [
                    {
                        "model": "gemini-2.0-flash",
                        "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
                        "metadata": {"key": "value"},
                    }
                ]
            },
            "config": {"display_name": "my-batch"},
        },
    },
    {
        "name": "batch_job_source_to_mldev_file_name",
        "converter": "_BatchJobSource_to_mldev",
        "input": {"file_name": "files/batch-input"},
    },
    {
        "name": "batch_job_source_vertex_only_gcs_uri",
        "converter": "_BatchJobSource_to_mldev",
        "input": {"gcs_uri": ["gs://bucket/in.jsonl"]},
        "expected_error": "field `gcs_uri` is only supported by the Vertex AI backend",
    },
    {
        "name": "batches_cancel_minimal",
        "converter": "_CancelBatchJobParameters_to_mldev",
        "input": {"name": "batches/abc123"},
    },
    {
        "name": "batches_delete_minimal",
        "converter": "_DeleteBatchJobParameters_to_mldev",
        "input": {"name": "batches/abc123"},
    },
    {
        "name": "batch_job_from_mldev_kitchen_sink",
        "converter": "_BatchJob_from_mldev",
        "input": {
            "name": "batches/abc123",
            "metadata": {
                "displayName": "my-batch",
                "state": "BATCH_STATE_SUCCEEDED",
                "createTime": "2026-01-01T00:00:00Z",
                "endTime": "2026-01-01T01:00:00Z",
                "updateTime": "2026-01-01T01:00:00Z",
                "model": "models/gemini-2.0-flash",
                "output": {"responsesFile": "files/batch-output"},
            },
        },
    },
    {
        "name": "batch_job_from_mldev_pending",
        "converter": "_BatchJob_from_mldev",
        "input": {
            "name": "batches/abc123",
            "metadata": {"state": "BATCH_STATE_PENDING"},
        },
    },
    {
        "name": "batch_job_from_mldev_inlined_embed_responses",
        "converter": "_BatchJob_from_mldev",
        "input": {
            "name": "batches/abc123",
            "metadata": {
                "state": "BATCH_STATE_SUCCEEDED",
                "output": {
                    "inlinedResponses": {
                        "inlinedResponses": [
                            {"response": {"embedding": {"values": [0.1, 0.2]}}}
                        ]
                    }
                },
            },
        },
    },
    {
        "name": "batch_job_destination_from_mldev_file",
        "converter": "_BatchJobDestination_from_mldev",
        "input": {"responsesFile": "files/batch-output"},
    },
    {
        "name": "batches_list_response_from_mldev",
        "converter": "_ListBatchJobsResponse_from_mldev",
        "input": {
            "nextPageToken": "tok",
            "operations": [
                {
                    "name": "batches/abc123",
                    "metadata": {"state": "BATCH_STATE_RUNNING", "model": "models/x"},
                }
            ],
        },
    },
    {
        "name": "batches_list_vertex_only_filter",
        "converter": "_ListBatchJobsParameters_to_mldev",
        "input": {"config": {"page_size": 5, "filter": "state=RUNNING"}},
        "expected_error": "field `filter` is only supported by the Vertex AI backend",
    },
    {
        "name": "delete_resource_job_from_mldev",
        "converter": "_DeleteResourceJob_from_mldev",
        "input": {
            "sdkHttpResponse": {"headers": {"x": "y"}},
            "name": "batches/abc123",
            "done": True,
        },
    },
    {
        "name": "inlined_request_to_mldev_minimal",
        "converter": "_InlinedRequest_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "metadata": {"k": "v"},
        },
    },
    {
        "name": "inlined_response_from_mldev_minimal",
        "converter": "_InlinedResponse_from_mldev",
        "input": {
            "response": {"candidates": [{"content": {"role": "model", "parts": [{"text": "hi"}]}}]},
            "metadata": {"k": "v"},
        },
    },
    {
        "name": "batches_image_config_vertex_only_prominent_people",
        "converter": "_ImageConfig_to_mldev",
        "input": {"prominent_people": {"people": []}},
        "expected_error": "field `prominent_people` is only supported by the Vertex AI backend",
    },
    # ================================================================
    # caches -- delete/list, plus the remaining response converters
    # ================================================================
    {
        "name": "caches_delete_minimal",
        "converter": "_DeleteCachedContentParameters_to_mldev",
        "input": {"name": "cachedContents/abc123"},
    },
    {
        "name": "caches_delete_response_from_mldev",
        "converter": "_DeleteCachedContentResponse_from_mldev",
        "input": {"sdkHttpResponse": {"headers": {"x": "y"}}},
    },
    {
        "name": "caches_list_kitchen_sink",
        "converter": "_ListCachedContentsParameters_to_mldev",
        "input": {"config": {"page_size": 10, "page_token": "tok"}},
    },
    {
        "name": "caches_list_response_from_mldev",
        "converter": "_ListCachedContentsResponse_from_mldev",
        "input": {
            "nextPageToken": "tok",
            "cachedContents": [
                {"name": "cachedContents/abc123", "displayName": "my-cache"}
            ],
        },
    },
    {
        "name": "caches_create_bare_model_name",
        "converter": "_CreateCachedContentParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash-001",
            "config": {
                "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
                "expire_time": "2026-06-01T00:00:00Z",
            },
        },
    },
    # ================================================================
    # files -- `t_file_name`'s URI form, plus the response converters
    # ================================================================
    {
        "name": "files_get_from_uri",
        "converter": "_GetFileParameters_to_mldev",
        "input": {"name": "https://generativelanguage.googleapis.com/v1beta/files/abc123"},
    },
    {
        "name": "files_get_bare_id",
        "converter": "_GetFileParameters_to_mldev",
        "input": {"name": "abc123"},
    },
    {
        "name": "files_upload_kitchen_sink",
        "converter": "_CreateFileParameters_to_mldev",
        "input": {
            "file": {
                "name": "files/abc123",
                "display_name": "my file",
                "mime_type": "text/plain",
                "size_bytes": 12,
            }
        },
    },
    {
        "name": "files_create_response_from_mldev",
        "converter": "_CreateFileResponse_from_mldev",
        "input": {"sdkHttpResponse": {"headers": {"x": "y"}}},
    },
    {
        "name": "files_delete_response_from_mldev",
        "converter": "_DeleteFileResponse_from_mldev",
        "input": {"sdkHttpResponse": {"headers": {"x": "y"}}},
    },
    {
        "name": "files_list_response_from_mldev",
        "converter": "_ListFilesResponse_from_mldev",
        "input": {
            "nextPageToken": "tok",
            "files": [{"name": "files/abc123", "displayName": "my file"}],
        },
    },
    {
        "name": "files_list_kitchen_sink",
        "converter": "_ListFilesParameters_to_mldev",
        "input": {"config": {"page_size": 10, "page_token": "tok"}},
    },
    # ================================================================
    # file_search_stores -- import / upload / list / get / delete
    # ================================================================
    {
        "name": "file_search_stores_create_kitchen_sink",
        "converter": "_CreateFileSearchStoreParameters_to_mldev",
        "input": {
            "config": {
                "display_name": "my-store",
                "embedding_model": "text-embedding-004",
            }
        },
    },
    {
        "name": "file_search_stores_get_minimal",
        "converter": "_GetFileSearchStoreParameters_to_mldev",
        "input": {"name": "fileSearchStores/store-1"},
    },
    {
        "name": "file_search_stores_list_kitchen_sink",
        "converter": "_ListFileSearchStoresParameters_to_mldev",
        "input": {"config": {"page_size": 10, "page_token": "tok"}},
    },
    {
        "name": "file_search_stores_list_response_from_mldev",
        "converter": "_ListFileSearchStoresResponse_from_mldev",
        "input": {
            "nextPageToken": "tok",
            "fileSearchStores": [
                {"name": "fileSearchStores/store-1", "displayName": "my-store"}
            ],
        },
    },
    {
        "name": "file_search_stores_delete_kitchen_sink",
        "converter": "_DeleteFileSearchStoreParameters_to_mldev",
        "input": {"name": "fileSearchStores/store-1", "config": {"force": True}},
    },
    {
        "name": "file_search_stores_import_kitchen_sink",
        "converter": "_ImportFileParameters_to_mldev",
        "input": {
            "file_search_store_name": "fileSearchStores/store-1",
            "file_name": "files/abc123",
            "config": {
                "custom_metadata": [
                    {"key": "topic", "string_value": "rust"},
                    {"key": "rank", "numeric_value": 1.0},
                ],
                "chunking_config": {
                    "white_space_config": {
                        "max_tokens_per_chunk": 200,
                        "max_overlap_tokens": 20,
                    }
                },
            },
        },
    },
    {
        "name": "file_search_stores_import_minimal",
        "converter": "_ImportFileParameters_to_mldev",
        "input": {
            "file_search_store_name": "fileSearchStores/store-1",
            "file_name": "files/abc123",
        },
    },
    {
        "name": "file_search_stores_upload_kitchen_sink",
        "converter": "_UploadToFileSearchStoreParameters_to_mldev",
        "input": {
            "file_search_store_name": "fileSearchStores/store-1",
            "config": {
                "mime_type": "text/plain",
                "display_name": "notes.txt",
                "custom_metadata": [{"key": "topic", "string_value": "rust"}],
                "chunking_config": {
                    "white_space_config": {"max_tokens_per_chunk": 100}
                },
            },
        },
    },
    {
        "name": "file_search_stores_upload_resumable_response_from_mldev",
        "converter": "_UploadToFileSearchStoreResumableResponse_from_mldev",
        "input": {"sdkHttpResponse": {"headers": {"x": "y"}}},
    },
    # ================================================================
    # tunings -- cancel + the response converters (`t_tuning_job_status`)
    # ================================================================
    {
        "name": "tunings_tune_kitchen_sink",
        "converter": "_CreateTuningJobParametersPrivate_to_mldev",
        "input": {
            "base_model": "models/gemini-2.0-flash-001",
            "training_dataset": {
                "examples": [
                    {"text_input": "in", "output": "out"},
                    {"text_input": "in2", "output": "out2"},
                ]
            },
            "config": {
                "tuned_model_display_name": "my-tuned-model",
                "epoch_count": 5,
                "batch_size": 4,
                "learning_rate": 0.001,
            },
        },
    },
    {
        "name": "tunings_tune_vertex_only_adapter_size",
        "converter": "_CreateTuningJobParametersPrivate_to_mldev",
        "input": {
            "base_model": "models/gemini-2.0-flash-001",
            "training_dataset": {"examples": [{"text_input": "in", "output": "out"}]},
            "config": {"adapter_size": "ADAPTER_SIZE_ONE"},
        },
        "expected_error": "field `adapter_size` is only supported by the Vertex AI backend",
    },
    {
        "name": "tuning_dataset_vertex_only_gcs_uri",
        "converter": "_TuningDataset_to_mldev",
        "input": {"gcs_uri": "gs://bucket/train.jsonl"},
        "expected_error": "field `gcs_uri` is only supported by the Vertex AI backend",
    },
    {
        "name": "tunings_cancel_minimal",
        "converter": "_CancelTuningJobParameters_to_mldev",
        "input": {"name": "tunedModels/abc123"},
    },
    {
        "name": "tunings_cancel_response_from_mldev",
        "converter": "_CancelTuningJobResponse_from_mldev",
        "input": {"sdkHttpResponse": {"headers": {"x": "y"}}},
    },
    {
        "name": "tuning_job_from_mldev_kitchen_sink",
        "converter": "_TuningJob_from_mldev",
        "input": {
            "sdkHttpResponse": {"headers": {"x": "y"}},
            "name": "tunedModels/abc123",
            "state": "ACTIVE",
            "createTime": "2026-01-01T00:00:00Z",
            "updateTime": "2026-01-01T02:00:00Z",
            "description": "my tuning job",
            "baseModel": "models/gemini-2.0-flash-001",
            "tuningTask": {
                "startTime": "2026-01-01T00:05:00Z",
                "completeTime": "2026-01-01T01:55:00Z",
            },
        },
    },
    {
        "name": "tuning_job_from_mldev_creating",
        "converter": "_TuningJob_from_mldev",
        "input": {"name": "tunedModels/abc123", "state": "CREATING"},
    },
    {
        "name": "tuning_job_from_mldev_unmapped_state",
        "converter": "_TuningJob_from_mldev",
        "input": {"name": "tunedModels/abc123", "state": "JOB_STATE_PAUSED"},
    },
    {
        "name": "tuned_model_from_mldev_minimal",
        "converter": "_TunedModel_from_mldev",
        "input": {"name": "tunedModels/abc123"},
    },
    {
        "name": "tuning_operation_from_mldev_minimal",
        "converter": "_TuningOperation_from_mldev",
        "input": {
            "sdkHttpResponse": {"headers": {"x": "y"}},
            "name": "tunedModels/abc123/operations/op-1",
            "metadata": {"completedPercent": 42},
            "done": False,
        },
    },
    # ================================================================
    # operations / file_search_stores long-running operations
    # ================================================================
    {
        "name": "operations_get_with_config",
        "converter": "_GetOperationParameters_to_mldev",
        "input": {"operation_name": "operations/abc123", "config": {}},
    },
    {
        "name": "import_file_operation_from_mldev",
        "converter": "_ImportFileOperation_from_mldev",
        "input": {
            "name": "fileSearchStores/store-1/operations/op-1",
            "metadata": {"progressPercent": 10},
            "done": True,
            "response": {},
        },
    },
    {
        "name": "import_file_response_from_mldev",
        "converter": "_ImportFileResponse_from_mldev",
        "input": {"sdkHttpResponse": {"headers": {"x": "y"}}},
    },
    {
        "name": "upload_to_file_search_store_operation_from_mldev",
        "converter": "_UploadToFileSearchStoreOperation_from_mldev",
        "input": {
            "name": "fileSearchStores/store-1/operations/op-2",
            "done": True,
            "response": {},
        },
    },
    {
        "name": "upload_to_file_search_store_response_from_mldev",
        "converter": "_UploadToFileSearchStoreResponse_from_mldev",
        "input": {"sdkHttpResponse": {"headers": {"x": "y"}}},
    },
    # ================================================================
    # auth_tokens (`_tokens_converters.py`)
    # ================================================================
    {
        "name": "auth_tokens_create_with_live_constraints",
        "converter": "_CreateAuthTokenParameters_to_mldev",
        "input": {
            "config": {
                "uses": 3,
                "expire_time": "2026-01-01T00:00:00Z",
                "new_session_expire_time": "2026-01-01T00:30:00Z",
                "lock_additional_fields": ["temperature"],
                "live_connect_constraints": {
                    "model": "gemini-2.0-flash-live-001",
                    "config": {
                        "response_modalities": ["AUDIO"],
                        "temperature": 0.5,
                        "system_instruction": {
                            "role": "user",
                            "parts": [{"text": "be brief"}],
                        },
                    },
                },
            }
        },
    },
    # ================================================================
    # Standalone leaf converters that are otherwise only reached
    # indirectly (they return their own `to_object`, so calling them
    # directly through the dispatcher is meaningful).
    #
    # The converters left with no *direct* case of their own are exactly
    # the `_XConfig_to_mldev` family (plus `_GenerateVideosSource_to_mldev`):
    # each writes only into its `parent_object` and returns an empty
    # `to_object`, so calling it through the dispatcher -- which passes
    # `parent_object = None` -- can never assert anything but its
    # Vertex-only rejection paths. They are covered indirectly, through the
    # `Parameters`-level converter that calls them (which every one of them
    # has a case for above), and directly wherever they do have a rejection
    # path worth pinning (`_GenerateImagesConfig_to_mldev`,
    # `_EmbedContentConfig_to_mldev`, `_ImageConfig_to_mldev`).
    # ================================================================
    {
        "name": "part_to_mldev_kitchen_sink",
        "converter": "_Part_to_mldev",
        "input": {
            "text": "hello",
            "thought": True,
            "thought_signature": "c2ln",
            "inline_data": {"data": "aGVsbG8=", "mime_type": "image/png"},
            "file_data": {"file_uri": "files/abc", "mime_type": "image/png"},
            "function_call": {"id": "c1", "name": "lookup", "args": {"q": "x"}},
            "function_response": {"id": "c1", "name": "lookup", "response": {"ok": True}},
            "executable_code": {"code": "print(1)", "language": "PYTHON"},
            "code_execution_result": {"outcome": "OUTCOME_OK", "output": "1"},
            "video_metadata": {"start_offset": "0s", "end_offset": "5s"},
        },
    },
    {
        "name": "part_to_mldev_minimal",
        "converter": "_Part_to_mldev",
        "input": {"text": "hello"},
    },
    {
        "name": "part_to_mldev_vertex_only_inline_data_display_name",
        "converter": "_Part_to_mldev",
        "input": {"inline_data": {"data": "aGVsbG8=", "mime_type": "image/png", "display_name": "x"}},
        "expected_error": "field `display_name` is only supported by the Vertex AI backend",
    },
    {
        "name": "tool_config_to_mldev_kitchen_sink",
        "converter": "_ToolConfig_to_mldev",
        "input": {
            "function_calling_config": {
                "mode": "ANY",
                "allowed_function_names": ["lookup", "search"],
            }
        },
    },
    {
        "name": "function_calling_config_to_mldev_minimal",
        "converter": "_FunctionCallingConfig_to_mldev",
        "input": {"mode": "AUTO"},
    },
    {
        "name": "function_calling_config_vertex_only_stream_function_call_arguments",
        "converter": "_FunctionCallingConfig_to_mldev",
        "input": {"mode": "AUTO", "stream_function_call_arguments": True},
        "expected_error": "field `stream_function_call_arguments` is only supported by the Vertex AI backend",
    },
    {
        # Every field of `_FetchPredictOperationParameters_to_mldev` is
        # Vertex-only, so the mldev converter can only ever reject.
        "name": "fetch_predict_operation_vertex_only_operation_name",
        "converter": "_FetchPredictOperationParameters_to_mldev",
        "input": {"operation_name": "operations/abc123"},
        "expected_error": "field `operation_name` is only supported by the Vertex AI backend",
    },
    {
        "name": "fetch_predict_operation_vertex_only_resource_name",
        "converter": "_FetchPredictOperationParameters_to_mldev",
        "input": {"resource_name": "projects/p/locations/l/publishers/google/models/m"},
        "expected_error": "field `resource_name` is only supported by the Vertex AI backend",
    },
    {
        "name": "files_internal_register_minimal",
        "converter": "_InternalRegisterFilesParameters_to_mldev",
        "input": {"uris": ["https://example.com/a.txt", "https://example.com/b.txt"]},
    },
    {
        "name": "files_register_response_from_mldev",
        "converter": "_RegisterFilesResponse_from_mldev",
        "input": {
            "sdkHttpResponse": {"headers": {"x": "y"}},
            "files": [{"name": "files/abc123", "displayName": "a.txt"}],
        },
    },
    {
        "name": "count_tokens_vertex_only_tools",
        "converter": "_CountTokensParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "config": {"tools": [{"google_search": {}}]},
        },
        "expected_error": "field `tools` is only supported by the Vertex AI backend",
    },
    {
        "name": "count_tokens_vertex_only_generation_config",
        "converter": "_CountTokensParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "config": {"generation_config": {"temperature": 0.1}},
        },
        "expected_error": "field `generation_config` is only supported by the Vertex AI backend",
    },
    {
        "name": "count_tokens_kitchen_sink",
        "converter": "_CountTokensParameters_to_mldev",
        "input": {
            "model": "gemini-2.0-flash",
            "contents": [
                {"role": "user", "parts": [{"text": "hi"}]},
                {"role": "model", "parts": [{"text": "hello"}]},
            ],
        },
    },
    {
        "name": "caches_update_expire_time",
        "converter": "_UpdateCachedContentParameters_to_mldev",
        "input": {
            "name": "cachedContents/abc123",
            "config": {"expire_time": "2026-06-01T00:00:00Z"},
        },
    },
]
