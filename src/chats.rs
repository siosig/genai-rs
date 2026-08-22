//! `client.chats()`: multi-turn chat sessions. Mirrors Python's `chats.py`.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;

use crate::client::Client;
use crate::error::Result;
use crate::types::{Content, Contents, GenerateContentConfig, GenerateContentResponse, Part};

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
    /// Sends `message` plus the accumulated curated history to the model
    /// and returns its response. Mirrors Python's `Chat.send_message`.
    ///
    /// Automatic function calling applies here exactly as it does for
    /// [`crate::models::Models::generate_content`], which this delegates
    /// to: if `config.tools` contains a tool built by
    /// [`crate::types::Tool::from_function`], the AFC loop runs and only
    /// the final, post-tool-call response is recorded into history.
    ///
    /// # Errors
    /// See [`crate::models::Models::generate_content`].
    pub async fn send_message(
        &mut self,
        message: impl Into<Contents>,
        config: Option<GenerateContentConfig>,
    ) -> Result<GenerateContentResponse> {
        let user_input = to_single_content(message.into());
        let mut contents_to_model = self.curated_history.clone();
        contents_to_model.push(user_input.clone());
        let effective_config = config.or_else(|| self.config.clone());

        let response = self
            .client
            .models()
            .generate_content(&self.model, contents_to_model, effective_config)
            .await?;

        let model_output = first_candidate_content(&response).map_or_else(Vec::new, |c| vec![c]);
        let is_valid = validate_response(&response);
        self.record_history(user_input, model_output, is_valid);
        Ok(response)
    }

    /// Sends `message` plus the accumulated curated history to the model,
    /// streaming incremental response chunks. The returned [`ChatStream`]
    /// borrows this [`Chat`] mutably and finalizes the model's turn into
    /// history once it is fully drained. Mirrors Python's
    /// `Chat.send_message_stream`.
    ///
    /// Unlike [`Self::send_message`], automatic function calling does
    /// **not** run here: [`crate::models::Models::generate_content_stream`]
    /// issues a single streaming request, matching Python, which likewise
    /// only drives the AFC loop from the unary path. A `functionCall` part
    /// is surfaced to the caller as-is.
    ///
    /// # Errors
    /// See [`crate::models::Models::generate_content_stream`].
    pub async fn send_message_stream(
        &mut self,
        message: impl Into<Contents>,
        config: Option<GenerateContentConfig>,
    ) -> Result<ChatStream<'_>> {
        let user_input = to_single_content(message.into());
        let mut contents_to_model = self.curated_history.clone();
        contents_to_model.push(user_input.clone());
        let effective_config = config.or_else(|| self.config.clone());

        let inner = self
            .client
            .models()
            .generate_content_stream(&self.model, contents_to_model, effective_config)
            .await?;

        Ok(ChatStream {
            chat: self,
            inner: Box::pin(inner),
            user_input,
            model_output: Vec::new(),
            is_valid: true,
            saw_finish_reason: false,
            finished: false,
        })
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
}

/// A stream of incremental [`GenerateContentResponse`] chunks returned by
/// [`Chat::send_message_stream`]. Borrows the originating [`Chat`] for its
/// whole lifetime; once the underlying HTTP stream is exhausted the
/// accumulated model turn is recorded into the chat's history.
pub struct ChatStream<'a> {
    chat: &'a mut Chat,
    inner: Pin<Box<dyn Stream<Item = Result<GenerateContentResponse>> + Send>>,
    user_input: Content,
    model_output: Vec<Content>,
    is_valid: bool,
    saw_finish_reason: bool,
    finished: bool,
}

impl Stream for ChatStream<'_> {
    type Item = Result<GenerateContentResponse>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                if !validate_response(&chunk) {
                    self.is_valid = false;
                }
                if let Some(content) = first_candidate_content(&chunk) {
                    self.model_output.push(content);
                }
                if chunk
                    .candidates
                    .as_ref()
                    .and_then(|c| c.first())
                    .is_some_and(|c| c.finish_reason.is_some())
                {
                    self.saw_finish_reason = true;
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(err))) => {
                // Mirrors Python: a mid-stream failure does not finalize
                // history (the exchange never completed).
                self.finished = true;
                Poll::Ready(Some(Err(err)))
            }
            Poll::Ready(None) => {
                self.finished = true;
                let is_valid =
                    self.is_valid && !self.model_output.is_empty() && self.saw_finish_reason;
                let user_input = std::mem::take(&mut self.user_input);
                let model_output = std::mem::take(&mut self.model_output);
                self.chat.record_history(user_input, model_output, is_valid);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
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
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::{Chats, extract_curated_history, validate_content, validate_response};
    use crate::client::Client;
    use crate::types::{Content, HttpOptions, Part};

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
}
