#![allow(clippy::unwrap_used, clippy::panic)]

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use postit_config::HttpSettings;
use postit_http::testkit::{StubLookup, fixed_now, seeded_jitter};
use postit_http::{
    AddressPolicy, HttpError, Idempotency, PublicClient, RateLimitHeaders, RetryPolicy,
    RetryRequest, Sensitivity, build_public_client, execute_public, is_allowed, resolve_checked,
    send_with_retry,
};
use reqwest::{Method, Url};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PRIVATE: AddressPolicy = AddressPolicy {
    allow_private: true,
};

fn settings() -> HttpSettings {
    HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-http-test/0".to_string(),
        extra_ca_files: Vec::<PathBuf>::new(),
    }
}

fn get(url: &str) -> reqwest::Request {
    reqwest::Request::new(Method::GET, Url::parse(url).unwrap())
}

/// `send_with_retry` through `client` with one attempt, for a GET of `url`.
async fn send_retrying(client: &PublicClient, url: &str) -> Result<reqwest::Response, HttpError> {
    let policy = RetryPolicy {
        max_attempts: 1,
        ..RetryPolicy::default()
    };
    let headers = RateLimitHeaders::default();
    send_with_retry(
        RetryRequest {
            client: client.into(),
            policy: &policy,
            idempotency: Idempotency::Idempotent,
            headers: &headers,
            now: fixed_now(Utc::now()),
            limiter: None,
            sensitivity: Sensitivity::Normal,
        },
        &mut seeded_jitter(),
        || Ok(get(url)),
    )
    .await
}

async fn ok_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    server
}

fn addr(text: &str) -> SocketAddr {
    text.parse().unwrap()
}

fn assert_rejected(result: Result<impl std::fmt::Debug, HttpError>) {
    match result {
        Err(HttpError::AddressRejected { .. }) => {}
        other => panic!("expected AddressRejected, got {other:?}"),
    }
}

#[test]
fn forbidden_ranges_table() {
    let table: &[(&str, bool, bool)] = &[
        ("127.0.0.1", false, true),
        ("10.1.2.3", false, true),
        ("172.16.0.1", false, true),
        ("192.168.1.1", false, true),
        ("100.64.0.1", false, true),
        ("169.254.169.254", false, true),
        ("0.0.0.0", false, false),
        ("224.0.0.1", false, false),
        ("255.255.255.255", false, false),
        ("::1", false, true),
        ("fc00::1", false, true),
        ("fe80::1", false, true),
        ("ff02::1", false, false),
        ("::", false, false),
        ("::ffff:127.0.0.1", false, true),
        ("64:ff9b::7f00:1", false, true),
        ("192.0.0.1", false, false),
        ("198.18.0.1", false, false),
        ("240.0.0.1", false, false),
        ("::7f00:1", false, true),
        ("::ffff:8.8.8.8", true, true),
        ("64:ff9b::808:808", true, true),
        ("8.8.8.8", true, true),
        ("2606:4700::1111", true, true),
    ];
    for &(text, strict_allowed, private_allowed) in table {
        let ip: IpAddr = text.parse().unwrap();
        assert_eq!(
            is_allowed(ip, AddressPolicy::strict()),
            strict_allowed,
            "{text} under strict"
        );
        assert_eq!(
            is_allowed(ip, PRIVATE),
            private_allowed,
            "{text} with allow_private"
        );
    }
}

#[tokio::test]
async fn stub_lookup_mixed_keeps_only_public() {
    let stub = StubLookup::new(vec![addr("10.0.0.1:443"), addr("93.184.216.34:443")]);
    let kept = stub
        .resolve_checked("example.test", 443, AddressPolicy::strict())
        .await
        .unwrap();
    assert_eq!(kept, vec![addr("93.184.216.34:443")]);
}

#[tokio::test]
async fn stub_lookup_only_private_is_rejected() {
    let stub = StubLookup::new(vec![addr("127.0.0.1:443")]);
    match stub
        .resolve_checked("internal.test", 443, AddressPolicy::strict())
        .await
    {
        Err(HttpError::AddressRejected { host }) => assert_eq!(host, "internal.test"),
        other => panic!("expected AddressRejected, got {other:?}"),
    }
}

#[tokio::test]
async fn private_allowed_keeps_lan() {
    let stub = StubLookup::new(vec![addr("192.168.1.5:25")]);
    let kept = stub.resolve_checked("mail.lan", 25, PRIVATE).await.unwrap();
    assert_eq!(kept, vec![addr("192.168.1.5:25")]);
}

#[tokio::test]
async fn resolve_checked_ip_literal() {
    assert_rejected(resolve_checked("127.0.0.1", 25, AddressPolicy::strict()).await);
    let kept = resolve_checked("127.0.0.1", 25, PRIVATE).await.unwrap();
    assert_eq!(kept, vec![addr("127.0.0.1:25")]);
}

#[tokio::test]
async fn public_client_refuses_loopback_literal_without_sending() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let client = build_public_client(&settings(), AddressPolicy::strict()).unwrap();
    let url = format!("http://127.0.0.1:{}/x", server.address().port());
    assert_rejected(execute_public(&client, get(&url)).await);
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn public_client_refuses_obfuscated_literals() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let client = build_public_client(&settings(), AddressPolicy::strict()).unwrap();
    let port = server.address().port();

    for url in [
        format!("http://2130706433:{port}/"),
        format!("http://[::ffff:7f00:1]:{port}/"),
        format!("http://0x7f.1:{port}/"),
    ] {
        assert_rejected(execute_public(&client, get(&url)).await);
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn public_client_refuses_loopback_by_name_through_the_resolver() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let client = build_public_client(&settings(), AddressPolicy::strict()).unwrap();
    let url = format!("http://localhost:{}/x", server.address().port());
    match execute_public(&client, get(&url)).await {
        Err(HttpError::AddressRejected { host }) => assert_eq!(host, "localhost"),
        other => panic!("expected AddressRejected, got {other:?}"),
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn public_client_with_private_allowed_does_not_follow_redirects() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/start"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/other"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/other"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let client = build_public_client(&settings(), PRIVATE).unwrap();
    let url = format!("http://127.0.0.1:{}/start", server.address().port());
    let response = execute_public(&client, get(&url)).await.unwrap();
    assert_eq!(response.status().as_u16(), 302);
    let hits = server.received_requests().await.unwrap();
    assert!(hits.iter().all(|r| r.url.path() != "/other"));
}

#[tokio::test]
async fn send_with_retry_through_a_public_client_refuses_a_loopback_literal() {
    let server = ok_server().await;
    let client = build_public_client(&settings(), AddressPolicy::strict()).unwrap();
    let port = server.address().port();

    for url in [
        format!("http://127.0.0.1:{port}/x"),
        format!("http://2130706433:{port}/x"),
    ] {
        assert_rejected(send_retrying(&client, &url).await);
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn send_with_retry_through_a_private_public_client_reaches_loopback() {
    let server = ok_server().await;
    let client = build_public_client(&settings(), PRIVATE).unwrap();
    let url = format!("http://127.0.0.1:{}/x", server.address().port());

    let response = send_retrying(&client, &url).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert!(client.policy().allow_private);
}
