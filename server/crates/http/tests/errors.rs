#![allow(clippy::unwrap_used, clippy::panic)]

use std::time::Duration;

use chrono::{DateTime, Utc};
use postit_http::{HttpError, retry_after_at, status_error};
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

#[tokio::test]
async fn status_error_redacts_and_truncates_excerpt() {
    let server = MockServer::start().await;
    let body = format!("access_token=SECRET123 {}", "a".repeat(2000));
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(400).set_body_string(body))
        .mount(&server)
        .await;

    let response = reqwest::get(server.uri()).await.unwrap();
    let err = status_error(response, now()).await;
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
    assert!(excerpt.len() <= 512 + 64, "len {}", excerpt.len());
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
