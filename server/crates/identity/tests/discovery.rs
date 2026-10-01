#![cfg(feature = "testkit")]

use std::time::Duration;

use postit_config::HttpSettings;
use postit_http::build_client;
use postit_identity::discovery::{HttpJwksSource, JwksSource, OidcDiscovery};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn http_client() -> reqwest::Client {
    build_client(&HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-identity-test/0".to_string(),
        extra_ca_files: Vec::new(),
    })
    .unwrap_or_else(|err| unreachable!("building test http client: {err}"))
}

async fn mount_discovery_and_jwks(server: &MockServer, jwks_body: serde_json::Value) {
    let discovery_body = serde_json::json!({
        "issuer": server.uri(),
        "jwks_uri": format!("{}/jwks.json", server.uri()),
    });
    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(discovery_body))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/jwks.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jwks_body))
        .mount(server)
        .await;
}

#[tokio::test]
async fn fetches_discovery_and_jwks() {
    let server = MockServer::start().await;
    mount_discovery_and_jwks(&server, serde_json::json!({"keys": []})).await;

    let source = HttpJwksSource::new(
        http_client(),
        Url::parse(&server.uri()).unwrap_or_else(|e| unreachable!("{e}")),
    );
    let discovery = OidcDiscovery::new(source, Duration::from_secs(3600));

    let jwks = discovery
        .jwks()
        .await
        .unwrap_or_else(|e| unreachable!("jwks: {e}"));
    assert!(jwks.keys.is_empty());
}

#[tokio::test]
async fn caches_jwks_within_the_refresh_interval() {
    let server = MockServer::start().await;
    mount_discovery_and_jwks(&server, serde_json::json!({"keys": []})).await;

    let source = HttpJwksSource::new(
        http_client(),
        Url::parse(&server.uri()).unwrap_or_else(|e| unreachable!("{e}")),
    );
    let discovery = OidcDiscovery::new(source, Duration::from_secs(3600));

    discovery
        .jwks()
        .await
        .unwrap_or_else(|e| unreachable!("first jwks: {e}"));
    discovery
        .jwks()
        .await
        .unwrap_or_else(|e| unreachable!("second jwks: {e}"));

    let requests = server.received_requests().await.unwrap_or_default();
    let jwks_requests = requests
        .iter()
        .filter(|r| r.url.path() == "/jwks.json")
        .count();
    assert_eq!(jwks_requests, 1);
}

#[tokio::test]
async fn refetches_once_on_an_unknown_kid_then_respects_the_one_minute_limit() {
    let server = MockServer::start().await;
    mount_discovery_and_jwks(&server, serde_json::json!({"keys": []})).await;

    let source = HttpJwksSource::new(
        http_client(),
        Url::parse(&server.uri()).unwrap_or_else(|e| unreachable!("{e}")),
    );
    let discovery = OidcDiscovery::new(source, Duration::from_secs(3600));
    discovery
        .jwks()
        .await
        .unwrap_or_else(|e| unreachable!("warm cache: {e}"));

    discovery
        .jwks_for_kid("missing-kid")
        .await
        .unwrap_or_else(|e| unreachable!("first lookup: {e}"));
    discovery
        .jwks_for_kid("still-missing-kid")
        .await
        .unwrap_or_else(|e| unreachable!("second lookup: {e}"));

    let requests = server.received_requests().await.unwrap_or_default();
    let jwks_requests = requests
        .iter()
        .filter(|r| r.url.path() == "/jwks.json")
        .count();
    // One warm-cache fetch, one refetch from the first unknown-kid lookup, and none from
    // the second lookup because it's within the one-minute limit.
    assert_eq!(jwks_requests, 2);
}

#[tokio::test]
async fn jwks_5xx_is_transient_and_404_is_permanent() {
    let server = postit_http::testkit::test_server().await;
    wiremock::Mock::given(wiremock::matchers::path(
        "/.well-known/openid-configuration",
    ))
    .respond_with(wiremock::ResponseTemplate::new(503))
    .mount(&server)
    .await;
    let source = HttpJwksSource::new(
        http_client(),
        url::Url::parse(&server.uri()).unwrap_or_else(|e| unreachable!("{e}")),
    );
    let err = source.discovery().await.err();
    assert!(
        matches!(err, Some(postit_http::HttpError::Transient(_))),
        "got {err:?}"
    );

    let missing = postit_http::testkit::test_server().await;
    wiremock::Mock::given(wiremock::matchers::path(
        "/.well-known/openid-configuration",
    ))
    .respond_with(wiremock::ResponseTemplate::new(404))
    .mount(&missing)
    .await;
    let source = HttpJwksSource::new(
        http_client(),
        url::Url::parse(&missing.uri()).unwrap_or_else(|e| unreachable!("{e}")),
    );
    let err = source.discovery().await.err();
    assert!(
        matches!(err, Some(postit_http::HttpError::Permanent(_))),
        "got {err:?}"
    );
}

#[tokio::test]
async fn is_loaded_flips_after_the_first_jwks_and_document_exposes_endpoints() {
    let issuer = postit_identity::testkit::TestIssuer::start().await;
    let discovery = OidcDiscovery::new(
        HttpJwksSource::new(http_client(), issuer.issuer_url()),
        std::time::Duration::from_secs(3600),
    );
    assert!(!discovery.is_loaded());
    assert!(discovery.document().is_none());
    discovery
        .jwks()
        .await
        .unwrap_or_else(|e| unreachable!("jwks: {e}"));
    assert!(discovery.is_loaded());
    let doc = discovery
        .document()
        .unwrap_or_else(|| unreachable!("document cached"));
    assert!(
        doc.authorization_endpoint
            .is_some_and(|u| u.ends_with("/authorize"))
    );
    assert!(doc.token_endpoint.is_some_and(|u| u.ends_with("/token")));
}

#[tokio::test]
async fn an_unknown_kid_during_an_outage_returns_the_cached_set_and_refetches_once() {
    let server = MockServer::builder().start().await; // dedicated, never pooled
    mount_discovery_and_jwks(&server, serde_json::json!({"keys": []})).await;
    let source = HttpJwksSource::new(
        http_client(),
        Url::parse(&server.uri()).unwrap_or_else(|e| unreachable!("{e}")),
    );
    let discovery = OidcDiscovery::new(source, Duration::from_secs(3600));
    discovery
        .jwks()
        .await
        .unwrap_or_else(|e| unreachable!("warm cache: {e}"));

    // The IdP goes down.
    server.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    for kid in ["random-1", "random-2", "random-3"] {
        let jwks = discovery
            .jwks_for_kid(kid)
            .await
            .unwrap_or_else(|e| unreachable!("a cached set must be returned for {kid}: {e}"));
        assert!(jwks.keys.is_empty());
    }
    let requests = server.received_requests().await.unwrap_or_default();
    assert_eq!(
        requests.len(),
        1,
        "only the first unknown kid may try the IdP within the one-minute window"
    );
}
