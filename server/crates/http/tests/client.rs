use std::path::PathBuf;
use std::time::Duration;

use postly_config::HttpSettings;
use postly_http::{HttpError, build_client, redact_header_value, redact_query_params};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn settings(connect_timeout: Duration, request_timeout: Duration) -> HttpSettings {
    HttpSettings {
        connect_timeout,
        request_timeout,
        user_agent: "postly-http-test/0".to_string(),
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
async fn outgoing_request_can_be_logged_without_leaking_the_bearer_token_or_a_signed_param() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/secure"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = build_client(&settings(Duration::from_secs(5), Duration::from_secs(5)))
        .unwrap_or_else(|err| unreachable!("building test client: {err}"));

    let url = format!(
        "{}/secure?signature=super-secret-signature&page=1",
        server.uri()
    );
    let request = client
        .get(&url)
        .header(reqwest::header::AUTHORIZATION, "Bearer super-secret-token")
        .build()
        .unwrap_or_else(|err| unreachable!("building test request: {err}"));

    let auth_value = request
        .headers()
        .get(reqwest::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let loggable_auth = redact_header_value(auth_value);

    let parsed = Url::parse(request.url().as_str())
        .unwrap_or_else(|err| unreachable!("parsing test request url: {err}"));
    let loggable_url = redact_query_params(&parsed, &["signature"]);

    let response = client.execute(request).await;
    assert!(response.is_ok());

    assert!(!loggable_auth.contains("super-secret-token"));
    assert_eq!(loggable_auth, "[redacted]");

    let rendered = loggable_url.to_string();
    assert!(!rendered.contains("super-secret-signature"));
    assert!(rendered.contains("page=1"));
}
