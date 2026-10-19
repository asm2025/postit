//! Leak coverage for spans, logs, and errors on the retry and `OutcomeUnknown` paths.
//!
//! Own test binary: the log capture installs a thread-default subscriber, and tracing's
//! callsite-interest cache is process-wide. It is also the one binary that installs the
//! process-wide redactor (defaults plus a test webhook route on the wiremock host), since
//! `send_with_retry` always reads the global one.

use std::path::PathBuf;
use std::sync::Once;
use std::time::Duration;

use chrono::{TimeZone as _, Utc};
use postit_config::HttpSettings;
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use postit_http::testkit::{fixed_now, seeded_jitter};
use postit_http::{
    HttpError, Idempotency, RateLimitHeaders, Redactor, RetryPolicy, RetryRequest, SensitiveUrl,
    Sensitivity, build_client, install_redactor, send_with_retry,
};
use reqwest::header::AUTHORIZATION;
use reqwest::{Method, Url};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BEARER: &str = "BEARER-FIXTURE-7a1";
const CODE: &str = "CODE-FIXTURE-8b2";
const STATE: &str = "STATE-FIXTURE-9c3";
const WEBHOOK: &str = "WEBHOOK-FIXTURE-0d4";
const PUSH: &str = "ENDPOINTSECRET";
const REFRESH: &str = "REFRESH-FIXTURE-1e5";

static INSTALL: Once = Once::new();

/// Installs the global redactor once for this binary: the defaults plus `127.0.0.1/hooks/`,
/// standing in for a webhook host on the wiremock server.
fn install() {
    INSTALL.call_once(|| {
        let mut redactor = Redactor::with_defaults();
        redactor.add(SensitiveUrl {
            host: "127.0.0.1".to_string(),
            host_suffix: false,
            path_prefix: Some("/hooks/".to_string()),
        });
        assert!(install_redactor(redactor), "redactor already installed");
    });
}

fn client(request_timeout: Duration) -> reqwest::Client {
    build_client(&HttpSettings {
        connect_timeout: Duration::from_secs(2),
        request_timeout,
        user_agent: "postit-http-test/0".to_string(),
        extra_ca_files: Vec::<PathBuf>::new(),
    })
    .unwrap_or_else(|err| unreachable!("building test client: {err}"))
}

fn fast_policy(max_attempts: u32) -> RetryPolicy {
    RetryPolicy {
        max_attempts,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
        ..RetryPolicy::default()
    }
}

async fn send(
    client: &reqwest::Client,
    policy: &RetryPolicy,
    idempotency: Idempotency,
    sensitivity: Sensitivity,
    verb: Method,
    url: &str,
) -> Result<reqwest::Response, HttpError> {
    let headers = RateLimitHeaders::default();
    let url = Url::parse(url).unwrap_or_else(|err| unreachable!("test url: {err}"));
    send_with_retry(
        RetryRequest {
            client,
            policy,
            idempotency,
            headers: &headers,
            address_policy: None,
            now: fixed_now(
                Utc.with_ymd_and_hms(2026, 10, 19, 12, 0, 0)
                    .single()
                    .unwrap_or_else(|| unreachable!("fixed date")),
            ),
            limiter: None,
            sensitivity,
        },
        &mut seeded_jitter(),
        || {
            let mut request = reqwest::Request::new(verb.clone(), url.clone());
            request.headers_mut().insert(
                AUTHORIZATION,
                format!("Bearer {BEARER}")
                    .parse()
                    .unwrap_or_else(|err| unreachable!("header: {err}")),
            );
            Ok(request)
        },
    )
    .await
}

fn closed_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map_or_else(|err| unreachable!("reserving a port: {err}"), |a| a.port())
}

#[tokio::test]
async fn opaque_request_span_has_no_url() {
    install();
    let (logs, _guard) = LogCapture::install();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/push/{PUSH}")))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;

    let response = send(
        &client(Duration::from_secs(5)),
        &fast_policy(1),
        Idempotency::NonIdempotent,
        Sensitivity::Opaque,
        Method::POST,
        &format!("{}/push/{PUSH}", server.uri()),
    )
    .await
    .unwrap_or_else(|err| unreachable!("push send: {err}"));
    assert_eq!(response.status().as_u16(), 201);

    let logs = logs.text();
    assert!(logs.contains("[redacted-url]"), "{logs}");
    assert_no_leak(&[PUSH, BEARER], &[("logs", &logs)]);
}

#[tokio::test]
async fn opaque_transport_error_has_no_url() {
    install();
    let (logs, _guard) = LogCapture::install();
    let port = closed_port();

    let Err(err) = send(
        &client(Duration::from_secs(2)),
        &fast_policy(2),
        Idempotency::NonIdempotent,
        Sensitivity::Opaque,
        Method::POST,
        &format!("http://127.0.0.1:{port}/push/{PUSH}"),
    )
    .await
    else {
        unreachable!("a closed port unexpectedly answered");
    };

    let logs = logs.text();
    assert!(logs.contains("http request failed"), "{logs}");
    assert!(logs.contains("[redacted-url]"), "{logs}");
    assert_no_leak(
        &[PUSH, BEARER],
        &[
            ("error", &err.to_string()),
            ("debug", &format!("{err:?}")),
            ("logs", &logs),
        ],
    );
}

#[tokio::test]
async fn retried_request_leaks_nothing() {
    install();
    let (logs, _guard) = LogCapture::install();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/hooks/{WEBHOOK}")))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/hooks/{WEBHOOK}")))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/cb"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let client = client(Duration::from_secs(5));

    // A webhook route, an OAuth code, and a bearer header: 503 then 200.
    let response = send(
        &client,
        &fast_policy(3),
        Idempotency::Idempotent,
        Sensitivity::Normal,
        Method::GET,
        &format!("{}/hooks/{WEBHOOK}?code={CODE}", server.uri()),
    )
    .await
    .unwrap_or_else(|err| unreachable!("webhook send: {err}"));
    assert_eq!(response.status().as_u16(), 200);

    // A callback whose retries run out: the error must be clean too.
    let Err(err) = send(
        &client,
        &fast_policy(2),
        Idempotency::Idempotent,
        Sensitivity::Normal,
        Method::GET,
        &format!("{}/cb?code={CODE}&state={STATE}&page=2", server.uri()),
    )
    .await
    else {
        unreachable!("an always-503 route unexpectedly succeeded");
    };

    let logs = logs.text();
    assert!(logs.contains("retrying http request"), "{logs}");
    assert!(logs.contains("/[redacted]"), "{logs}");
    assert!(logs.contains("page=2"), "{logs}");
    assert_no_leak(
        &[BEARER, CODE, STATE, WEBHOOK],
        &[
            ("error", &err.to_string()),
            ("debug", &format!("{err:?}")),
            ("logs", &logs),
        ],
    );
}

#[tokio::test]
async fn outcome_unknown_leaks_nothing() {
    install();
    let (logs, _guard) = LogCapture::install();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/hooks/{WEBHOOK}")))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;

    // A non-idempotent POST that times out after it was sent: `OutcomeUnknown`.
    let Err(err) = send(
        &client(Duration::from_millis(300)),
        &fast_policy(3),
        Idempotency::NonIdempotent,
        Sensitivity::Normal,
        Method::POST,
        &format!("{}/hooks/{WEBHOOK}?code={CODE}", server.uri()),
    )
    .await
    else {
        unreachable!("a delayed route answered inside the timeout");
    };
    assert!(matches!(err, HttpError::OutcomeUnknown(_)), "{err:?}");

    let logs = logs.text();
    assert_no_leak(
        &[BEARER, CODE, WEBHOOK],
        &[
            ("error", &err.to_string()),
            ("debug", &format!("{err:?}")),
            ("logs", &logs),
        ],
    );
}

#[tokio::test]
async fn status_excerpt_leaks_nothing() {
    install();
    let (logs, _guard) = LogCapture::install();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_string(format!(
            "invalid grant refresh_token={REFRESH}; retry at https://h.example/cb?state={STATE} or https://hooks.slack.com/services/T/B/{WEBHOOK}"
        )))
        .mount(&server)
        .await;

    let Err(err) = send(
        &client(Duration::from_secs(5)),
        &fast_policy(1),
        Idempotency::Idempotent,
        Sensitivity::Normal,
        Method::GET,
        &format!("{}/token", server.uri()),
    )
    .await
    else {
        unreachable!("a 400 route unexpectedly succeeded");
    };
    let HttpError::Status {
        status: 400,
        body_excerpt: Some(excerpt),
        ..
    } = &err
    else {
        unreachable!("expected a 400 with a body: {err:?}");
    };
    assert!(excerpt.contains("invalid grant"), "{excerpt}");

    assert_no_leak(
        &[REFRESH, STATE, WEBHOOK],
        &[
            ("excerpt", excerpt),
            ("debug", &format!("{err:?}")),
            ("logs", &logs.text()),
        ],
    );
}
