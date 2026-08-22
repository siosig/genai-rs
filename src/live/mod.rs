//! `client.live()`: bidirectional realtime (Live API) sessions over WebSocket. Mirrors Python's `live.py`.

pub mod music;

use futures_core::Stream;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use secrecy::ExposeSecret;
use serde_json::{Map, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use crate::client::Client;
use crate::converters::generated::live_converters as conv;
use crate::error::{Error, Result};
use crate::types::{
    Content, FunctionResponse, LiveConnectConfig, LiveServerMessage, LiveServerSetupComplete,
};

pub(crate) type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Handle for `client.live()`: opens bidirectional realtime sessions and
/// exposes `client.live().music()` for realtime music generation. Mirrors
/// Python's `AsyncLive`.
#[derive(Clone)]
pub struct Live {
    pub(crate) client: Client,
}

impl Live {
    /// Opens a realtime bidirectional session with `model`, performing the
    /// `setup` / `setupComplete` handshake before returning. Mirrors
    /// Python's `AsyncLive.connect`.
    ///
    /// # Errors
    /// Returns [`Error::UnsupportedByBackend`] if `config` sets a field
    /// only the Vertex AI backend supports, [`Error::WebSocket`] for a
    /// connection/handshake failure, or [`Error::Stream`] if the first
    /// server message is not `setupComplete`.
    pub async fn connect(
        &self,
        model: &str,
        config: Option<LiveConnectConfig>,
    ) -> Result<LiveSession> {
        let params = serde_json::json!({ "model": model, "config": config });
        let mut request = conv::live_connect_parameters_to_mldev(&params, None, None)?;
        // Mirrors Python's `del request_dict['config']`: `config` is an
        // artifact of how the converter threads writes through to
        // `setup.*` via its `parent_object` parameter (see
        // `live_connect_parameters_to_mldev`'s source) and is never part
        // of the wire message.
        if let Some(obj) = request.as_object_mut() {
            obj.remove("config");
        }

        let (url, auth_header) = websocket_endpoint(&self.client, "BidiGenerateContent");
        let mut ws = connect_ws(&url, auth_header.as_deref()).await?;
        send_json(&mut ws, &request).await?;

        let setup_complete = match recv_decoded(&mut ws).await? {
            Some(raw) => {
                let mldev = conv::live_server_message_from_mldev(&raw, None, None)?;
                let message: LiveServerMessage = serde_json::from_value(mldev)?;
                message.setup_complete.ok_or_else(|| {
                    Error::Stream(format!(
                        "expected `setupComplete` as the first Live API message, got: {raw}"
                    ))
                })?
            }
            None => {
                return Err(Error::Stream(
                    "connection closed before `setupComplete` was received".to_owned(),
                ));
            }
        };

        let (sink, stream) = ws.split();
        Ok(LiveSession {
            sink,
            stream,
            setup_complete: Some(setup_complete),
        })
    }

    /// Returns the handle for `client.live().music()`: realtime music
    /// generation sessions. Mirrors Python's `AsyncLive.music`.
    #[must_use]
    pub fn music(&self) -> music::LiveMusic {
        music::LiveMusic {
            client: self.client.clone(),
        }
    }
}

/// An open bidirectional Live API session, returned by [`Live::connect`].
/// Mirrors Python's `AsyncSession`.
#[derive(Debug)]
pub struct LiveSession {
    sink: SplitSink<WsStream, Message>,
    stream: SplitStream<WsStream>,
    setup_complete: Option<LiveServerSetupComplete>,
}

impl LiveSession {
    /// Sends non-realtime, turn-based content to the model. Mirrors
    /// Python's `AsyncSession.send_client_content`.
    ///
    /// # Errors
    /// Returns [`Error::WebSocket`] if the send fails (e.g. the session is
    /// no longer connected).
    pub async fn send_client_content(
        &mut self,
        turns: Option<Vec<Content>>,
        turn_complete: bool,
    ) -> Result<()> {
        let content = crate::types::LiveClientContent {
            turns,
            turn_complete: Some(turn_complete),
        };
        let params = serde_json::to_value(&content)?;
        let mldev = conv::live_client_content_to_mldev(&params, None, None)?;
        self.send_wire("clientContent", mldev).await
    }

    /// Sends realtime input (audio/video/text chunks or activity
    /// start/end markers) to the model; only one field of `input` should
    /// be set per call. Mirrors Python's `AsyncSession.send_realtime_input`.
    ///
    /// # Errors
    /// Returns [`Error::WebSocket`] if the send fails.
    pub async fn send_realtime_input(&mut self, input: RealtimeInput) -> Result<()> {
        let params = serde_json::to_value(&input)?;
        let mldev = conv::live_send_realtime_input_parameters_to_mldev(&params, None, None)?;
        self.send_wire("realtimeInput", mldev).await
    }

    /// Sends the client's responses to a `LiveServerToolCall`. Mirrors
    /// Python's `AsyncSession.send_tool_response`.
    ///
    /// # Errors
    /// Returns [`Error::Validation`] if a response is missing its `id`, or
    /// [`Error::WebSocket`] if the send fails.
    pub async fn send_tool_response(
        &mut self,
        function_responses: Vec<FunctionResponse>,
    ) -> Result<()> {
        let mut items = Vec::with_capacity(function_responses.len());
        for response in &function_responses {
            if response.id.is_none() {
                return Err(Error::Validation(
                    "FunctionResponse.id is required when responding to a Live API tool call"
                        .to_owned(),
                ));
            }
            items.push(camelize_function_response(serde_json::to_value(response)?));
        }
        let payload = Value::Object(Map::from_iter([(
            "functionResponses".to_owned(),
            Value::Array(items),
        )]));
        self.send_wire("toolResponse", payload).await
    }

    /// Yields server messages as they arrive, transparently passing
    /// through `turn_complete` and `goAway` messages. Ends (without an
    /// `Err`) when the server closes the connection; a mid-stream
    /// transport error yields one `Err` item and then ends the stream.
    /// Mirrors Python's `AsyncSession.receive`.
    pub fn receive(&mut self) -> impl Stream<Item = Result<LiveServerMessage>> + '_ {
        async_stream::try_stream! {
            loop {
                match recv_decoded(&mut self.stream).await {
                    Ok(Some(raw)) => {
                        let mldev = conv::live_server_message_from_mldev(&raw, None, None)?;
                        yield serde_json::from_value(mldev)?;
                    }
                    Ok(None) => {
                        tracing::debug!("Live API session closed by the server");
                        break;
                    }
                    Err(err) => {
                        tracing::debug!(error = %err, "Live API websocket error; ending stream");
                        Err(err)?;
                        break;
                    }
                }
            }
        }
    }

    /// Closes the session, sending a WebSocket close frame. Mirrors
    /// Python's `AsyncSession.close`.
    ///
    /// # Errors
    /// Returns [`Error::WebSocket`] if the close frame could not be sent.
    pub async fn close(mut self) -> Result<()> {
        self.sink.close().await.map_err(ws_err)
    }

    /// The server's response to the initial `setup` handshake message.
    #[must_use]
    pub fn setup_complete(&self) -> Option<&LiveServerSetupComplete> {
        self.setup_complete.as_ref()
    }

    async fn send_wire(&mut self, key: &str, payload: Value) -> Result<()> {
        let message = Value::Object(Map::from_iter([(key.to_owned(), payload)]));
        send_json(&mut self.sink, &message).await
    }
}

/// User input sent in real time via [`LiveSession::send_realtime_input`].
/// Only one field should be set per call. Mirrors Python's
/// `send_realtime_input`'s keyword arguments
/// (`types.LiveSendRealtimeInputParameters`).
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RealtimeInput {
    /// A `Blob`-like realtime media chunk (image or audio); its MIME type
    /// determines how the server interprets it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<crate::types::Blob>,
    /// A realtime audio chunk (MIME type must start with `audio/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<crate::types::Blob>,
    /// Marks the end of the realtime audio stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_stream_end: Option<bool>,
    /// A realtime video frame (MIME type must start with `image/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<crate::types::Blob>,
    /// A realtime text chunk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Marks the start of user activity (manual voice-activity detection).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_start: Option<crate::types::ActivityStart>,
    /// Marks the end of user activity (manual voice-activity detection).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_end: Option<crate::types::ActivityEnd>,
}

/// Converts a `tokio_tungstenite`/`tungstenite` transport error into
/// [`Error::WebSocket`].
pub(crate) fn ws_err(err: tokio_tungstenite::tungstenite::Error) -> Error {
    Error::WebSocket(Box::new(err))
}

/// Builds the Live API WebSocket URL for `service_method` (e.g.
/// `BidiGenerateContent`, `BidiGenerateMusic`), resolving the
/// `auth_tokens/`-prefixed ephemeral-token special case: an ephemeral
/// token is sent as an `Authorization: Token <key>` header (no `?key=`
/// query param), and `BidiGenerateContent` becomes
/// `BidiGenerateContentConstrained`. Returns the URL and, for the
/// ephemeral-token case, the `Authorization` header value to send.
///
/// # The returned URL contains the API key
///
/// **Never log this URL, and never put it in an error message, a panic
/// payload, or a `Debug` output.**
///
/// Unless the key is an ephemeral `auth_tokens/` token, it is embedded in the
/// query string as `?key=<api key>`. That is the Live API's protocol, not a
/// choice this crate makes -- the handshake is a plain WebSocket upgrade with
/// nowhere else to put credentials -- so the only available mitigation is to
/// keep the string from escaping.
///
/// Today nothing leaks it: this module does not log the URL, and
/// `tokio_tungstenite::connect_async` does not include it in the errors it
/// returns (the `UrlError::UnableToConnect(url)` variant that would is only
/// produced by the synchronous client). **Do not rely on that.** It is a
/// property of a dependency's current implementation, one `{err}` away from
/// changing, and an API key in a log line is a credential leak the moment the
/// log is shipped anywhere.
///
/// If you need to report a connection failure, report the host and the service
/// method -- both are in [`HttpOptions::base_url`](crate::types::HttpOptions)
/// and the `service_method` argument -- not this return value.
pub(crate) fn websocket_endpoint(
    client: &Client,
    service_method: &str,
) -> (String, Option<String>) {
    let http = client.http();
    let ws_base = http
        .base_url()
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let ws_base = ws_base.trim_end_matches('/');
    let version = http.api_version();
    let api_key = http.api_key().expose_secret().to_owned();

    let (method, query, auth_header) = if api_key.starts_with("auth_tokens/") {
        let method = if service_method == "BidiGenerateContent" {
            "BidiGenerateContentConstrained"
        } else {
            service_method
        };
        (method, String::new(), Some(format!("Token {api_key}")))
    } else {
        (service_method, format!("?key={api_key}"), None)
    };

    (
        format!(
            "{ws_base}/ws/google.ai.generativelanguage.{version}.GenerativeService.{method}{query}"
        ),
        auth_header,
    )
}

/// Opens the WebSocket connection, attaching `auth_header` as an
/// `Authorization` header when present.
pub(crate) async fn connect_ws(url: &str, auth_header: Option<&str>) -> Result<WsStream> {
    let mut request = url.into_client_request().map_err(ws_err)?;
    if let Some(value) = auth_header {
        let header_value = HeaderValue::from_str(value)
            .map_err(|e| Error::Validation(format!("invalid Authorization header value: {e}")))?;
        request.headers_mut().insert(AUTHORIZATION, header_value);
    }
    let (ws, _response) = connect_async(request).await.map_err(ws_err)?;
    Ok(ws)
}

/// Serializes `value` and sends it as a single WebSocket text frame.
pub(crate) async fn send_json<S>(sink: &mut S, value: &Value) -> Result<()>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let text = serde_json::to_string(value)?;
    sink.send(Message::text(text)).await.map_err(ws_err)
}

/// Reads and JSON-decodes the next text/binary frame from `stream`,
/// transparently skipping ping/pong/raw frames. Returns `Ok(None)` when
/// the peer closes the connection (cleanly or abruptly) rather than
/// treating that as an error, per the Live API contract.
pub(crate) async fn recv_decoded<S>(stream: &mut S) -> Result<Option<Value>>
where
    S: Stream<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        match stream.next().await {
            None
            | Some(
                Ok(Message::Close(_))
                | Err(
                    tokio_tungstenite::tungstenite::Error::ConnectionClosed
                    | tokio_tungstenite::tungstenite::Error::AlreadyClosed,
                ),
            ) => return Ok(None),
            Some(Ok(Message::Text(text))) => return Ok(Some(serde_json::from_str(&text)?)),
            Some(Ok(Message::Binary(bytes))) => {
                return Ok(Some(serde_json::from_slice(&bytes)?));
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
            Some(Err(err)) => return Err(ws_err(err)),
        }
    }
}

/// Renames known struct field keys of a `FunctionResponse`-shaped JSON
/// value from `snake_case` to `camelCase`, matching pydantic's
/// `alias_generator=to_camel` (mirrors what `model_dump(by_alias=True)`
/// does in Python's `send_tool_response`). Values under the `response`
/// key are arbitrary user data (`Dict[str, Any]` in Python), not model
/// fields, and are left untouched.
fn camelize_function_response(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, val)| {
                    let val = if key == "response" {
                        val
                    } else {
                        camelize_function_response(val)
                    };
                    (snake_to_camel(&key), val)
                })
                .collect(),
        ),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(camelize_function_response).collect())
        }
        other => other,
    }
}

/// Converts a `snake_case` identifier to `camelCase`.
fn snake_to_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper_next = false;
    for ch in s.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use serde_json::json;

    use super::{camelize_function_response, snake_to_camel, websocket_endpoint};
    use crate::client::Client;
    use crate::http::HttpClient;
    use crate::types::HttpOptions;

    fn client_with(api_key: &str, base_url: &str) -> Client {
        Client::builder()
            .api_key(api_key)
            .http_options(HttpOptions {
                base_url: Some(base_url.to_owned()),
                ..Default::default()
            })
            .build()
            .unwrap()
    }

    #[test]
    fn snake_to_camel_converts_underscored_identifiers() {
        assert_eq!(snake_to_camel("turn_complete"), "turnComplete");
        assert_eq!(snake_to_camel("will_continue"), "willContinue");
        assert_eq!(snake_to_camel("id"), "id");
    }

    #[test]
    fn websocket_endpoint_uses_query_key_for_a_plain_api_key() {
        let client = client_with("plain-key", "http://example.test/");
        let (url, auth_header) = websocket_endpoint(&client, "BidiGenerateContent");
        assert_eq!(
            url,
            "ws://example.test/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key=plain-key"
        );
        assert!(auth_header.is_none());
    }

    #[test]
    fn websocket_endpoint_uses_music_method_verbatim() {
        let client = client_with("plain-key", "https://example.test");
        let (url, _) = websocket_endpoint(&client, "BidiGenerateMusic");
        assert!(url.starts_with("wss://example.test/ws/"));
        assert!(url.contains("BidiGenerateMusic"));
        assert!(!url.contains("Constrained"));
    }

    #[test]
    fn websocket_endpoint_uses_authorization_header_for_an_ephemeral_token() {
        let client = client_with("auth_tokens/xyz", "https://example.test/");
        let (url, auth_header) = websocket_endpoint(&client, "BidiGenerateContent");
        assert!(url.contains("BidiGenerateContentConstrained"));
        assert!(!url.contains("key="));
        assert_eq!(auth_header.as_deref(), Some("Token auth_tokens/xyz"));
    }

    #[test]
    fn websocket_endpoint_swaps_https_and_http_schemes() {
        let https_client = client_with("k", "https://example.test/");
        assert!(
            websocket_endpoint(&https_client, "BidiGenerateContent")
                .0
                .starts_with("wss://")
        );

        let plaintext_client = client_with("k", "http://example.test/");
        assert!(
            websocket_endpoint(&plaintext_client, "BidiGenerateContent")
                .0
                .starts_with("ws://")
        );
    }

    #[test]
    fn camelize_function_response_renames_known_fields_but_not_response_contents() {
        let value = json!({
            "will_continue": true,
            "id": "call-1",
            "response": {"snake_case_key": "left alone"},
        });
        let camelized = camelize_function_response(value);
        assert_eq!(
            camelized,
            json!({
                "willContinue": true,
                "id": "call-1",
                "response": {"snake_case_key": "left alone"},
            })
        );
    }

    #[test]
    fn http_client_accessor_exposes_api_key_and_base_url() {
        // Sanity check on the accessors `websocket_endpoint` relies on,
        // independent of `Client`'s wiring.
        let http =
            HttpClient::new(SecretString::from("k".to_owned()), &HttpOptions::default()).unwrap();
        assert_eq!(
            http.base_url(),
            "https://generativelanguage.googleapis.com/"
        );
        assert_eq!(http.api_version(), "v1beta");
    }
}
