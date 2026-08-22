//! Generated converter modules. See each file for its source module.

pub(crate) mod batches;
pub(crate) mod caches;
pub(crate) mod chats;
pub(crate) mod documents;
pub(crate) mod file_search_stores;
pub(crate) mod files;
pub(crate) mod live_converters;
pub(crate) mod models;
pub(crate) mod operations;
pub(crate) mod operations_converters;
pub(crate) mod tokens_converters;
pub(crate) mod tunings;

use serde_json::Value;

use crate::error::Result;

/// Dispatches to a generated `_X_to_mldev`/`_X_from_mldev` converter
/// by its Python name (e.g. `"_GenerateContentParameters_to_mldev"`),
/// calling it as `(input, None, None)`. Used by the golden-fixture
/// converter test suite (`tests/converters_golden.rs`) to invoke a
/// converter by name from `tests/fixtures/converters/**/*.json`.
#[allow(
    clippy::too_many_lines,
    reason = "one match arm per known converter function; the line count is mechanical, same as the generated converter files themselves"
)]
pub(crate) fn dispatch(name: &str, input: &Value) -> Result<Value> {
    match name {
        "_AuthConfig_to_mldev" => live_converters::auth_config_to_mldev(input, None, None),
        "_BatchJobDestination_from_mldev" => {
            batches::batch_job_destination_from_mldev(input, None, None)
        }
        "_BatchJobSource_to_mldev" => batches::batch_job_source_to_mldev(input, None, None),
        "_BatchJob_from_mldev" => batches::batch_job_from_mldev(input, None, None),
        "_Blob_to_mldev" => live_converters::blob_to_mldev(input, None, None),
        "_CancelBatchJobParameters_to_mldev" => {
            batches::cancel_batch_job_parameters_to_mldev(input, None, None)
        }
        "_CancelTuningJobParameters_to_mldev" => {
            tunings::cancel_tuning_job_parameters_to_mldev(input, None, None)
        }
        "_CancelTuningJobResponse_from_mldev" => {
            tunings::cancel_tuning_job_response_from_mldev(input, None, None)
        }
        "_Candidate_from_mldev" => batches::candidate_from_mldev(input, None, None),
        "_CitationMetadata_from_mldev" => batches::citation_metadata_from_mldev(input, None, None),
        "_Content_to_mldev" => live_converters::content_to_mldev(input, None, None),
        "_CountTokensConfig_to_mldev" => models::count_tokens_config_to_mldev(input, None, None),
        "_CountTokensParameters_to_mldev" => {
            models::count_tokens_parameters_to_mldev(input, None, None)
        }
        "_CountTokensResponse_from_mldev" => {
            models::count_tokens_response_from_mldev(input, None, None)
        }
        "_CreateAuthTokenConfig_to_mldev" => {
            tokens_converters::create_auth_token_config_to_mldev(input, None, None)
        }
        "_CreateAuthTokenParameters_to_mldev" => {
            tokens_converters::create_auth_token_parameters_to_mldev(input, None, None)
        }
        "_CreateBatchJobConfig_to_mldev" => {
            batches::create_batch_job_config_to_mldev(input, None, None)
        }
        "_CreateBatchJobParameters_to_mldev" => {
            batches::create_batch_job_parameters_to_mldev(input, None, None)
        }
        "_CreateCachedContentConfig_to_mldev" => {
            caches::create_cached_content_config_to_mldev(input, None, None)
        }
        "_CreateCachedContentParameters_to_mldev" => {
            caches::create_cached_content_parameters_to_mldev(input, None, None)
        }
        "_CreateEmbeddingsBatchJobConfig_to_mldev" => {
            batches::create_embeddings_batch_job_config_to_mldev(input, None, None)
        }
        "_CreateEmbeddingsBatchJobParameters_to_mldev" => {
            batches::create_embeddings_batch_job_parameters_to_mldev(input, None, None)
        }
        "_CreateFileParameters_to_mldev" => {
            files::create_file_parameters_to_mldev(input, None, None)
        }
        "_CreateFileResponse_from_mldev" => {
            files::create_file_response_from_mldev(input, None, None)
        }
        "_CreateFileSearchStoreConfig_to_mldev" => {
            file_search_stores::create_file_search_store_config_to_mldev(input, None, None)
        }
        "_CreateFileSearchStoreParameters_to_mldev" => {
            file_search_stores::create_file_search_store_parameters_to_mldev(input, None, None)
        }
        "_CreateTuningJobConfig_to_mldev" => {
            tunings::create_tuning_job_config_to_mldev(input, None, None)
        }
        "_CreateTuningJobParametersPrivate_to_mldev" => {
            tunings::create_tuning_job_parameters_private_to_mldev(input, None, None)
        }
        "_DeleteBatchJobParameters_to_mldev" => {
            batches::delete_batch_job_parameters_to_mldev(input, None, None)
        }
        "_DeleteCachedContentParameters_to_mldev" => {
            caches::delete_cached_content_parameters_to_mldev(input, None, None)
        }
        "_DeleteCachedContentResponse_from_mldev" => {
            caches::delete_cached_content_response_from_mldev(input, None, None)
        }
        "_DeleteDocumentConfig_to_mldev" => {
            documents::delete_document_config_to_mldev(input, None, None)
        }
        "_DeleteDocumentParameters_to_mldev" => {
            documents::delete_document_parameters_to_mldev(input, None, None)
        }
        "_DeleteFileParameters_to_mldev" => {
            files::delete_file_parameters_to_mldev(input, None, None)
        }
        "_DeleteFileResponse_from_mldev" => {
            files::delete_file_response_from_mldev(input, None, None)
        }
        "_DeleteFileSearchStoreConfig_to_mldev" => {
            file_search_stores::delete_file_search_store_config_to_mldev(input, None, None)
        }
        "_DeleteFileSearchStoreParameters_to_mldev" => {
            file_search_stores::delete_file_search_store_parameters_to_mldev(input, None, None)
        }
        "_DeleteModelParameters_to_mldev" => {
            models::delete_model_parameters_to_mldev(input, None, None)
        }
        "_DeleteModelResponse_from_mldev" => {
            models::delete_model_response_from_mldev(input, None, None)
        }
        "_DeleteResourceJob_from_mldev" => {
            batches::delete_resource_job_from_mldev(input, None, None)
        }
        "_EmbedContentBatch_to_mldev" => batches::embed_content_batch_to_mldev(input, None, None),
        "_EmbedContentConfig_to_mldev" => batches::embed_content_config_to_mldev(input, None, None),
        "_EmbedContentParametersPrivate_to_mldev" => {
            models::embed_content_parameters_private_to_mldev(input, None, None)
        }
        "_EmbedContentResponse_from_mldev" => {
            models::embed_content_response_from_mldev(input, None, None)
        }
        "_EmbeddingsBatchJobSource_to_mldev" => {
            batches::embeddings_batch_job_source_to_mldev(input, None, None)
        }
        "_FetchPredictOperationParameters_to_mldev" => {
            operations_converters::fetch_predict_operation_parameters_to_mldev(input, None, None)
        }
        "_FileData_to_mldev" => live_converters::file_data_to_mldev(input, None, None),
        "_FunctionCall_to_mldev" => live_converters::function_call_to_mldev(input, None, None),
        "_FunctionCallingConfig_to_mldev" => {
            batches::function_calling_config_to_mldev(input, None, None)
        }
        "_GenerateContentConfig_to_mldev" => {
            batches::generate_content_config_to_mldev(input, None, None)
        }
        "_GenerateContentParameters_to_mldev" => {
            models::generate_content_parameters_to_mldev(input, None, None)
        }
        "_GenerateContentResponse_from_mldev" => {
            batches::generate_content_response_from_mldev(input, None, None)
        }
        "_GenerateImagesConfig_to_mldev" => {
            models::generate_images_config_to_mldev(input, None, None)
        }
        "_GenerateImagesParameters_to_mldev" => {
            models::generate_images_parameters_to_mldev(input, None, None)
        }
        "_GenerateImagesResponse_from_mldev" => {
            models::generate_images_response_from_mldev(input, None, None)
        }
        "_GenerateVideosConfig_to_mldev" => {
            models::generate_videos_config_to_mldev(input, None, None)
        }
        "_GenerateVideosOperation_from_mldev" => {
            operations_converters::generate_videos_operation_from_mldev(input, None, None)
        }
        "_GenerateVideosParameters_to_mldev" => {
            models::generate_videos_parameters_to_mldev(input, None, None)
        }
        "_GenerateVideosResponse_from_mldev" => {
            operations_converters::generate_videos_response_from_mldev(input, None, None)
        }
        "_GenerateVideosSource_to_mldev" => {
            models::generate_videos_source_to_mldev(input, None, None)
        }
        "_GeneratedImage_from_mldev" => models::generated_image_from_mldev(input, None, None),
        "_GeneratedVideo_from_mldev" => {
            operations_converters::generated_video_from_mldev(input, None, None)
        }
        "_GetBatchJobParameters_to_mldev" => {
            batches::get_batch_job_parameters_to_mldev(input, None, None)
        }
        "_GetCachedContentParameters_to_mldev" => {
            caches::get_cached_content_parameters_to_mldev(input, None, None)
        }
        "_GetDocumentParameters_to_mldev" => {
            documents::get_document_parameters_to_mldev(input, None, None)
        }
        "_GetFileParameters_to_mldev" => files::get_file_parameters_to_mldev(input, None, None),
        "_GetFileSearchStoreParameters_to_mldev" => {
            file_search_stores::get_file_search_store_parameters_to_mldev(input, None, None)
        }
        "_GetModelParameters_to_mldev" => models::get_model_parameters_to_mldev(input, None, None),
        "_GetOperationParameters_to_mldev" => {
            operations_converters::get_operation_parameters_to_mldev(input, None, None)
        }
        "_GetTuningJobParameters_to_mldev" => {
            tunings::get_tuning_job_parameters_to_mldev(input, None, None)
        }
        "_GoogleMaps_to_mldev" => live_converters::google_maps_to_mldev(input, None, None),
        "_GoogleSearch_to_mldev" => live_converters::google_search_to_mldev(input, None, None),
        "_ImageConfig_to_mldev" => batches::image_config_to_mldev(input, None, None),
        "_Image_from_mldev" => models::image_from_mldev(input, None, None),
        "_Image_to_mldev" => models::image_to_mldev(input, None, None),
        "_ImportFileConfig_to_mldev" => {
            file_search_stores::import_file_config_to_mldev(input, None, None)
        }
        "_ImportFileOperation_from_mldev" => {
            operations_converters::import_file_operation_from_mldev(input, None, None)
        }
        "_ImportFileParameters_to_mldev" => {
            file_search_stores::import_file_parameters_to_mldev(input, None, None)
        }
        "_ImportFileResponse_from_mldev" => {
            operations_converters::import_file_response_from_mldev(input, None, None)
        }
        "_InlinedRequest_to_mldev" => batches::inlined_request_to_mldev(input, None, None),
        "_InlinedResponse_from_mldev" => batches::inlined_response_from_mldev(input, None, None),
        "_InternalRegisterFilesParameters_to_mldev" => {
            files::internal_register_files_parameters_to_mldev(input, None, None)
        }
        "_ListBatchJobsConfig_to_mldev" => {
            batches::list_batch_jobs_config_to_mldev(input, None, None)
        }
        "_ListBatchJobsParameters_to_mldev" => {
            batches::list_batch_jobs_parameters_to_mldev(input, None, None)
        }
        "_ListBatchJobsResponse_from_mldev" => {
            batches::list_batch_jobs_response_from_mldev(input, None, None)
        }
        "_ListCachedContentsConfig_to_mldev" => {
            caches::list_cached_contents_config_to_mldev(input, None, None)
        }
        "_ListCachedContentsParameters_to_mldev" => {
            caches::list_cached_contents_parameters_to_mldev(input, None, None)
        }
        "_ListCachedContentsResponse_from_mldev" => {
            caches::list_cached_contents_response_from_mldev(input, None, None)
        }
        "_ListDocumentsConfig_to_mldev" => {
            documents::list_documents_config_to_mldev(input, None, None)
        }
        "_ListDocumentsParameters_to_mldev" => {
            documents::list_documents_parameters_to_mldev(input, None, None)
        }
        "_ListDocumentsResponse_from_mldev" => {
            documents::list_documents_response_from_mldev(input, None, None)
        }
        "_ListFileSearchStoresConfig_to_mldev" => {
            file_search_stores::list_file_search_stores_config_to_mldev(input, None, None)
        }
        "_ListFileSearchStoresParameters_to_mldev" => {
            file_search_stores::list_file_search_stores_parameters_to_mldev(input, None, None)
        }
        "_ListFileSearchStoresResponse_from_mldev" => {
            file_search_stores::list_file_search_stores_response_from_mldev(input, None, None)
        }
        "_ListFilesConfig_to_mldev" => files::list_files_config_to_mldev(input, None, None),
        "_ListFilesParameters_to_mldev" => files::list_files_parameters_to_mldev(input, None, None),
        "_ListFilesResponse_from_mldev" => files::list_files_response_from_mldev(input, None, None),
        "_ListModelsConfig_to_mldev" => models::list_models_config_to_mldev(input, None, None),
        "_ListModelsParameters_to_mldev" => {
            models::list_models_parameters_to_mldev(input, None, None)
        }
        "_ListModelsResponse_from_mldev" => {
            models::list_models_response_from_mldev(input, None, None)
        }
        "_LiveClientContent_to_mldev" => {
            live_converters::live_client_content_to_mldev(input, None, None)
        }
        "_LiveClientMessage_to_mldev" => {
            live_converters::live_client_message_to_mldev(input, None, None)
        }
        "_LiveClientRealtimeInput_to_mldev" => {
            live_converters::live_client_realtime_input_to_mldev(input, None, None)
        }
        "_LiveClientSetup_to_mldev" => {
            live_converters::live_client_setup_to_mldev(input, None, None)
        }
        "_LiveConnectConfig_to_mldev" => {
            live_converters::live_connect_config_to_mldev(input, None, None)
        }
        "_LiveConnectConstraints_to_mldev" => {
            tokens_converters::live_connect_constraints_to_mldev(input, None, None)
        }
        "_LiveConnectParameters_to_mldev" => {
            live_converters::live_connect_parameters_to_mldev(input, None, None)
        }
        "_LiveMusicConnectParameters_to_mldev" => {
            live_converters::live_music_connect_parameters_to_mldev(input, None, None)
        }
        "_LiveMusicSetConfigParameters_to_mldev" => {
            live_converters::live_music_set_config_parameters_to_mldev(input, None, None)
        }
        "_LiveMusicSetWeightedPromptsParameters_to_mldev" => {
            live_converters::live_music_set_weighted_prompts_parameters_to_mldev(input, None, None)
        }
        "_LiveSendRealtimeInputParameters_to_mldev" => {
            live_converters::live_send_realtime_input_parameters_to_mldev(input, None, None)
        }
        "_LiveServerMessage_from_mldev" => {
            live_converters::live_server_message_from_mldev(input, None, None)
        }
        "_Model_from_mldev" => models::model_from_mldev(input, None, None),
        "_Part_to_mldev" => live_converters::part_to_mldev(input, None, None),
        "_RegisterFilesResponse_from_mldev" => {
            files::register_files_response_from_mldev(input, None, None)
        }
        "_SafetyAttributes_from_mldev" => models::safety_attributes_from_mldev(input, None, None),
        "_SafetySetting_to_mldev" => live_converters::safety_setting_to_mldev(input, None, None),
        "_SessionResumptionConfig_to_mldev" => {
            live_converters::session_resumption_config_to_mldev(input, None, None)
        }
        "_ToolConfig_to_mldev" => batches::tool_config_to_mldev(input, None, None),
        "_Tool_to_mldev" => live_converters::tool_to_mldev(input, None, None),
        "_TunedModel_from_mldev" => tunings::tuned_model_from_mldev(input, None, None),
        "_TuningDataset_to_mldev" => tunings::tuning_dataset_to_mldev(input, None, None),
        "_TuningJob_from_mldev" => tunings::tuning_job_from_mldev(input, None, None),
        "_TuningOperation_from_mldev" => tunings::tuning_operation_from_mldev(input, None, None),
        "_UpdateCachedContentConfig_to_mldev" => {
            caches::update_cached_content_config_to_mldev(input, None, None)
        }
        "_UpdateCachedContentParameters_to_mldev" => {
            caches::update_cached_content_parameters_to_mldev(input, None, None)
        }
        "_UpdateModelConfig_to_mldev" => models::update_model_config_to_mldev(input, None, None),
        "_UpdateModelParameters_to_mldev" => {
            models::update_model_parameters_to_mldev(input, None, None)
        }
        "_UploadToFileSearchStoreConfig_to_mldev" => {
            file_search_stores::upload_to_file_search_store_config_to_mldev(input, None, None)
        }
        "_UploadToFileSearchStoreOperation_from_mldev" => {
            operations_converters::upload_to_file_search_store_operation_from_mldev(
                input, None, None,
            )
        }
        "_UploadToFileSearchStoreParameters_to_mldev" => {
            file_search_stores::upload_to_file_search_store_parameters_to_mldev(input, None, None)
        }
        "_UploadToFileSearchStoreResponse_from_mldev" => {
            operations_converters::upload_to_file_search_store_response_from_mldev(
                input, None, None,
            )
        }
        "_UploadToFileSearchStoreResumableResponse_from_mldev" => {
            file_search_stores::upload_to_file_search_store_resumable_response_from_mldev(
                input, None, None,
            )
        }
        "_VideoGenerationReferenceImage_to_mldev" => {
            models::video_generation_reference_image_to_mldev(input, None, None)
        }
        "_Video_from_mldev" => operations_converters::video_from_mldev(input, None, None),
        "_Video_to_mldev" => models::video_to_mldev(input, None, None),
        "_VoiceActivity_from_mldev" => {
            live_converters::voice_activity_from_mldev(input, None, None)
        }
        _ => Err(crate::error::Error::Validation(format!(
            "converters::generated::dispatch: unknown converter `{name}`"
        ))),
    }
}
