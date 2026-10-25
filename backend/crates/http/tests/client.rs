use std::path::PathBuf;
use std::time::Duration;

use postit_config::HttpSettings;
use postit_http::{HttpError, build_client, execute_traced};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn settings(connect_timeout: Duration, request_timeout: Duration) -> HttpSettings {
    HttpSettings {
        connect_timeout,
        request_timeout,
        user_agent: "postit-http-test/0".to_string(),
        extra_ca_files: Vec::<PathBuf>::new(),
    }
}

#[tokio::test]
async fn request_past_the_configured_timeout_is_classified_transient() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/slow"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(300)))
        .mount(&server)
        .await;

    let client = build_client(&settings(
        Duration::from_millis(500),
        Duration::from_millis(50),
    ))
    .unwrap_or_else(|err| unreachable!("building test client: {err}"));

    let result = client.get(format!("{}/slow", server.uri())).send().await;
    let err = result.err().map(HttpError::from);

    assert!(matches!(err, Some(HttpError::Transient(_))));
}

#[tokio::test]
async fn request_within_the_configured_timeout_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/fast"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let client = build_client(&settings(Duration::from_secs(5), Duration::from_secs(5)))
        .unwrap_or_else(|err| unreachable!("building test client: {err}"));

    let response = client.get(format!("{}/fast", server.uri())).send().await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn execute_traced_returns_the_response_on_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let client = build_client(&settings(Duration::from_secs(5), Duration::from_secs(5)))
        .unwrap_or_else(|err| unreachable!("building test client: {err}"));
    let request = client
        .get(format!("{}/ok", server.uri()))
        .build()
        .unwrap_or_else(|err| unreachable!("building test request: {err}"));

    let response = execute_traced(&client, request).await;

    assert!(response.is_ok());
}

#[tokio::test]
async fn execute_traced_classifies_a_connect_failure_as_transient() {
    let client = build_client(&settings(Duration::from_millis(50), Duration::from_secs(5)))
        .unwrap_or_else(|err| unreachable!("building test client: {err}"));
    // Port 1 on loopback refuses connections immediately, so this never touches the network.
    let request = client
        .get("http://127.0.0.1:1/unreachable")
        .build()
        .unwrap_or_else(|err| unreachable!("building test request: {err}"));

    let result = execute_traced(&client, request).await;

    assert!(matches!(result, Err(HttpError::Transient(_))));
}
