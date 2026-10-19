#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use postit_config::HttpSettings;
use postit_http::testkit::{fixed_now, seeded_jitter};
use postit_http::{
    HttpError, Idempotency, Now, RateLimitHeaders, ResetHeader, RetryPolicy, RetryRequest,
    build_client, send_with_retry,
};
use reqwest::header::HeaderName;
use reqwest::{Method, Url};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::time::Instant;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn settings(request_timeout: Duration) -> HttpSettings {
    HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout,
        user_agent: "postit-http-test/0".to_string(),
        extra_ca_files: Vec::<PathBuf>::new(),
    }
}

/// A client that arms no tokio timers, for paused-clock tests. Under `start_paused` the
/// runtime auto-advances to the next timer whenever it waits on a socket, so the timeouts
/// `build_client` sets (and hyper's idle-pool reaper) would fire spuriously and inflate the
/// elapsed virtual time. Without them, the only timers are the retry loop's own sleeps.
fn client() -> reqwest::Client {
    #[allow(clippy::disallowed_methods)] // test-only client without timers; see above
    reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .build()
        .unwrap()
}

fn at() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 19, 12, 0, 0).unwrap()
}

fn http_date(time: DateTime<Utc>) -> String {
    time.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

fn request<'a>(
    client: &'a reqwest::Client,
    policy: &'a RetryPolicy,
    idempotency: Idempotency,
    headers: &'a RateLimitHeaders,
    now: Now,
) -> RetryRequest<'a> {
    RetryRequest {
        client,
        policy,
        idempotency,
        headers,
        address_policy: None,
        now,
        limiter: None,
    }
}

fn build(method: Method, url: &str) -> impl FnMut() -> Result<reqwest::Request, HttpError> {
    let url = Url::parse(url).unwrap();
    move || Ok(reqwest::Request::new(method.clone(), url.clone()))
}

async fn mount(server: &MockServer, verb: &str, template: ResponseTemplate, times: u64) {
    Mock::given(method(verb))
        .and(path("/r"))
        .respond_with(template)
        .up_to_n_times(times)
        .mount(server)
        .await;
}

async fn requests(server: &MockServer) -> usize {
    server.received_requests().await.unwrap().len()
}

/// Runs `send_with_retry` against `server` and returns the result plus elapsed virtual time.
async fn run(
    server: &MockServer,
    verb: Method,
    idempotency: Idempotency,
    policy: &RetryPolicy,
    headers: &RateLimitHeaders,
) -> (Result<reqwest::Response, HttpError>, Duration) {
    let client = client();
    let start = Instant::now();
    let result = send_with_retry(
        request(&client, policy, idempotency, headers, fixed_now(at())),
        &mut seeded_jitter(),
        build(verb, &format!("{}/r", server.uri())),
    )
    .await;
    (result, start.elapsed())
}

fn status_of(result: &Result<reqwest::Response, HttpError>) -> Option<u16> {
    result.as_ref().ok().map(|r| r.status().as_u16())
}

#[tokio::test(start_paused = true)]
async fn idempotent_get_retries_503_then_succeeds() {
    let server = MockServer::start().await;
    mount(&server, "GET", ResponseTemplate::new(503), 2).await;
    mount(&server, "GET", ResponseTemplate::new(200), 1).await;

    let (result, _) = run(
        &server,
        Method::GET,
        Idempotency::Idempotent,
        &RetryPolicy::default(),
        &RateLimitHeaders::default(),
    )
    .await;

    assert_eq!(status_of(&result), Some(200));
    assert_eq!(requests(&server).await, 3);
}

async fn three_attempt_elapsed() -> Duration {
    let server = MockServer::start().await;
    mount(&server, "GET", ResponseTemplate::new(503), 2).await;
    mount(&server, "GET", ResponseTemplate::new(200), 1).await;
    let (result, elapsed) = run(
        &server,
        Method::GET,
        Idempotency::Idempotent,
        &RetryPolicy::default(),
        &RateLimitHeaders::default(),
    )
    .await;
    assert_eq!(status_of(&result), Some(200));
    elapsed
}

#[tokio::test(start_paused = true)]
async fn backoff_is_reproducible_for_a_seed() {
    let first = three_attempt_elapsed().await;
    let second = three_attempt_elapsed().await;
    assert_eq!(first, second);
    // Two full-jitter sleeps: at most 500ms + 1s.
    assert!(first <= Duration::from_millis(1500), "{first:?}");
}

#[tokio::test(start_paused = true)]
async fn budget_stops_retries_early() {
    let server = MockServer::start().await;
    mount(&server, "GET", ResponseTemplate::new(503), u64::MAX).await;
    let policy = RetryPolicy {
        max_attempts: 10,
        base_delay: Duration::from_millis(800),
        budget: Duration::from_secs(1),
        ..RetryPolicy::default()
    };

    let (result, elapsed) = run(
        &server,
        Method::GET,
        Idempotency::Idempotent,
        &policy,
        &RateLimitHeaders::default(),
    )
    .await;

    assert!(
        matches!(result, Err(HttpError::Status { status: 503, .. })),
        "{result:?}"
    );
    assert!(requests(&server).await < 10);
    assert!(elapsed <= Duration::from_secs(1), "{elapsed:?}");
}

#[tokio::test(start_paused = true)]
async fn retry_after_delta_is_honored() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        ResponseTemplate::new(429).insert_header("Retry-After", "7"),
        1,
    )
    .await;
    mount(&server, "GET", ResponseTemplate::new(200), 1).await;

    let (result, elapsed) = run(
        &server,
        Method::GET,
        Idempotency::Idempotent,
        &RetryPolicy::default(),
        &RateLimitHeaders::default(),
    )
    .await;

    assert_eq!(status_of(&result), Some(200));
    assert!(elapsed >= Duration::from_secs(7), "{elapsed:?}");
    assert!(elapsed < Duration::from_millis(7700), "{elapsed:?}");
}

#[tokio::test(start_paused = true)]
async fn retry_after_http_date_is_honored() {
    let server = MockServer::start().await;
    let date = http_date(at() + chrono::Duration::seconds(5));
    mount(
        &server,
        "GET",
        ResponseTemplate::new(503).insert_header("Retry-After", date.as_str()),
        1,
    )
    .await;
    mount(&server, "GET", ResponseTemplate::new(200), 1).await;

    let (result, elapsed) = run(
        &server,
        Method::GET,
        Idempotency::Idempotent,
        &RetryPolicy::default(),
        &RateLimitHeaders::default(),
    )
    .await;

    assert_eq!(status_of(&result), Some(200));
    assert!(elapsed >= Duration::from_secs(5), "{elapsed:?}");
    assert!(elapsed < Duration::from_millis(5500), "{elapsed:?}");
}

#[tokio::test(start_paused = true)]
async fn retry_after_over_cap_surfaces_without_sleeping() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        ResponseTemplate::new(429).insert_header("Retry-After", "86400"),
        u64::MAX,
    )
    .await;

    let (result, elapsed) = run(
        &server,
        Method::GET,
        Idempotency::Idempotent,
        &RetryPolicy::default(),
        &RateLimitHeaders::default(),
    )
    .await;

    match result {
        Err(HttpError::RateLimited { retry_after }) => {
            assert_eq!(retry_after, Some(Duration::from_hours(24)));
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
    assert_eq!(elapsed, Duration::ZERO);
    assert_eq!(requests(&server).await, 1);
}

#[tokio::test(start_paused = true)]
async fn huge_retry_after_does_not_overflow() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        ResponseTemplate::new(429).insert_header("Retry-After", u64::MAX.to_string().as_str()),
        u64::MAX,
    )
    .await;

    let (result, elapsed) = run(
        &server,
        Method::GET,
        Idempotency::Idempotent,
        &RetryPolicy::default(),
        &RateLimitHeaders::default(),
    )
    .await;

    assert!(
        matches!(result, Err(HttpError::RateLimited { retry_after: Some(d) }) if d == Duration::from_secs(u64::MAX)),
        "{result:?}"
    );
    assert_eq!(elapsed, Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn garbage_retry_after_uses_backoff() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        ResponseTemplate::new(429).insert_header("Retry-After", "soon"),
        1,
    )
    .await;
    mount(&server, "GET", ResponseTemplate::new(200), 1).await;

    let (result, elapsed) = run(
        &server,
        Method::GET,
        Idempotency::Idempotent,
        &RetryPolicy::default(),
        &RateLimitHeaders::default(),
    )
    .await;

    assert_eq!(status_of(&result), Some(200));
    assert_eq!(requests(&server).await, 2);
    // First backoff is full jitter over base_delay.
    assert!(elapsed <= Duration::from_millis(500), "{elapsed:?}");
}

#[tokio::test(start_paused = true)]
async fn epoch_seconds_reset_header() {
    let server = MockServer::start().await;
    let reset = (at().timestamp() + 3).to_string();
    mount(
        &server,
        "GET",
        ResponseTemplate::new(429).insert_header("x-rate-limit-reset", reset.as_str()),
        1,
    )
    .await;
    mount(&server, "GET", ResponseTemplate::new(200), 1).await;
    let headers = RateLimitHeaders(vec![ResetHeader::EpochSeconds(HeaderName::from_static(
        "x-rate-limit-reset",
    ))]);

    let (result, elapsed) = run(
        &server,
        Method::GET,
        Idempotency::Idempotent,
        &RetryPolicy::default(),
        &headers,
    )
    .await;

    assert_eq!(status_of(&result), Some(200));
    assert!(elapsed >= Duration::from_secs(3), "{elapsed:?}");
    assert!(elapsed < Duration::from_millis(3300), "{elapsed:?}");
}

#[tokio::test(start_paused = true)]
async fn non_idempotent_post_plain_503_is_outcome_unknown() {
    let server = MockServer::start().await;
    mount(&server, "POST", ResponseTemplate::new(503), u64::MAX).await;

    let (result, _) = run(
        &server,
        Method::POST,
        Idempotency::NonIdempotent,
        &RetryPolicy::default(),
        &RateLimitHeaders::default(),
    )
    .await;

    assert!(
        matches!(result, Err(HttpError::OutcomeUnknown(_))),
        "{result:?}"
    );
    assert_eq!(requests(&server).await, 1);
}

#[tokio::test(start_paused = true)]
async fn non_idempotent_post_500_is_outcome_unknown() {
    let server = MockServer::start().await;
    mount(&server, "POST", ResponseTemplate::new(500), u64::MAX).await;

    let (result, _) = run(
        &server,
        Method::POST,
        Idempotency::NonIdempotent,
        &RetryPolicy::default(),
        &RateLimitHeaders::default(),
    )
    .await;

    assert!(
        matches!(result, Err(HttpError::OutcomeUnknown(_))),
        "{result:?}"
    );
    assert_eq!(requests(&server).await, 1);
}

#[tokio::test(start_paused = true)]
async fn non_idempotent_post_429_with_retry_after_is_retried() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        ResponseTemplate::new(429).insert_header("Retry-After", "1"),
        1,
    )
    .await;
    mount(&server, "POST", ResponseTemplate::new(200), 1).await;

    let (result, _) = run(
        &server,
        Method::POST,
        Idempotency::NonIdempotent,
        &RetryPolicy::default(),
        &RateLimitHeaders::default(),
    )
    .await;

    assert_eq!(status_of(&result), Some(200));
    assert_eq!(requests(&server).await, 2);
}

#[tokio::test(start_paused = true)]
async fn non_idempotent_post_connect_refused_is_retried() {
    let server = MockServer::start().await;
    mount(&server, "POST", ResponseTemplate::new(200), 1).await;
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_url = Url::parse(&format!("http://{}/r", dead.local_addr().unwrap())).unwrap();
    drop(dead);
    let live_url = Url::parse(&format!("{}/r", server.uri())).unwrap();

    let client = client();
    let policy = RetryPolicy::default();
    let headers = RateLimitHeaders::default();
    let mut calls = 0_u32;
    let result = send_with_retry(
        request(
            &client,
            &policy,
            Idempotency::NonIdempotent,
            &headers,
            fixed_now(at()),
        ),
        &mut seeded_jitter(),
        || {
            calls += 1;
            let url = if calls == 1 { &dead_url } else { &live_url };
            Ok(reqwest::Request::new(Method::POST, url.clone()))
        },
    )
    .await;

    assert_eq!(status_of(&result), Some(200));
    assert_eq!(calls, 2);
    assert_eq!(requests(&server).await, 1);
}

// Paused like the others: the listener runs on this runtime and `client()` arms no timers,
// so the reset is what ends the request, never an auto-advanced timeout.
#[tokio::test(start_paused = true)]
async fn non_idempotent_post_reset_after_body_is_outcome_unknown() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/r", listener.local_addr().unwrap());
    let connections = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&connections);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            seen.fetch_add(1, Ordering::SeqCst);
            let mut buffer = vec![0_u8; 4096];
            let mut read = Vec::new();
            // Read the full request (headers and the small body), then drop the socket.
            while let Ok(n) = socket.read(&mut buffer).await {
                if n == 0 {
                    break;
                }
                read.extend_from_slice(&buffer[..n]);
                if read.ends_with(b"hello") {
                    break;
                }
            }
            drop(socket);
        }
    });

    let client = client();
    let policy = RetryPolicy::default();
    let headers = RateLimitHeaders::default();
    let target = Url::parse(&url).unwrap();
    let result = send_with_retry(
        request(
            &client,
            &policy,
            Idempotency::NonIdempotent,
            &headers,
            fixed_now(at()),
        ),
        &mut seeded_jitter(),
        || {
            let mut req = reqwest::Request::new(Method::POST, target.clone());
            *req.body_mut() = Some(reqwest::Body::from("hello"));
            Ok(req)
        },
    )
    .await;

    match result {
        Err(HttpError::OutcomeUnknown(text)) => {
            assert!(!text.contains("timed out"), "should be a reset: {text}");
        }
        other => panic!("expected OutcomeUnknown, got {other:?}"),
    }
    assert_eq!(connections.load(Ordering::SeqCst), 1);
}

// Real time, not paused: this case needs reqwest's request timer, and with a paused clock the
// runtime auto-advances to that timer as soon as it waits on the socket, so the timeout would
// fire without the server ever being slow. A 200ms real timeout keeps the test fast.
#[tokio::test]
async fn non_idempotent_post_timeout_is_outcome_unknown() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        ResponseTemplate::new(200).set_delay(Duration::from_secs(2)),
        u64::MAX,
    )
    .await;

    let client = build_client(&settings(Duration::from_millis(200))).unwrap();
    let policy = RetryPolicy::default();
    let headers = RateLimitHeaders::default();
    let result = send_with_retry(
        request(
            &client,
            &policy,
            Idempotency::NonIdempotent,
            &headers,
            fixed_now(at()),
        ),
        &mut seeded_jitter(),
        build(Method::POST, &format!("{}/r", server.uri())),
    )
    .await;

    assert!(
        matches!(result, Err(HttpError::OutcomeUnknown(_))),
        "{result:?}"
    );
    assert_eq!(requests(&server).await, 1);
}

#[tokio::test(start_paused = true)]
async fn make_is_called_once_per_attempt() {
    let server = MockServer::start().await;
    mount(&server, "GET", ResponseTemplate::new(503), 2).await;
    mount(&server, "GET", ResponseTemplate::new(200), 1).await;
    let url = Url::parse(&format!("{}/r", server.uri())).unwrap();

    let client = client();
    let policy = RetryPolicy::default();
    let headers = RateLimitHeaders::default();
    let mut calls = 0_usize;
    let result = send_with_retry(
        request(
            &client,
            &policy,
            Idempotency::Idempotent,
            &headers,
            fixed_now(at()),
        ),
        &mut seeded_jitter(),
        || {
            calls += 1;
            Ok(reqwest::Request::new(Method::GET, url.clone()))
        },
    )
    .await;

    assert_eq!(status_of(&result), Some(200));
    assert_eq!(calls, requests(&server).await);
    assert_eq!(calls, 3);
}

#[tokio::test(start_paused = true)]
async fn closure_error_on_rebuild_stops() {
    let server = MockServer::start().await;
    mount(&server, "GET", ResponseTemplate::new(503), u64::MAX).await;
    let url = Url::parse(&format!("{}/r", server.uri())).unwrap();

    let client = client();
    let policy = RetryPolicy::default();
    let headers = RateLimitHeaders::default();
    let mut calls = 0_usize;
    let result = send_with_retry(
        request(
            &client,
            &policy,
            Idempotency::Idempotent,
            &headers,
            fixed_now(at()),
        ),
        &mut seeded_jitter(),
        || {
            calls += 1;
            if calls == 1 {
                Ok(reqwest::Request::new(Method::GET, url.clone()))
            } else {
                Err(HttpError::Permanent(
                    "body stream already consumed".to_string(),
                ))
            }
        },
    )
    .await;

    match result {
        Err(HttpError::Permanent(text)) => assert_eq!(text, "body stream already consumed"),
        other => panic!("expected the closure error, got {other:?}"),
    }
    assert_eq!(calls, 2);
    assert_eq!(requests(&server).await, 1);
}

#[tokio::test(start_paused = true)]
async fn idempotent_get_reset_after_send_is_retried() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = Url::parse(&format!("http://{}/r", listener.local_addr().unwrap())).unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&connections);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let n = seen.fetch_add(1, Ordering::SeqCst) + 1;
            let mut buffer = vec![0_u8; 4096];
            let mut read = Vec::new();
            while let Ok(count) = socket.read(&mut buffer).await {
                if count == 0 {
                    break;
                }
                read.extend_from_slice(&buffer[..count]);
                if read.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            // The first connection drops after reading the request; the second answers.
            if n >= 2 {
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                    .await;
            }
            drop(socket);
        }
    });

    let client = client();
    let policy = RetryPolicy::default();
    let headers = RateLimitHeaders::default();
    let result = send_with_retry(
        request(
            &client,
            &policy,
            Idempotency::Idempotent,
            &headers,
            fixed_now(at()),
        ),
        &mut seeded_jitter(),
        build(Method::GET, url.as_str()),
    )
    .await;

    assert_eq!(status_of(&result), Some(200), "{result:?}");
    assert_eq!(connections.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn non_idempotent_post_429_without_reset_is_status() {
    let server = MockServer::start().await;
    mount(&server, "POST", ResponseTemplate::new(429), u64::MAX).await;

    let (result, elapsed) = run(
        &server,
        Method::POST,
        Idempotency::NonIdempotent,
        &RetryPolicy::default(),
        &RateLimitHeaders::default(),
    )
    .await;

    assert!(
        matches!(result, Err(HttpError::Status { status: 429, .. })),
        "{result:?}"
    );
    assert_eq!(requests(&server).await, 1);
    assert_eq!(elapsed, Duration::ZERO);
}
