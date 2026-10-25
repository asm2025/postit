//! The streaming writer: counts bytes, enforces a limit, and optionally hashes and captures the
//! head the sniffer needs. Nothing here holds more than one chunk.

use futures::StreamExt;
use postit_core::ByteStream;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::sniff::SNIFF_LEN;

/// Why copying a stream stopped.
#[derive(Debug)]
pub(crate) enum IntakeError {
    /// The stream carried more bytes than the limit allows.
    TooLong,
    /// The incoming stream failed, for example a dropped connection.
    Stream,
    /// Writing to storage failed.
    Io(std::io::Error),
}

/// What a copy learned.
#[derive(Debug)]
pub(crate) struct Written {
    /// Bytes written.
    pub bytes: u64,
    /// SHA-256 of the bytes, when hashing was asked for.
    pub sha256: Option<Vec<u8>>,
    /// The first [`SNIFF_LEN`] bytes.
    pub head: Vec<u8>,
}

/// Copies `body` into `out`, refusing to write past `limit` bytes. The limit is checked before
/// each chunk reaches the file.
pub(crate) async fn copy_stream<W: AsyncWrite + Unpin>(
    out: &mut W,
    mut body: ByteStream,
    limit: u64,
    hash: bool,
) -> Result<Written, IntakeError> {
    let mut hasher = hash.then(Sha256::new);
    let mut head = Vec::with_capacity(SNIFF_LEN);
    let mut bytes: u64 = 0;
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|_| IntakeError::Stream)?;
        bytes = bytes.saturating_add(chunk.len() as u64);
        if bytes > limit {
            return Err(IntakeError::TooLong);
        }
        if head.len() < SNIFF_LEN {
            let take = (SNIFF_LEN - head.len()).min(chunk.len());
            head.extend_from_slice(&chunk[..take]);
        }
        if let Some(h) = hasher.as_mut() {
            h.update(&chunk);
        }
        out.write_all(&chunk).await.map_err(IntakeError::Io)?;
    }
    out.flush().await.map_err(IntakeError::Io)?;
    Ok(Written {
        bytes,
        sha256: hasher.map(|h| h.finalize().to_vec()),
        head,
    })
}
