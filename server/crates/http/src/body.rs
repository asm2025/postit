//! Streaming request bodies, so large media is never held in memory whole.

use std::io;

use bytes::Bytes;
use futures::Stream;

/// Wraps a byte stream as a request body. The stream is polled as the connection takes
/// bytes, so memory stays bounded by the chunk size. The caller sets `Content-Length` when
/// the size is known; without it the body is sent chunked.
pub fn stream_body<S>(stream: S) -> reqwest::Body
where
    S: Stream<Item = io::Result<Bytes>> + Send + 'static,
{
    reqwest::Body::wrap_stream(stream)
}
