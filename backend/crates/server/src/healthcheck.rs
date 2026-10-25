use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use anyhow::{Context, bail};

/// `GET /health` over plain HTTP/1.1. Raw TCP on purpose: `postit-http` stays the only
/// `reqwest` factory, and a healthcheck needs nothing more.
///
/// # Errors
///
/// Fails when the connection fails or the status is not 200.
pub fn probe(addr: SocketAddr) -> anyhow::Result<()> {
    let timeout = Duration::from_secs(5);
    let mut stream = TcpStream::connect_timeout(&addr, timeout)
        .with_context(|| format!("connecting to {addr}"))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut head = [0_u8; 32];
    let read = stream.read(&mut head)?;
    let line = String::from_utf8_lossy(&head[..read]);
    if line.starts_with("HTTP/1.1 200") || line.starts_with("HTTP/1.0 200") {
        return Ok(());
    }
    bail!("unhealthy: {}", line.lines().next().unwrap_or_default())
}
