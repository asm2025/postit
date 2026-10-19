#![allow(clippy::unwrap_used, clippy::panic)]

use std::time::Duration;

use chrono::{DateTime, Utc};
use postit_config::HttpSettings;
use postit_http::{HttpError, build_client, retry_after_at, status_error};
use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn headers(value: &str) -> HeaderMap {
    let mut map = HeaderMap::new();
    map.insert(RETRY_AFTER, HeaderValue::from_str(value).unwrap());
    map
}

fn now() -> DateTime<Utc> {
    "2015-10-21T07:27:00Z".parse().unwrap()
}

#[test]
fn retry_after_delta_seconds() {
    assert_eq!(
        retry_after_at(&headers("120"), now()),
        Some(Duration::from_secs(120))
    );
}

#[test]
fn retry_after_http_date_relative_to_now() {
    assert_eq!(
        retry_after_at(&headers("Wed, 21 Oct 2015 07:28:00 GMT"), now()),
        Some(Duration::from_secs(60))
    );
}

#[test]
fn retry_after_past_date_is_zero() {
    assert_eq!(
        retry_after_at(&headers("Fri, 01 Jan 1999 00:00:00 GMT"), now()),
        Some(Duration::ZERO)
    );
}

#[test]
fn retry_after_garbage_is_none() {
    for value in ["soon", "-5", ""] {
        assert_eq!(retry_after_at(&headers(value), now()), None, "{value:?}");
    }
}

/// Fetches `server`'s root through a `postit-http` client.
async fn fetch(server: &MockServer) -> reqwest::Response {
    let client = build_client(&HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-http-test/0".to_string(),
        extra_ca_files: Vec::new(),
    })
    .unwrap();
    client.get(server.uri()).send().await.unwrap()
}

/// The status error for a 400 whose body is `body`.
async fn status_error_for(body: String) -> HttpError {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(400).set_body_string(body))
        .mount(&server)
        .await;
    status_error(fetch(&server).await, now()).await
}

fn excerpt_of(err: HttpError) -> String {
    match err {
        HttpError::Status {
            body_excerpt: Some(excerpt),
            ..
        } => excerpt,
        other => panic!("expected Status with an excerpt, got {other:?}"),
    }
}

#[tokio::test]
async fn status_error_redacts_a_json_token_cut_at_the_excerpt_limit() {
    // The value starts 15 bytes before the 512-byte cut, so cutting before redacting leaves
    // an unclosed `"ya29.SECRETPRE`.
    let padding = "x".repeat(480);
    let body =
        format!(r#"{padding} {{"access_token": "ya29.SECRETPREFIX-rest-of-token", "n": 1}}"#);
    let excerpt = excerpt_of(status_error_for(body).await);
    assert!(excerpt.len() <= 512, "len {}", excerpt.len());
    assert!(!excerpt.contains("ya29"), "{excerpt}");
    assert!(!excerpt.contains("SECRETPRE"), "{excerpt}");
}

#[tokio::test]
async fn status_error_excerpt_stays_within_limit_on_a_multibyte_cut() {
    // One ASCII byte then two-byte characters: byte 512 falls inside an `é`.
    let body = format!("a{}", "é".repeat(400));
    let excerpt = excerpt_of(status_error_for(body).await);
    assert!(excerpt.len() <= 512, "len {}", excerpt.len());
    assert!(excerpt.starts_with("aé"), "{excerpt}");
    assert!(!excerpt.contains('\u{fffd}'), "{excerpt}");
}

#[tokio::test]
async fn status_error_keeps_a_huge_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", u64::MAX.to_string().as_str())
                .set_body_string("x".repeat(2000)),
        )
        .mount(&server)
        .await;
    match status_error(fetch(&server).await, now()).await {
        HttpError::Status {
            status: 429,
            retry_after,
            body_excerpt,
        } => {
            assert_eq!(retry_after, Some(Duration::from_secs(u64::MAX)));
            assert_eq!(body_excerpt.map(|e| e.len()), Some(512));
        }
        other => panic!("expected Status 429, got {other:?}"),
    }
}

#[tokio::test]
async fn status_error_redacts_and_truncates_excerpt() {
    let server = MockServer::start().await;
    let body = format!("access_token=SECRET123 {}", "a".repeat(2000));
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(400).set_body_string(body))
        .mount(&server)
        .await;

    let err = status_error(fetch(&server).await, now()).await;
    let HttpError::Status {
        status,
        body_excerpt,
        ..
    } = err
    else {
        panic!("expected Status");
    };
    assert_eq!(status, 400);
    let excerpt = body_excerpt.unwrap();
    assert!(excerpt.len() <= 512, "len {}", excerpt.len());
    assert!(!excerpt.contains("SECRET123"));
}

#[test]
fn status_retryability() {
    let status = |status| HttpError::Status {
        status,
        retry_after: None,
        body_excerpt: None,
    };
    assert!(status(429).is_retryable());
    assert!(status(503).is_retryable());
    assert!(!status(404).is_retryable());
}

#[test]
fn outcome_unknown_and_address_rejected_are_not_retryable() {
    assert!(!HttpError::OutcomeUnknown("x".into()).is_retryable());
    assert!(!HttpError::AddressRejected { host: "h".into() }.is_retryable());
}

#[test]
fn address_rejected_display_has_host_only() {
    let text = HttpError::AddressRejected {
        host: "internal.example".into(),
    }
    .to_string();
    assert!(text.contains("internal.example"));
    assert!(!text.contains('/'));
}
