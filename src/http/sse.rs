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
    use futures_util::{StreamExt, stream};

    use super::parse_sse;

    fn chunks(data: &[&str]) -> impl futures_core::Stream<Item = Result<bytes::Bytes, std::io::Error>> {
        let owned: Vec<Result<bytes::Bytes, std::io::Error>> =
            data.iter().map(|s| Ok(bytes::Bytes::from((*s).to_owned()))).collect();
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
}
