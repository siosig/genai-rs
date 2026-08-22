//! In-process mock WebSocket server for the Live API integration tests
//! (`tests/live.rs`, `tests/live_music.rs`): binds an OS-assigned
//! localhost port, accepts a single connection, and hands the upgraded
//! stream plus the raw HTTP handshake request (URI/headers, for asserting
//! on `?key=...` / `Authorization`) to a caller-supplied async handler.

#![allow(
    dead_code,
    reason = "not every test binary that includes this module uses every helper"
)]

use std::future::Future;

use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

/// The HTTP request a client sent to establish the WebSocket handshake.
#[derive(Debug, Clone)]
pub struct HandshakeRequest {
    /// The request URI, including path and query string (e.g.
    /// `/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key=test-key`).
    pub uri: String,
    /// All request headers as `(name, value)` pairs (non-UTF-8 values are
    /// reported as an empty string).
    pub headers: Vec<(String, String)>,
}

impl HandshakeRequest {
    /// Returns the (case-insensitive) value of the first header named
    /// `name`, if present.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Starts a mock WebSocket server on `127.0.0.1` (an OS-assigned port),
/// accepts exactly one connection, and runs `handler` against the
/// resulting stream and the captured handshake request. Returns the
/// server's `http://127.0.0.1:<port>` base URL (suitable for
/// `HttpOptions::base_url`, which `Live::connect`/`LiveMusic::connect`
/// swap to `ws://`) and a `JoinHandle` the caller must `.await` after
/// driving the client side, so any assertion failure inside `handler`
/// fails the test instead of being silently dropped.
#[expect(
    clippy::expect_used,
    reason = "test infrastructure: a failure here means the mock server itself is broken, not the code under test"
)]
#[expect(
    clippy::result_large_err,
    reason = "tungstenite::Error size is fixed by the tokio-tungstenite crate, not under this test helper's control"
)]
pub async fn start_mock_ws_server<F, Fut>(handler: F) -> (String, JoinHandle<()>)
where
    F: FnOnce(WebSocketStream<TcpStream>, HandshakeRequest) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock ws server");
    let port = listener.local_addr().expect("local_addr").port();

    let join = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept mock ws connection");

        let (tx, rx) = tokio::sync::oneshot::channel::<HandshakeRequest>();
        let callback = move |req: &Request, resp: Response| {
            let headers = req
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.to_string(),
                        value.to_str().unwrap_or_default().to_owned(),
                    )
                })
                .collect();
            let _ = tx.send(HandshakeRequest {
                uri: req.uri().to_string(),
                headers,
            });
            Ok(resp)
        };
        let ws = tokio_tungstenite::accept_hdr_async(stream, callback)
            .await
            .expect("ws handshake");
        let request = rx
            .await
            .expect("handshake callback ran before accept_hdr_async resolved");

        handler(ws, request).await;
    });

    (format!("http://127.0.0.1:{port}"), join)
}
