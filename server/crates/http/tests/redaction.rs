//! Own test binary: the log capture installs a thread-default subscriber, and tracing's
//! callsite-interest cache is process-wide, so it must not share a process with other tests.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use postit_config::HttpSettings;
use postit_http::{build_client, execute_traced};
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl MakeWriter<'_> for Captured {
    type Writer = Self;
    fn make_writer(&self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test]
async fn failed_request_never_leaks_query_secrets_into_the_error_or_logs() {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(captured.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    // Bind then drop a listener so the port is closed and the connection is refused.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map_or_else(|err| unreachable!("reserving a port: {err}"), |a| a.port());

    let client = build_client(&HttpSettings {
        connect_timeout: Duration::from_secs(2),
        request_timeout: Duration::from_secs(2),
        user_agent: "postit-http-test/0".to_string(),
        extra_ca_files: Vec::new(),
    })
    .unwrap_or_else(|err| unreachable!("building test client: {err}"));
    let request = client
        .get(format!(
            "http://127.0.0.1:{port}/cb?token=tok-secret-1&code=code-secret-2&page=3"
        ))
        .build()
        .unwrap_or_else(|err| unreachable!("building test request: {err}"));

    let Err(err) = execute_traced(&client, request).await else {
        unreachable!("a closed port unexpectedly answered");
    };
    assert!(err.is_retryable(), "connection refused is transient: {err}");

    let logs = String::from_utf8_lossy(
        &captured
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
    .into_owned();
    assert!(logs.contains("http request failed"), "{logs}");
    for text in [format!("{err}"), format!("{err:?}"), logs] {
        assert!(!text.contains("tok-secret-1"), "{text}");
        assert!(!text.contains("code-secret-2"), "{text}");
    }
}
