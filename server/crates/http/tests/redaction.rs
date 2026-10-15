//! Own test binary: the log capture installs a thread-default subscriber, and tracing's
//! callsite-interest cache is process-wide, so it must not share a process with other tests.

use std::time::Duration;

use postit_config::HttpSettings;
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use postit_http::{build_client, execute_traced};

#[tokio::test]
async fn failed_request_never_leaks_query_secrets_into_the_error_or_logs() {
    let (logs, _guard) = LogCapture::install();

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

    let logs = logs.text();
    assert!(logs.contains("http request failed"), "{logs}");
    assert_no_leak(
        &["tok-secret-1", "code-secret-2"],
        &[
            ("error", &err.to_string()),
            ("debug", &format!("{err:?}")),
            ("logs", &logs),
        ],
    );
}
