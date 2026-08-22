//! Server-Sent Events parsing for streaming responses
//! (`?alt=sse`), mirroring Python's `_api_client.py` streaming handling:
//! only `data: ` lines are treated as payload; everything else is ignored.

use bytes::Bytes;
use eventsource_stream::Eventsource;
use futures_core::Stream;
use futures_util::StreamExt;
use serde_json::Value;

use crate::error::Error;

/// Converts a raw byte stream (an SSE response body) into a stream of
/// decoded JSON values, one per `data:` event.
pub(crate) fn parse_sse<S, E>(byte_stream: S) -> impl Stream<Item = Result<Value, Error>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    byte_stream
        .map(|chunk| chunk.map_err(|e| std::io::Error::other(e.to_string())))
        .eventsource()
        .map(|event| match event {
            Ok(event) => serde_json::from_str::<Value>(&event.data).map_err(Error::from),
            Err(err) => Err(Error::Stream(err.to_string())),
        })
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};

    use futures_util::{StreamExt, stream};

    use super::parse_sse;
    use crate::error::Error;

    fn chunks(
        data: &[&str],
    ) -> impl futures_core::Stream<Item = Result<bytes::Bytes, std::io::Error>> {
        let owned: Vec<Result<bytes::Bytes, std::io::Error>> = data
            .iter()
            .map(|s| Ok(bytes::Bytes::from((*s).to_owned())))
            .collect();
        stream::iter(owned)
    }

    #[tokio::test]
    async fn parses_data_only_events_in_order() {
        let raw = chunks(&["data: {\"a\":1}\n\n", "event: ping\ndata: {\"a\":2}\n\n"]);
        let values: Vec<_> = parse_sse(raw).map(|r| r.unwrap()).collect().await;
        assert_eq!(values.len(), 2);
        assert_eq!(values[0]["a"], 1);
        assert_eq!(values[1]["a"], 2);
    }

    #[tokio::test]
    async fn non_json_data_yields_an_error() {
        let raw = chunks(&["data: not json\n\n"]);
        let values: Vec<_> = parse_sse(raw).collect().await;
        assert_eq!(values.len(), 1);
        assert!(values[0].is_err());
    }

    /// Serves exactly one raw HTTP response on a loopback socket and hands
    /// back its address. `response` is written verbatim -- headers included
    /// -- and the connection is then closed, which is what makes it possible
    /// to hang up in the middle of a body that a `Content-Length` header
    /// promised was longer (i.e. to simulate a mid-stream disconnect, which
    /// neither `wiremock` nor a well-behaved server can produce).
    ///
    /// The `unwrap`s below are covered by `clippy.toml`'s
    /// `allow-unwrap-in-tests`, which applies to everything under this
    /// `#[cfg(test)]` module; a loopback bind failure here is a broken test
    /// environment, not a runtime condition.
    fn serve_raw_once(response: &'static str) -> std::net::SocketAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let Ok((mut socket, _)) = listener.accept() else {
                return;
            };
            // Read the request line/headers so the client's write completes;
            // the content is irrelevant to what we serve back.
            let mut buffer = [0_u8; 2048];
            let _ = socket.read(&mut buffer);
            let _ = socket.write_all(response.as_bytes());
            let _ = socket.flush();
            drop(socket);
        });
        addr
    }

    /// Contract (`contracts/wire-protocol.md`, its SSE streaming section): on a
    /// mid-stream disconnect the chunks received so far have already been
    /// yielded, and the stream then yields `Err(Error::Stream)` *once* and
    /// ends -- neither a silent truncation nor an error that repeats
    /// forever.
    ///
    /// This drives the decoder over a real `reqwest` body so the guarantee
    /// is pinned end to end: the server promises `Content-Length: 500`,
    /// sends one complete event plus half of a second one, then hangs up.
    /// `reqwest` surfaces that as a body-decode error and then reports the
    /// body finished, so the single-error-then-end shape below is what a
    /// caller of `Models::generate_content_stream` actually observes.
    #[tokio::test]
    async fn mid_stream_disconnect_yields_exactly_one_stream_error_then_ends() {
        let addr = serve_raw_once(concat!(
            "HTTP/1.1 200 OK\r\n",
            "Content-Type: text/event-stream\r\n",
            "Content-Length: 500\r\n",
            "\r\n",
            "data: {\"a\":1}\n\ndata: {\"a\":2}\n\ndata: {\"a\"",
        ));

        let response = reqwest::get(format!("http://{addr}/"))
            .await
            .expect("the raw server accepts the request");
        let items: Vec<_> = parse_sse(response.bytes_stream()).collect().await;

        // The two complete events arrived before the disconnect...
        assert_eq!(items.len(), 3);
        assert_eq!(
            items[0].as_ref().expect("first event decodes")["a"],
            serde_json::json!(1)
        );
        assert_eq!(
            items[1].as_ref().expect("second event decodes")["a"],
            serde_json::json!(2)
        );
        // ...the truncated third is reported as one transport failure...
        let err = items[2].as_ref().expect_err("the disconnect is an error");
        assert!(
            matches!(err, Error::Stream(_)),
            "a mid-stream disconnect must map to Error::Stream, got {err:?}"
        );
        // ...and `collect()` returning at all is the proof that the stream
        // ended rather than repeating the error indefinitely.
    }

    /// The decoder-level half of the same contract: a transport error
    /// reaching the SSE decoder becomes one `Error::Stream` item carrying
    /// the transport's own message, rather than being swallowed or turned
    /// into a JSON-decode error.
    ///
    /// Scope note: [`parse_sse`] maps errors, it does not fuse the stream --
    /// "one error, then end" comes from the byte stream underneath it
    /// (`reqwest` reports the body finished after a decode failure, which
    /// the sibling test above pins end to end). A hand-written byte stream
    /// that kept producing after an error would keep being decoded, so any
    /// future non-`reqwest` transport needs to be fused before it gets here.
    #[tokio::test]
    async fn a_transport_error_maps_to_one_error_stream_item() {
        let raw = stream::iter(vec![
            Ok(bytes::Bytes::from_static(b"data: {\"a\":1}\n\n")),
            Err(std::io::Error::other("connection reset by peer")),
        ]);

        let items: Vec<_> = parse_sse(raw).collect().await;

        assert_eq!(items.len(), 2);
        assert!(items[0].is_ok());
        let err = items[1]
            .as_ref()
            .expect_err("the transport failure surfaces");
        let Error::Stream(message) = err else {
            panic!("expected Error::Stream, got {err:?}");
        };
        assert!(
            message.contains("connection reset by peer"),
            "the transport's own message should survive: {message}"
        );
    }

    /// Contract (`contracts/wire-protocol.md`): a non-2xx response to a
    /// streaming request has its body read to completion and parsed as the
    /// API's error envelope, so the caller sees the server's message rather
    /// than a generic transport failure.
    ///
    /// Note the shape this takes in Rust: `request_stream` is an `async fn`
    /// returning `Result<impl Stream, _>`, so the failure surfaces when
    /// awaiting the call -- i.e. *before* the stream exists, rather than as
    /// its first item. That is strictly earlier than Python, where the error
    /// is raised on the first `next()` of the generator; either way no
    /// successful chunk is ever observed.
    ///
    /// The message is deliberately far larger than one TCP segment, so a
    /// partially-read body would produce a truncated (or unparseable) error.
    #[tokio::test]
    async fn non_2xx_streaming_response_reads_the_whole_body_into_an_api_error() {
        let message = format!("quota exceeded: {}", "x".repeat(64 * 1024));
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(429).set_body_json(serde_json::json!({
                    "error": {
                        "code": 429,
                        "message": message,
                        "status": "RESOURCE_EXHAUSTED",
                        "details": [{"@type": "type.googleapis.com/google.rpc.RetryInfo"}],
                    }
                })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = crate::client::Client::builder()
            .api_key("test-key")
            .http_options(crate::types::HttpOptions {
                base_url: Some(server.uri()),
                ..Default::default()
            })
            .build()
            .expect("the test client builds");

        let err = client
            .http()
            .request_stream(
                reqwest::Method::POST,
                "models/gemini-2.5-flash:streamGenerateContent",
                Some("alt=sse"),
                Some(serde_json::json!({"contents": []})),
                None,
            )
            .await
            .err()
            .expect("a 429 must not produce a stream");

        let Error::Api(api_err) = err else {
            panic!("expected Error::Api, got {err:?}");
        };
        assert_eq!(api_err.code, 429);
        assert_eq!(api_err.status.as_deref(), Some("RESOURCE_EXHAUSTED"));
        assert_eq!(api_err.message, message);
        assert_eq!(api_err.details.len(), 1);
        server.verify().await;
    }
}
