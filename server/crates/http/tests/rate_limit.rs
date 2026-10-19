#![allow(clippy::unwrap_used, clippy::panic)]

use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use chrono::{TimeZone as _, Utc};
use governor::Quota;
use governor::clock::FakeRelativeClock;
use postit_http::testkit::{fixed_now, seeded_jitter};
use postit_http::{
    BucketSpec, HttpError, Idempotency, RateLimitHeaders, RateLimiters, RetryPolicy, RetryRequest,
    send_with_retry,
};
use reqwest::{Method, Url};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn per_minute(n: u32) -> Quota {
    Quota::per_minute(NonZeroU32::new(n).unwrap())
}

fn spec(name: &str, quota: Quota) -> BucketSpec {
    BucketSpec {
        name: name.to_string(),
        quota,
    }
}

fn fake(specs: Vec<BucketSpec>) -> (RateLimiters<FakeRelativeClock>, FakeRelativeClock) {
    let clock = FakeRelativeClock::default();
    (RateLimiters::with_clock(specs, clock.clone()), clock)
}

#[test]
fn bucket_admits_quota_then_rejects() {
    let (limiters, clock) = fake(vec![spec("post", per_minute(3))]);
    for _ in 0..3 {
        assert!(limiters.try_acquire("post", "acct").unwrap());
    }
    assert!(!limiters.try_acquire("post", "acct").unwrap());
    clock.advance(Duration::from_secs(20));
    assert!(limiters.try_acquire("post", "acct").unwrap());
}

#[test]
fn keys_do_not_share_capacity() {
    let (limiters, _) = fake(vec![spec("post", per_minute(1))]);
    assert!(limiters.try_acquire("post", "acct-a").unwrap());
    assert!(!limiters.try_acquire("post", "acct-a").unwrap());
    assert!(limiters.try_acquire("post", "acct-b").unwrap());
}

#[test]
fn buckets_do_not_share_capacity() {
    let (limiters, _) = fake(vec![
        spec("post", per_minute(1)),
        spec("read", per_minute(1)),
    ]);
    assert!(limiters.try_acquire("post", "acct").unwrap());
    assert!(!limiters.try_acquire("post", "acct").unwrap());
    assert!(limiters.try_acquire("read", "acct").unwrap());
}

#[test]
fn unknown_bucket_is_an_error() {
    let (limiters, _) = fake(vec![]);
    let err = limiters.try_acquire("nope", "acct").unwrap_err();
    assert!(
        matches!(&err, HttpError::Permanent(m) if m == "unknown rate-limit bucket nope"),
        "{err:?}"
    );
}

#[tokio::test]
async fn acquire_waits_instead_of_failing() {
    // One permit every 50 ms with a burst of one.
    let quota = Quota::with_period(Duration::from_millis(50))
        .unwrap()
        .allow_burst(NonZeroU32::new(1).unwrap());
    let limiters = RateLimiters::new(vec![spec("post", quota)]);
    let start = Instant::now();
    limiters.acquire("post", "acct").await.unwrap();
    limiters.acquire("post", "acct").await.unwrap();
    assert!(
        start.elapsed() >= Duration::from_millis(45),
        "{:?}",
        start.elapsed()
    );
    assert!(matches!(
        limiters.acquire("nope", "acct").await,
        Err(HttpError::Permanent(_))
    ));
}

/// The untimed test client from `tests/retry.rs`; see the note there.
fn client() -> reqwest::Client {
    #[allow(clippy::disallowed_methods)] // test-only client without timers
    reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .build()
        .unwrap()
}

#[tokio::test(start_paused = true)]
async fn send_with_retry_acquires_before_each_attempt() {
    let server = MockServer::start().await;
    for status in [503, 200] {
        Mock::given(method("GET"))
            .and(path("/r"))
            .respond_with(ResponseTemplate::new(status))
            .up_to_n_times(1)
            .mount(&server)
            .await;
    }
    // A burst of 3 that never refills during the test: the leftover capacity shows how many
    // permits the call took.
    let limiters = RateLimiters::new(vec![spec(
        "get",
        Quota::per_hour(NonZeroU32::new(3).unwrap()),
    )]);
    let client = client();
    let policy = RetryPolicy::default();
    let headers = RateLimitHeaders::default();
    let now = fixed_now(Utc.with_ymd_and_hms(2026, 10, 19, 12, 0, 0).unwrap());
    let url = Url::parse(&format!("{}/r", server.uri())).unwrap();

    let response = send_with_retry(
        RetryRequest {
            client: &client,
            policy: &policy,
            idempotency: Idempotency::Idempotent,
            headers: &headers,
            address_policy: None,
            now,
            limiter: Some((&limiters, "get", "acct")),
        },
        &mut seeded_jitter(),
        || Ok(reqwest::Request::new(Method::GET, url.clone())),
    )
    .await
    .unwrap();

    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    // Two attempts took two of three permits: exactly one is left.
    assert!(limiters.try_acquire("get", "acct").unwrap());
    assert!(!limiters.try_acquire("get", "acct").unwrap());
}
