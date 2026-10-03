//! `client.chats()`: multi-turn chat sessions. Mirrors Python's `chats.py`.

use std::{
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures_core::Stream;
use futures_util::StreamExt;

use crate::{
    automatic_function_calling_util::{FunctionTool, registered_tools_for},
    client::Client,
    errors::Result,
    extra_utils::{get_max_remote_calls_afc, should_disable_afc},
    types::{Content, Contents, GenerateContentConfig, GenerateContentResponse, Part},
};

/// Handle for `client.chats()`. Cheap to construct; borrows nothing.
/// Mirrors Python's `Chats`.
#[derive(Clone)]
pub struct Chats {
    pub(crate) client: Client,
}

impl Chats {
    /// Creates a new [`Chat`] session. Mirrors Python's `Chats.create`.
    ///
    /// `history` seeds the conversation (e.g. to resume a previously
    /// recorded session); an invalid trailing model turn in the seed
    /// history is excluded from the curated history exactly as it would be
    /// after a live `send_message` call, matching Python's
    /// `_extract_curated_history`.
    #[must_use]
    pub fn create(
        &self,
        model: &str,
        config: Option<GenerateContentConfig>,
        history: Option<Vec<Content>>,
    ) -> Chat {
        let comprehensive_history = history.unwrap_or_default();
        let curated_history = extract_curated_history(&comprehensive_history);
        Chat {
            client: self.client.clone(),
            model: model.to_owned(),
            config,
            comprehensive_history,
            curated_history,
        }
    }
}

/// A multi-turn chat session. Accumulates history across calls to
/// [`Chat::send_message`]/[`Chat::send_message_stream`] and replays it on
/// every request, exactly like Python's `Chat`.
pub struct Chat {
    client: Client,
    model: String,
    config: Option<GenerateContentConfig>,
    comprehensive_history: Vec<Content>,
    curated_history: Vec<Content>,
}

impl Chat {
    /// Appends one exchange to both histories, mirroring Python's
    /// `_BaseChat.record_history`: the user turn and the model's output are
    /// always appended to the comprehensive history; they are appended to
    /// the curated history only when `is_valid`. An empty `model_output` is
    /// recorded as a single empty-parts `Content` so the history keeps
    /// alternating user/model turns.
    fn record_history(&mut self, user_input: Content, model_output: Vec<Content>, is_valid: bool) {
        let output_contents = if model_output.is_empty() {
            vec![Content {
                role: Some("model".to_owned()),
                parts: Some(Vec::new()),
            }]
        } else {
            model_output
        };

        self.comprehensive_history.push(user_input.clone());
        self.comprehensive_history
            .extend(output_contents.iter().cloned());
        if is_valid {
            self.curated_history.push(user_input);
            self.curated_history.extend(output_contents);
        }
    }

    /// Returns the chat history: the curated (valid-only) history if
    /// `curated` is `true`, otherwise the comprehensive history (every
    /// turn, including invalid model outputs). Mirrors Python's
    /// `Chat.get_history`.
    #[must_use]
    pub fn get_history(&self, curated: bool) -> &[Content] {
        if curated {
            &self.curated_history
        } else {
            &self.comprehensive_history
        }
    }

    /// Sends `message` plus the accumulated curated history to the model
    /// and returns its response. Mirrors Python's `Chat.send_message`.
    ///
    /// Automatic function calling runs here when `config.tools` contains a
    /// tool built by [`crate::types::Tool::from_function`] and AFC is not
    /// disabled. As in Python's `chats.py`, the loop lives in the chat
    /// itself so every intermediate turn is recorded into the history, one
    /// exchange per remote call:
    ///
    /// ```text
    /// [user "..."], [model functionCall], [user functionResponse], [model "final text"]
    /// ```
    ///
    /// When `maximum_remote_calls` is exhausted, the functions of the last
    /// request are not called (no request is left to send their result
    /// with) and the turn ends on the model's unanswered `functionCall`.
    /// The returned response carries no `automatic_function_calling_history`
    /// because the history is on the chat, as in Python.
    ///
    /// # Errors
    /// See [`crate::models::Models::generate_content`].
    pub async fn send_message(
        &mut self,
        message: impl Into<Contents>,
        config: Option<GenerateContentConfig>,
    ) -> Result<GenerateContentResponse> {
        let mut user_input = to_single_content(message.into());
        let mut contents_to_model = self.curated_history.clone();
        contents_to_model.push(user_input.clone());
        let effective_config = config.or_else(|| self.config.clone());
        let models = self.client.models();

        let callables = registered_tools_for(effective_config.as_ref());
        let afc_enabled = !callables.is_empty() && !should_disable_afc(effective_config.as_ref());
        let mut remaining = if afc_enabled {
            get_max_remote_calls_afc(effective_config.as_ref())?
        } else {
            1
        };

        loop {
            let response = models
                .generate_content_once(
                    &self.model,
                    contents_to_model.clone(),
                    effective_config.clone(),
                )
                .await?;
            remaining -= 1;

            let call_content = first_candidate_content(&response)
                .filter(|content| content.parts.as_ref().is_some_and(|p| !p.is_empty()));
            let call_content = match call_content {
                Some(content) if afc_enabled && remaining > 0 => content,
                _ => {
                    let model_output =
                        first_candidate_content(&response).map_or_else(Vec::new, |c| vec![c]);
                    let is_valid = validate_response(&response);
                    self.record_history(user_input, model_output, is_valid);
                    return Ok(response);
                }
            };

            let response_parts = function_response_parts(&call_content, &callables).await?;
            if response_parts.is_empty() {
                let is_valid = validate_response(&response);
                self.record_history(user_input, vec![call_content], is_valid);
                return Ok(response);
            }
            let response_content = Content {
                role: Some("user".to_owned()),
                parts: Some(response_parts),
            };
            contents_to_model.push(call_content.clone());
            contents_to_model.push(response_content.clone());
            self.record_history(
                std::mem::replace(&mut user_input, response_content),
                vec![call_content],
                validate_response(&response),
            );
        }
    }

    /// Sends `message` plus the accumulated curated history to the model,
    /// streaming incremental response chunks. The returned [`ChatStream`]
    /// borrows this [`Chat`] mutably and finalizes the model's turn into
    /// history once it is fully drained. Mirrors Python's
    /// `Chat.send_message_stream`.
    ///
    /// Automatic function calling runs across the stream as in Python: when
    /// a round's chunks contain `functionCall` parts and AFC is enabled,
    /// the functions are called once the round is drained and another
    /// streaming request carries their results, all within the one
    /// returned stream. Every chunk of every round is yielded. When
    /// `maximum_remote_calls` is exhausted the last round's functions are
    /// not called and the turn ends on the unanswered `functionCall`.
    ///
    /// # Errors
    /// See [`crate::models::Models::generate_content_stream`]. A failure
    /// of a later AFC round, or of a called function, is yielded as the
    /// stream's last item.
    pub async fn send_message_stream(
        &mut self,
        message: impl Into<Contents>,
        config: Option<GenerateContentConfig>,
    ) -> Result<ChatStream<'_>> {
        let user_input = to_single_content(message.into());
        let mut contents_to_model = self.curated_history.clone();
        contents_to_model.push(user_input.clone());
        let effective_config = config.or_else(|| self.config.clone());
        let models = self.client.models();

        let callables = registered_tools_for(effective_config.as_ref());
        let afc_enabled = !callables.is_empty() && !should_disable_afc(effective_config.as_ref());
        let remaining = if afc_enabled {
            get_max_remote_calls_afc(effective_config.as_ref())?
        } else {
            1
        };

        // The first request is issued eagerly so that a failure to start it
        // is this method's `Err`, not the stream's first item.
        let first_round = Box::pin(models.generate_content_stream_once(
            &self.model,
            contents_to_model.clone(),
            effective_config.clone(),
        ))
        .await?;

        let model = self.model.clone();
        let chat = self;
        let inner = async_stream::stream! {
            let mut user_input = user_input;
            let mut remaining = remaining;
            let mut first_round = Some(first_round);
            let mut model_output: Vec<Content> = Vec::new();
            let mut is_valid = true;
            let mut saw_finish_reason = false;

            while remaining > 0 {
                let mut round = match first_round.take() {
                    Some(round) => round,
                    None => match models
                        .generate_content_stream_once(
                            &model,
                            contents_to_model.clone(),
                            effective_config.clone(),
                        )
                        .await
                    {
                        Ok(round) => round,
                        Err(error) => {
                            yield Err(error);
                            return;
                        }
                    },
                };
                remaining -= 1;
                let is_last_round = remaining == 0;

                let mut state = RoundState::new();

                while let Some(item) = round.next().await {
                    let chunk = match item {
                        Ok(chunk) => chunk,
                        // A mid-stream failure does not finalize history
                        // (the exchange never completed), as in Python.
                        Err(error) => {
                            yield Err(error);
                            return;
                        }
                    };
                    let tools = (afc_enabled && !is_last_round).then_some(&callables);
                    if let Err(error) = state.absorb(&chunk, tools).await {
                        yield Err(error);
                        return;
                    }
                    yield Ok(chunk);
                }
                model_output = state.model_output;
                is_valid = state.is_valid;
                saw_finish_reason = state.saw_finish_reason;
                let response_parts = state.response_parts;
                let last_chunk_has_content = state.last_chunk_has_content;

                if is_last_round || response_parts.is_empty() {
                    break;
                }
                if last_chunk_has_content {
                    let response_content = Content {
                        role: Some("user".to_owned()),
                        parts: Some(response_parts),
                    };
                    contents_to_model.extend(model_output.iter().cloned());
                    contents_to_model.push(response_content.clone());
                    chat.record_history(
                        std::mem::replace(&mut user_input, response_content),
                        std::mem::take(&mut model_output),
                        is_valid,
                    );
                }
            }

            chat.record_history(user_input, model_output, is_valid && saw_finish_reason);
        };

        Ok(ChatStream {
            inner: Box::pin(inner),
        })
    }
}

/// A stream of incremental [`GenerateContentResponse`] chunks returned by
/// [`Chat::send_message_stream`]. Borrows the originating [`Chat`] for its
/// whole lifetime; once the underlying HTTP stream is exhausted the
/// accumulated model turn is recorded into the chat's history.
pub struct ChatStream<'a> {
    inner: Pin<Box<dyn Stream<Item = Result<GenerateContentResponse>> + Send + 'a>>,
}

impl Stream for ChatStream<'_> {
    type Item = Result<GenerateContentResponse>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

/// What one streaming round has produced so far, folded in chunk by chunk.
struct RoundState {
    /// The first candidate's content of every chunk that had one.
    model_output: Vec<Content>,
    /// Whether every chunk so far was a valid response.
    is_valid: bool,
    /// Whether any chunk carried a finish reason.
    saw_finish_reason: bool,
    /// The function responses for the calls seen so far.
    response_parts: Vec<Part>,
    /// Whether the most recent chunk had candidate content.
    last_chunk_has_content: bool,
}

impl RoundState {
    fn new() -> Self {
        Self {
            model_output: Vec::new(),
            is_valid: true,
            saw_finish_reason: false,
            response_parts: Vec::new(),
            last_chunk_has_content: false,
        }
    }

    /// Folds `chunk` in. When `callables` is given, the functions the chunk
    /// asks for are called and their responses collected.
    ///
    /// # Errors
    /// See [`function_response_parts`].
    async fn absorb(
        &mut self,
        chunk: &GenerateContentResponse,
        callables: Option<&HashMap<String, Arc<dyn FunctionTool>>>,
    ) -> Result<()> {
        if !validate_response(chunk) {
            self.is_valid = false;
        }
        let content = first_candidate_content(chunk);
        if let (Some(callables), Some(content)) = (callables, content.as_ref()) {
            self.response_parts
                .extend(function_response_parts(content, callables).await?);
        }
        self.last_chunk_has_content = content.is_some();
        self.model_output.extend(content);
        if chunk
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .is_some_and(|c| c.finish_reason.is_some())
        {
            self.saw_finish_reason = true;
        }
        Ok(())
    }
}

/// The `FunctionResponse` parts answering every `functionCall` part of
/// `content`, one per call. Mirrors Python's
/// `_extra_utils.get_function_response_parts_async`.
///
/// # Errors
/// See [`crate::extra_utils::function_response_parts`].
async fn function_response_parts(
    content: &Content,
    callables: &HashMap<String, Arc<dyn FunctionTool>>,
) -> Result<Vec<Part>> {
    crate::extra_utils::function_response_parts(
        content.parts.as_deref().unwrap_or_default(),
        callables,
    )
    .await
}

/// Collapses an `impl Into<Contents>` chat message into a single
/// user-authored [`Content`], mirroring Python's `_transformers.t_content`.
/// A single-`Content` input (the common case: a bare string, [`Part`], or
/// `Vec<Part>`) is passed through unchanged; a multi-`Content` input has its
/// parts merged into one user turn.
fn to_single_content(contents: Contents) -> Content {
    let list: Vec<Content> = contents.into();
    let mut iter = list.into_iter();
    let Some(first) = iter.next() else {
        return Content {
            role: Some("user".to_owned()),
            parts: Some(Vec::new()),
        };
    };
    let Some(second) = iter.next() else {
        return first;
    };
    let mut parts = first.parts.unwrap_or_default();
    parts.extend(second.parts.unwrap_or_default());
    for rest in iter {
        parts.extend(rest.parts.unwrap_or_default());
    }
    Content {
        role: Some("user".to_owned()),
        parts: Some(parts),
    }
}

/// The first candidate's `content`, if any.
fn first_candidate_content(response: &GenerateContentResponse) -> Option<Content> {
    response
        .candidates
        .as_ref()
        .and_then(|c| c.first())
        .and_then(|c| c.content.clone())
}

/// Mirrors Python's `_validate_content`: a `Content` is valid iff it has at
/// least one part and none of its parts are the empty default `Part`.
fn validate_content(content: &Content) -> bool {
    match &content.parts {
        None => false,
        Some(parts) => !parts.is_empty() && !parts.iter().any(|p| *p == Part::default()),
    }
}

/// Mirrors Python's `_validate_response`: a response is valid iff it has a
/// first candidate with valid content.
fn validate_response(response: &GenerateContentResponse) -> bool {
    response
        .candidates
        .as_ref()
        .and_then(|c| c.first())
        .and_then(|c| c.content.as_ref())
        .is_some_and(validate_content)
}

/// Mirrors Python's `_extract_curated_history`: walks a comprehensive
/// history and keeps user turns, plus each contiguous run of model turns
/// only if every turn in that run is valid (an invalid run drops its
/// preceding user turn too, since that exchange as a whole failed). A turn
/// whose role is neither `"user"` nor `"model"` is treated as `"user"` (an
/// unset role defaults to `"user"` per the Gemini API), matching the
/// crate's fallible-free `create` signature rather than Python's
/// `ValueError`.
fn extract_curated_history(comprehensive_history: &[Content]) -> Vec<Content> {
    let mut curated = Vec::new();
    let length = comprehensive_history.len();
    let mut i = 0;
    while i < length {
        if comprehensive_history[i].role.as_deref() == Some("model") {
            let mut current_output = Vec::new();
            let mut is_valid = true;
            while i < length && comprehensive_history[i].role.as_deref() == Some("model") {
                current_output.push(comprehensive_history[i].clone());
                if is_valid && !validate_content(&comprehensive_history[i]) {
                    is_valid = false;
                }
                i += 1;
            }
            if is_valid {
                curated.extend(current_output);
            } else if !curated.is_empty() {
                curated.pop();
            }
        } else {
            curated.push(comprehensive_history[i].clone());
            i += 1;
        }
    }
    curated
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

    use super::{Chats, extract_curated_history, validate_content, validate_response};
    use crate::{
        client::Client,
        types::{Content, HttpOptions, Part},
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

    fn chats(server: &MockServer) -> Chats {
        Chats {
            client: test_client(server.uri()),
        }
    }

    // Kept for parity with other test modules that construct a raw
    // SecretString directly; unused here but documents the pattern.
    #[allow(dead_code)]
    fn _unused(s: SecretString) {
        drop(s);
    }

    fn model_content(text: &str) -> Content {
        Content {
            role: Some("model".to_owned()),
            parts: Some(vec![Part::from_text(text)]),
        }
    }

    fn user_content(text: &str) -> Content {
        Content {
            role: Some("user".to_owned()),
            parts: Some(vec![Part::from_text(text)]),
        }
    }

    #[test]
    fn validate_content_rejects_missing_or_empty_parts() {
        assert!(!validate_content(&Content::default()));
        assert!(!validate_content(&Content {
            parts: Some(vec![]),
            role: Some("model".to_owned()),
        }));
        assert!(!validate_content(&Content {
            parts: Some(vec![Part::default()]),
            role: Some("model".to_owned()),
        }));
        assert!(validate_content(&model_content("hi")));
    }

    #[test]
    fn validate_response_requires_a_first_candidate_with_valid_content() {
        assert!(!validate_response(
            &crate::types::GenerateContentResponse::default()
        ));
    }

    #[test]
    fn extract_curated_history_keeps_a_fully_valid_run() {
        let history = vec![user_content("hi"), model_content("hello")];
        let curated = extract_curated_history(&history);
        assert_eq!(curated, history);
    }

    #[test]
    fn extract_curated_history_drops_the_preceding_user_turn_on_an_invalid_model_run() {
        let invalid_model = Content {
            role: Some("model".to_owned()),
            parts: Some(vec![]),
        };
        let history = vec![
            user_content("first"),
            model_content("ok"),
            user_content("second"),
            invalid_model,
        ];
        let curated = extract_curated_history(&history);
        assert_eq!(curated, vec![user_content("first"), model_content("ok")]);
    }

    #[tokio::test]
    async fn send_message_records_both_turns_and_replays_curated_history() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "candidates": [{
                    "content": {"role": "model", "parts": [{"text": "hi there"}]},
                    "finishReason": "STOP"
                }]
            })))
            .expect(2)
            .mount(&server)
            .await;

        let mut chat = chats(&server).create("gemini-2.5-flash", None, None);
        chat.send_message("hello", None).await.unwrap();
        chat.send_message("again", None).await.unwrap();

        assert_eq!(chat.get_history(true).len(), 4);
        assert_eq!(chat.get_history(false).len(), 4);
        assert_eq!(
            chat.get_history(true)[0].parts.as_ref().unwrap()[0]
                .text
                .as_deref(),
            Some("hello")
        );
        assert_eq!(
            chat.get_history(true)[3].parts.as_ref().unwrap()[0]
                .text
                .as_deref(),
            Some("hi there")
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn send_message_excludes_an_invalid_response_from_curated_history_only() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"candidates": []})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut chat = chats(&server).create("gemini-2.5-flash", None, None);
        chat.send_message("hello", None).await.unwrap();

        // Comprehensive history still records the user turn and a
        // placeholder empty model turn; curated history stays empty.
        assert_eq!(chat.get_history(false).len(), 2);
        assert_eq!(chat.get_history(true).len(), 0);
        assert_eq!(chat.get_history(false)[1].role.as_deref(), Some("model"));
        assert_eq!(chat.get_history(false)[1].parts, Some(vec![]));
        server.verify().await;
    }

    /// Pins Python's `chats.py` behavior: after an automatic-function-calling
    /// round-trip every turn lands in the chat history, one exchange per
    /// remote call:
    ///
    /// ```text
    /// [user text], [model functionCall], [user functionResponse], [model text]
    /// ```
    #[tokio::test]
    async fn send_message_with_afc_records_every_turn() {
        /// Arguments of the demo tool registered by this test.
        #[derive(serde::Deserialize, schemars::JsonSchema)]
        struct WeatherArgs {
            location: String,
        }

        let server = MockServer::start().await;
        // First request: the model asks for the tool. Mounted first and
        // capped at one match, so the second mock answers the AFC follow-up.
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "candidates": [{
                    "content": {"role": "model", "parts": [{"functionCall": {
                        "name": "chats_afc_history_get_weather",
                        "args": {"location": "NYC"}
                    }}]},
                    "finishReason": "STOP"
                }]
            })))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        // Second request: the post-tool-call answer.
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "candidates": [{
                    "content": {"role": "model", "parts": [{"text": "It's sunny in NYC."}]},
                    "finishReason": "STOP"
                }]
            })))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;

        let tool = crate::automatic_function_calling_util::function_tool::<WeatherArgs, _, _, _>(
            "chats_afc_history_get_weather",
            "Gets the weather for a location.",
            |args: WeatherArgs| async move {
                Ok(serde_json::json!({"conditions": "sunny", "location": args.location}))
            },
        );
        let config = crate::types::GenerateContentConfig {
            tools: Some(vec![crate::types::Tool::from_function(tool)]),
            ..Default::default()
        };

        let mut chat = chats(&server).create("gemini-2.5-flash", Some(config), None);
        let response = chat
            .send_message("what's the weather in NYC?", None)
            .await
            .unwrap();
        assert_eq!(response.text().as_deref(), Some("It's sunny in NYC."));

        // As in Python, the chat (not the response) carries the AFC turns.
        assert!(response.automatic_function_calling_history.is_none());
        let history = chat.get_history(false);
        assert_eq!(history.len(), 4);
        assert_eq!(history[0].role.as_deref(), Some("user"));
        assert_eq!(history[1].role.as_deref(), Some("model"));
        assert!(
            history[1].parts.as_ref().unwrap()[0]
                .function_call
                .is_some()
        );
        assert_eq!(history[2].role.as_deref(), Some("user"));
        assert!(
            history[2].parts.as_ref().unwrap()[0]
                .function_response
                .is_some()
        );
        assert_eq!(history[3].role.as_deref(), Some("model"));
        assert_eq!(
            history[3].parts.as_ref().unwrap()[0].text.as_deref(),
            Some("It's sunny in NYC.")
        );
        assert_eq!(chat.get_history(true), history);

        server.verify().await;
    }
}
