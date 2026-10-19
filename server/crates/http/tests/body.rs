#![allow(clippy::unwrap_used, clippy::panic)]

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use futures::StreamExt as _;
use futures::stream;
use postit_http::stream_body;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

const CHUNK: usize = 64 * 1024;
const CHUNKS: u64 = 1024; // 64 MiB
const TOTAL: u64 = CHUNK as u64 * CHUNKS;

#[tokio::test]
async fn streamed_body_is_not_buffered() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let read = Arc::new(AtomicU64::new(0));

    let reader_count = Arc::clone(&read);
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0_u8; CHUNK];
        let mut head = Vec::new();
        // Consume the request head, then discard the body until the declared length is in.
        let body_start = loop {
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0, "closed before the head ended");
            head.extend_from_slice(&buf[..n]);
            if let Some(pos) = head.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
        };
        let mut got = (head.len() - body_start) as u64;
        reader_count.store(got, Ordering::SeqCst);
        while got < TOTAL {
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0, "closed early at {got}");
            got += n as u64;
            reader_count.store(got, Ordering::SeqCst);
        }
        socket
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
            .await
            .unwrap();
    });

    // High-water mark of chunks produced but not yet read by the listener. It includes what the
    // OS socket buffers hold on loopback (about 21 chunks, 1.3 MiB, on Windows), so the bound
    // is 64 chunks (4 MiB): buffering the 64 MiB body would reach 1024.
    let produced = Arc::new(AtomicU64::new(0));
    let high = Arc::new(AtomicU64::new(0));
    let (p, h, r) = (Arc::clone(&produced), Arc::clone(&high), Arc::clone(&read));
    let chunks = stream::iter(0..CHUNKS).map(move |_| {
        let made = p.fetch_add(1, Ordering::SeqCst) + 1;
        let consumed = r.load(Ordering::SeqCst) / CHUNK as u64;
        h.fetch_max(made.saturating_sub(consumed), Ordering::SeqCst);
        Ok::<_, io::Error>(Bytes::from(vec![7_u8; CHUNK]))
    });

    #[allow(clippy::disallowed_methods)] // test client against a local listener
    let client = reqwest::Client::builder().build().unwrap();
    let response = client
        .post(format!("http://{addr}/upload"))
        .header("content-length", TOTAL)
        .body(stream_body(chunks))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status().as_u16(), 200);
    server.await.unwrap();
    assert_eq!(read.load(Ordering::SeqCst), TOTAL);
    assert_eq!(produced.load(Ordering::SeqCst), CHUNKS);
    let high = high.load(Ordering::SeqCst);
    assert!(high <= 64, "high-water mark {high} chunks");
}
