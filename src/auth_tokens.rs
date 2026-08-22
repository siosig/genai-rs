//! `client.auth_tokens()`: ephemeral auth token creation for the Live API. Mirrors Python's `tokens.py`.

use reqwest::Method;
use serde_json::Value;

use crate::{
    client::Client,
    converters::generated::tokens_converters as conv,
    error::Result,
    types::{AuthToken, CreateAuthTokenConfig},
};

/// The (`snake_case`) field names of [`crate::types::GenerationConfig`],
/// used by [`convert_bidi_setup_to_token_setup`] to decide which
/// `lock_additional_fields` entries need a `generationConfig.` prefix.
/// Mirrors Python's `types.GenerationConfig().model_dump().keys()`.
const GENERATION_CONFIG_FIELDS: &[&str] = &[
    "model_selection_config",
    "response_json_schema",
    "audio_timestamp",
    "candidate_count",
    "enable_affective_dialog",
    "frequency_penalty",
    "logprobs",
    "max_output_tokens",
    "media_resolution",
    "presence_penalty",
    "response_logprobs",
    "response_mime_type",
    "response_modalities",
    "response_schema",
    "routing_config",
    "seed",
    "speech_config",
    "stop_sequences",
    "temperature",
    "thinking_config",
    "top_k",
    "top_p",
    "enable_enhanced_civic_answers",
    "response_format",
    "translation_config",
    "audio_transcription_config",
];

/// Mirrors `_common.get_value_by_path`'s truthiness check for a single
/// already-resolved JSON value (empty string/array/object, `false`, `0`,
/// and `null` are all falsy).
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_none_or(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Mirrors Python's `_get_field_masks`: for each key in `setup`, emits
/// `key` (if its value isn't a non-empty object) or `key.subkey` for each
/// key of a non-empty nested object value.
fn get_field_masks(setup: Option<&serde_json::Map<String, Value>>) -> String {
    let Some(setup) = setup else {
        return String::new();
    };
    let mut fields = Vec::new();
    for (key, value) in setup {
        if let Value::Object(nested) = value {
            if !nested.is_empty() {
                for nested_key in nested.keys() {
                    fields.push(format!("{key}.{nested_key}"));
                }
                continue;
            }
        }
        fields.push(key.clone());
    }
    fields.join(",")
}

/// Mirrors Python's `_convert_bidi_setup_to_token_setup`: the auth token
/// service expects a bare `BidiGenerateContentSetup` under
/// `bidiGenerateContentSetup` (not the `{"setup": ..., "config": ...}`
/// envelope `live_connect_constraints_to_mldev` produces), plus a
/// `fieldMask` string derived from `lock_additional_fields`.
fn convert_bidi_setup_to_token_setup(request: &mut Value, config: Option<&CreateAuthTokenConfig>) {
    let Some(obj) = request.as_object_mut() else {
        return;
    };

    let setup_field = obj
        .get("bidiGenerateContentSetup")
        .and_then(Value::as_object)
        .and_then(|bidi| bidi.get("setup"))
        .filter(|v| is_truthy(v))
        .cloned();

    if let Some(setup) = setup_field {
        let field_mask = get_field_masks(setup.as_object());
        obj.insert("bidiGenerateContentSetup".to_owned(), setup);

        match config.and_then(|c| c.lock_additional_fields.as_ref()) {
            Some(fields) if fields.is_empty() => {
                obj.insert("fieldMask".to_owned(), Value::String(field_mask));
            }
            None => {
                obj.remove("fieldMask");
            }
            Some(fields) => {
                let mapped: Vec<String> = fields
                    .iter()
                    .map(|field| {
                        if GENERATION_CONFIG_FIELDS.contains(&field.as_str()) {
                            format!("generationConfig.{field}")
                        } else {
                            field.clone()
                        }
                    })
                    .collect();
                let combined = if mapped.is_empty() {
                    field_mask
                } else {
                    format!("{field_mask},{}", mapped.join(","))
                };
                obj.insert("fieldMask".to_owned(), Value::String(combined));
            }
        }
    } else {
        let raw_list = obj.get("fieldMask").and_then(Value::as_array).cloned();
        match raw_list {
            Some(list) if !list.is_empty() => {
                let joined = list
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(",");
                obj.insert("fieldMask".to_owned(), Value::String(joined));
            }
            _ => {
                obj.remove("fieldMask");
            }
        }
    }

    let still_has_setup = obj.get("bidiGenerateContentSetup").is_some_and(is_truthy);
    if !still_has_setup {
        obj.remove("bidiGenerateContentSetup");
    }
}

/// Handle for `client.auth_tokens()`. Cheap to construct; borrows
/// nothing.
#[derive(Clone)]
pub struct AuthTokens {
    pub(crate) client: Client,
}

impl AuthTokens {
    /// \[Experimental\] Creates an ephemeral auth token for use with the
    /// Live API. Mirrors Python's `Tokens.create`.
    ///
    /// `config.live_connect_constraints`, when set, locks the Live API
    /// session's parameters for anyone using the resulting token; see the
    /// Python SDK's `Tokens.create` docstring for the exact locking
    /// semantics of `lock_additional_fields`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Api`] for a non-2xx response.
    pub async fn create(&self, config: Option<CreateAuthTokenConfig>) -> Result<AuthToken> {
        let params = serde_json::json!({ "config": config.clone() });
        let mut request = conv::create_auth_token_parameters_to_mldev(&params, None, None)?;
        if let Some(obj) = request.as_object_mut() {
            // Mirrors Python's `request_dict.pop('config', None)`: the
            // converter leaves an empty `"config": {}` behind since all
            // of `CreateAuthTokenConfig`'s fields are flattened onto the
            // parent object instead.
            obj.remove("config");
        }
        convert_bidi_setup_to_token_setup(&mut request, config.as_ref());

        let response = self
            .client
            .http()
            .request(Method::POST, "auth_tokens", None, Some(request), None)
            .await?;
        let wire: Value = if response.body.is_empty() {
            Value::Object(serde_json::Map::new())
        } else {
            serde_json::from_slice(&response.body)?
        };
        Ok(serde_json::from_value(wire)?)
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, method, path},
    };

    use super::AuthTokens;
    use crate::{
        client::Client,
        types::{CreateAuthTokenConfig, HttpOptions, LiveConnectConstraints},
    };

    fn test_client(base_url: String) -> Client {
        Client::builder()
            .api_key("test-key")
            .http_options(HttpOptions {
                base_url: Some(base_url),
                ..Default::default()
            })
            .build()
            .unwrap()
    }

    fn auth_tokens(server: &MockServer) -> AuthTokens {
        AuthTokens {
            client: test_client(server.uri()),
        }
    }

    #[tokio::test]
    async fn create_posts_uses_and_expire_time() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/auth_tokens"))
            .and(body_json(serde_json::json!({
                "uses": 10,
                "expireTime": "2025-05-01T00:00:00Z"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "auth_tokens/abc123"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let token = auth_tokens(&server)
            .create(Some(CreateAuthTokenConfig {
                uses: Some(10),
                expire_time: Some("2025-05-01T00:00:00Z".to_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(token.name.as_deref(), Some("auth_tokens/abc123"));
        server.verify().await;
    }

    #[tokio::test]
    async fn create_with_live_connect_constraints_locks_the_whole_setup() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/auth_tokens"))
            .and(body_json(serde_json::json!({
                "uses": 1,
                "bidiGenerateContentSetup": {
                    "model": "models/gemini-live-2.5-flash-preview",
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "auth_tokens/xyz789"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let token = auth_tokens(&server)
            .create(Some(CreateAuthTokenConfig {
                uses: Some(1),
                live_connect_constraints: Some(LiveConnectConstraints {
                    model: Some("gemini-live-2.5-flash-preview".to_owned()),
                    config: None,
                }),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(token.name.as_deref(), Some("auth_tokens/xyz789"));
        server.verify().await;
    }
}
