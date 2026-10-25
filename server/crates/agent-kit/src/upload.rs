//! The chunked upload driver: re-chunks a byte stream into fixed-size pieces with bounded memory.

use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use postit_core::{AgentError, ByteStream, Message, PublishProgress, Violation};

/// One piece of an upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Where the piece starts in the whole source.
    pub offset: u64,
    /// The piece's bytes: `chunk_size` long, except possibly the last.
    pub bytes: Bytes,
    /// Whether this is the final piece.
    pub last: bool,
}

fn invalid(key: &'static str, en: &'static str) -> AgentError {
    AgentError::Validation(vec![Violation::agent_rule(Message::new_static(key, en))])
}

/// Feeds `stream` to `send` in chunks of `chunk_size`, starting at byte `resume_from` (earlier
/// bytes are read and discarded as they arrive). Returns the total source length.
///
/// Memory is bounded: one chunk waits in look-ahead so the final one can be marked `last`, plus
/// the chunk being filled. `progress.cancel_requested()` is checked before each `send`.
///
/// # Errors
/// `Validation` (`agent_kit.invalid_chunk_size`, `agent_kit.media_empty`,
/// `agent_kit.resume_offset_beyond_end`) before any `send`, `Transport` (its text redacted) when
/// the stream fails, [`AgentError::Cancelled`], or the error `send` returns.
pub async fn upload_chunks<F, Fut>(
    mut stream: ByteStream,
    chunk_size: usize,
    resume_from: u64,
    progress: &dyn PublishProgress,
    mut send: F,
) -> Result<u64, AgentError>
where
    F: FnMut(Chunk) -> Fut,
    Fut: Future<Output = Result<(), AgentError>>,
{
    if chunk_size == 0 {
        return Err(invalid(
            "agent_kit.invalid_chunk_size",
            "The upload chunk size must be above zero.",
        ));
    }
    let mut total: u64 = 0;
    let mut buf = BytesMut::new();
    let mut next_offset = resume_from;
    let mut pending: Option<(u64, Bytes)> = None;

    while let Some(piece) = stream.next().await {
        // The error text may name the source's (signed) URL.
        let mut piece = piece.map_err(|e| AgentError::Transport {
            message: postit_http::redact_secrets(&e.to_string()),
        })?;
        let piece_start = total;
        total += piece.len() as u64;
        if total <= resume_from {
            continue;
        }
        if piece_start < resume_from {
            // The skipped part is shorter than this piece, so it fits in usize.
            let skip = usize::try_from(resume_from - piece_start).unwrap_or(piece.len());
            let _ = piece.split_to(skip);
        }
        buf.extend_from_slice(&piece);
        drop(piece);
        while buf.len() >= chunk_size {
            let bytes = buf.split_to(chunk_size).freeze();
            let offset = next_offset;
            next_offset += chunk_size as u64;
            if let Some((o, b)) = pending.replace((offset, bytes)) {
                emit(&mut send, progress, o, b, false).await?;
            }
        }
    }

    if total == 0 {
        return Err(invalid("agent_kit.media_empty", "The media file is empty."));
    }
    if resume_from >= total {
        return Err(invalid(
            "agent_kit.resume_offset_beyond_end",
            "The upload cannot resume past the end of the file.",
        ));
    }
    if !buf.is_empty() {
        if let Some((o, b)) = pending.take() {
            emit(&mut send, progress, o, b, false).await?;
        }
        pending = Some((next_offset, buf.split().freeze()));
    }
    if let Some((o, b)) = pending {
        emit(&mut send, progress, o, b, true).await?;
    }
    Ok(total)
}

async fn emit<F, Fut>(
    send: &mut F,
    progress: &dyn PublishProgress,
    offset: u64,
    bytes: Bytes,
    last: bool,
) -> Result<(), AgentError>
where
    F: FnMut(Chunk) -> Fut,
    Fut: Future<Output = Result<(), AgentError>>,
{
    if progress.cancel_requested() {
        return Err(AgentError::Cancelled);
    }
    send(Chunk {
        offset,
        bytes,
        last,
    })
    .await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use super::*;

    struct Flag(AtomicBool);

    #[async_trait]
    impl PublishProgress for Flag {
        async fn save_container_ref(&self, _: &str) -> Result<(), AgentError> {
            Ok(())
        }
        fn cancel_requested(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn no_cancel() -> Flag {
        Flag(AtomicBool::new(false))
    }

    fn source(len: usize, piece: usize) -> ByteStream {
        let pieces: Vec<std::io::Result<Bytes>> = (0..len)
            .step_by(piece)
            .map(|s| {
                Ok(Bytes::from(
                    (s..(s + piece).min(len))
                        .map(|i| u8::try_from(i % 256).unwrap_or(0))
                        .collect::<Vec<_>>(),
                ))
            })
            .collect();
        futures::stream::iter(pieces).boxed()
    }

    type Seen = Vec<(u64, usize, bool)>;

    async fn run(stream: ByteStream, size: usize, from: u64) -> (Result<u64, AgentError>, Seen) {
        let seen = Mutex::new(Vec::new());
        let res = upload_chunks(stream, size, from, &no_cancel(), |c| {
            seen.lock().unwrap().push((c.offset, c.bytes.len(), c.last));
            async { Ok(()) }
        })
        .await;
        (res, seen.into_inner().unwrap())
    }

    fn debug(err: &AgentError) -> String {
        format!("{err:?}")
    }

    #[tokio::test]
    async fn upload_exact_chunks() {
        let (res, seen) = run(source(10, 10), 4, 0).await;
        assert_eq!(res.unwrap(), 10);
        assert_eq!(seen, [(0, 4, false), (4, 4, false), (8, 2, true)]);
    }

    #[tokio::test]
    async fn upload_exact_multiple_marks_last() {
        let (res, seen) = run(source(8, 3), 4, 0).await;
        assert_eq!(res.unwrap(), 8);
        assert_eq!(seen, [(0, 4, false), (4, 4, true)]);
    }

    #[tokio::test]
    async fn upload_unaligned_source_pieces() {
        let (res, seen) = run(source(10, 3), 4, 0).await;
        assert_eq!(res.unwrap(), 10);
        assert_eq!(seen, [(0, 4, false), (4, 4, false), (8, 2, true)]);
    }

    #[tokio::test]
    async fn upload_resumes() {
        let (res, seen) = run(source(10, 3), 4, 4).await;
        assert_eq!(res.unwrap(), 10);
        assert_eq!(seen, [(4, 4, false), (8, 2, true)]);
    }

    #[tokio::test]
    async fn upload_resumes_with_correct_bytes() {
        let got = Mutex::new(Vec::new());
        upload_chunks(source(10, 3), 4, 5, &no_cancel(), |c| {
            got.lock().unwrap().extend_from_slice(&c.bytes);
            async { Ok(()) }
        })
        .await
        .unwrap();
        assert_eq!(got.into_inner().unwrap(), [5, 6, 7, 8, 9]);
    }

    #[tokio::test]
    async fn upload_rejects_empty() {
        let (res, seen) = run(futures::stream::empty().boxed(), 4, 0).await;
        let err = res.unwrap_err();
        assert!(matches!(err, AgentError::Validation(_)));
        assert!(debug(&err).contains("agent_kit.media_empty"));
        assert!(seen.is_empty());
    }

    #[tokio::test]
    async fn upload_rejects_zero_chunk_size() {
        let (res, seen) = run(source(10, 3), 0, 0).await;
        assert!(debug(&res.unwrap_err()).contains("agent_kit.invalid_chunk_size"));
        assert!(seen.is_empty());
    }

    #[tokio::test]
    async fn upload_rejects_offset_past_end() {
        for from in [10, 11, 40] {
            let (res, seen) = run(source(10, 3), 4, from).await;
            let err = res.unwrap_err();
            assert!(matches!(err, AgentError::Validation(_)));
            assert!(debug(&err).contains("agent_kit.resume_offset_beyond_end"));
            assert!(seen.is_empty(), "no send before the check, from {from}");
        }
    }

    #[tokio::test]
    async fn upload_stream_error_is_transport() {
        let s = futures::stream::iter(vec![
            Ok(Bytes::from_static(b"abcd")),
            Err(std::io::Error::other("boom")),
        ])
        .boxed();
        let (res, _) = run(s, 4, 0).await;
        assert!(matches!(res.unwrap_err(), AgentError::Transport { .. }));
    }

    #[tokio::test]
    async fn upload_stream_error_redacts_signed_url() {
        let s = futures::stream::iter(vec![Err(std::io::Error::other(
            "reading https://media.example.com/a.png?signature=s3cr3tsig&exp=1 failed",
        ))])
        .boxed();
        let (res, _) = run(s, 4, 0).await;
        let err = res.unwrap_err();
        let AgentError::Transport { message } = &err else {
            panic!("want Transport, got {err:?}");
        };
        assert!(!message.contains("s3cr3tsig"), "{message}");
        assert!(message.contains("media.example.com"), "{message}");
    }

    #[tokio::test]
    async fn upload_cancel_between_chunks() {
        let flag = Flag(AtomicBool::new(false));
        let sends = AtomicU64::new(0);
        let err = upload_chunks(source(10, 3), 4, 0, &flag, |_| {
            sends.fetch_add(1, Ordering::SeqCst);
            flag.0.store(true, Ordering::SeqCst);
            async { Ok(()) }
        })
        .await
        .unwrap_err();
        assert!(matches!(err, AgentError::Cancelled));
        assert_eq!(sends.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn upload_bounded_memory() {
        const MIB: usize = 1024 * 1024;
        let pulled = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&pulled);
        let s = futures::stream::unfold(0usize, move |n| {
            let counter = Arc::clone(&counter);
            async move {
                if n == 64 {
                    return None;
                }
                counter.fetch_add(MIB as u64, Ordering::SeqCst);
                Some((Ok(Bytes::from(vec![0u8; MIB])), n + 1))
            }
        })
        .boxed();
        let sends = AtomicU64::new(0);
        let total = upload_chunks(s, MIB, 0, &no_cancel(), |c| {
            let p = pulled.load(Ordering::SeqCst);
            assert!(
                p <= c.offset + 2 * MIB as u64,
                "pulled {p} at offset {}",
                c.offset
            );
            sends.fetch_add(1, Ordering::SeqCst);
            async { Ok(()) }
        })
        .await
        .unwrap();
        assert_eq!(total, 64 * MIB as u64);
        assert_eq!(sends.load(Ordering::SeqCst), 64);
    }
}
