//! `client.live().music()`: realtime music generation sessions over
//! WebSocket. Mirrors Python's `live_music.py`.

use futures_core::Stream;
use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;

use super::{WsStream, connect_ws, recv_decoded, send_json, websocket_endpoint, ws_err};
use crate::{
    client::Client,
    converters::generated::live_converters as conv,
    error::{Error, Result},
    types::{
        LiveMusicGenerationConfig, LiveMusicPlaybackControl, LiveMusicServerMessage, WeightedPrompt,
    },
};

/// Handle for `client.live().music()`: opens realtime music generation
/// sessions. Mirrors Python's `AsyncLiveMusic`.
#[derive(Clone)]
pub struct LiveMusic {
    pub(crate) client: Client,
}

impl LiveMusic {
    /// Opens a realtime music generation session with `model`, performing
    /// the `setup` / `setupComplete` handshake before returning. Mirrors
    /// Python's `AsyncLiveMusic.connect`.
    ///
    /// # Errors
    /// Returns [`Error::WebSocket`] for a connection/handshake failure, or
    /// [`Error::Stream`] if the first server message is not
    /// `setupComplete`.
    pub async fn connect(&self, model: &str) -> Result<LiveMusicSession> {
        // Unlike `live_connect_parameters_to_mldev`, the music variant of
        // this converter does not normalize `model` itself (mirrors
        // Python's `live_music.py`, which calls `t.t_model` at the call
        // site before building `LiveMusicConnectParameters`).
        let model = crate::transformers::t_model(Value::String(model.to_owned()))?;
        let params = serde_json::json!({ "model": model });
        let request = conv::live_music_connect_parameters_to_mldev(&params, None, None)?;

        let (url, auth_header) = websocket_endpoint(&self.client, "BidiGenerateMusic");
        let mut ws = connect_ws(&url, auth_header.as_deref()).await?;
        send_json(&mut ws, &request).await?;

        match recv_decoded(&mut ws).await? {
            Some(raw) => {
                let message: LiveMusicServerMessage = serde_json::from_value(raw.clone())
                    .map_err(|_| Error::Stream(format!("failed to parse setupComplete: {raw}")))?;
                if message.setup_complete.is_none() {
                    return Err(Error::Stream(format!(
                        "expected `setupComplete` as the first Live Music message, got: {raw}"
                    )));
                }
            }
            None => {
                return Err(Error::Stream(
                    "connection closed before `setupComplete` was received".to_owned(),
                ));
            }
        }

        let (sink, stream) = ws.split();
        Ok(LiveMusicSession { sink, stream })
    }
}

/// An open realtime music generation session, returned by
/// [`LiveMusic::connect`]. Mirrors Python's `AsyncMusicSession`.
#[derive(Debug)]
pub struct LiveMusicSession {
    sink: SplitSink<WsStream, Message>,
    stream: SplitStream<WsStream>,
}

impl LiveMusicSession {
    /// Sets the weighted text prompts steering music generation. Mirrors
    /// Python's `AsyncMusicSession.set_weighted_prompts`.
    ///
    /// # Errors
    /// Returns [`Error::WebSocket`] if the send fails.
    pub async fn set_weighted_prompts(&mut self, prompts: Vec<WeightedPrompt>) -> Result<()> {
        let params = serde_json::json!({ "weighted_prompts": prompts });
        let mldev = conv::live_music_set_weighted_prompts_parameters_to_mldev(&params, None, None)?;
        self.send_wire("clientContent", mldev).await
    }

    /// Sets the music generation configuration. Mirrors Python's
    /// `AsyncMusicSession.set_music_generation_config`.
    ///
    /// # Errors
    /// Returns [`Error::WebSocket`] if the send fails.
    pub async fn set_music_generation_config(
        &mut self,
        config: LiveMusicGenerationConfig,
    ) -> Result<()> {
        let params = serde_json::json!({ "music_generation_config": config });
        // Already the full wire envelope: `{"musicGenerationConfig": {...}}`.
        let mldev = conv::live_music_set_config_parameters_to_mldev(&params, None, None)?;
        send_json(&mut self.sink, &mldev).await
    }

    /// Sends a playback signal to start the music stream. Mirrors
    /// Python's `AsyncMusicSession.play`.
    ///
    /// # Errors
    /// Returns [`Error::WebSocket`] if the send fails.
    pub async fn play(&mut self) -> Result<()> {
        self.send_playback_control(LiveMusicPlaybackControl::Play)
            .await
    }

    /// Sends a playback signal to pause the music stream. Mirrors
    /// Python's `AsyncMusicSession.pause`.
    ///
    /// # Errors
    /// Returns [`Error::WebSocket`] if the send fails.
    pub async fn pause(&mut self) -> Result<()> {
        self.send_playback_control(LiveMusicPlaybackControl::Pause)
            .await
    }

    /// Sends a playback signal to stop the music stream, resetting the
    /// generation context while retaining the current config. Mirrors
    /// Python's `AsyncMusicSession.stop`.
    ///
    /// # Errors
    /// Returns [`Error::WebSocket`] if the send fails.
    pub async fn stop(&mut self) -> Result<()> {
        self.send_playback_control(LiveMusicPlaybackControl::Stop)
            .await
    }

    /// Resets the generation context (prompts retained) without stopping
    /// music generation. Mirrors Python's
    /// `AsyncMusicSession.reset_context`.
    ///
    /// # Errors
    /// Returns [`Error::WebSocket`] if the send fails.
    pub async fn reset_context(&mut self) -> Result<()> {
        self.send_playback_control(LiveMusicPlaybackControl::ResetContext)
            .await
    }

    /// Yields server messages (audio chunks, filtered-prompt
    /// notifications) as they arrive. Ends (without an `Err`) when the
    /// server closes the connection; a mid-stream transport error yields
    /// one `Err` item and then ends the stream. Mirrors Python's
    /// `AsyncMusicSession.receive`.
    pub fn receive(&mut self) -> impl Stream<Item = Result<LiveMusicServerMessage>> + '_ {
        async_stream::try_stream! {
            loop {
                match recv_decoded(&mut self.stream).await {
                    Ok(Some(raw)) => yield serde_json::from_value(raw)?,
                    Ok(None) => {
                        tracing::debug!("Live Music session closed by the server");
                        break;
                    }
                    Err(err) => {
                        tracing::debug!(error = %err, "Live Music websocket error; ending stream");
                        Err(err)?;
                        break;
                    }
                }
            }
        }
    }

    /// Closes the session, sending a WebSocket close frame. Mirrors
    /// Python's `AsyncMusicSession.close`.
    ///
    /// # Errors
    /// Returns [`Error::WebSocket`] if the close frame could not be sent.
    pub async fn close(mut self) -> Result<()> {
        self.sink.close().await.map_err(ws_err)
    }

    async fn send_playback_control(&mut self, control: LiveMusicPlaybackControl) -> Result<()> {
        let value = serde_json::to_value(control)?;
        self.send_wire("playbackControl", value).await
    }

    async fn send_wire(&mut self, key: &str, payload: Value) -> Result<()> {
        let message = Value::Object(serde_json::Map::from_iter([(key.to_owned(), payload)]));
        send_json(&mut self.sink, &message).await
    }
}
