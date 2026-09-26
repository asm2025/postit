#![cfg(feature = "testkit")]

use std::time::Duration;

use postit_config::HttpSettings;
use postit_http::build_client;
use postit_identity::discovery::{HttpJwksSource, OidcDiscovery};
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
