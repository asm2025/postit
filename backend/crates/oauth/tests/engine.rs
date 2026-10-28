//! The provider-neutral OAuth token client against the mock OAuth provider.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::fmt::Write as _;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{TimeZone, Utc};
use emixcrypto::HashAlgorithm as _;
use postit_agent_kit::testkit::{MockOAuthProvider, OAuthFailure};
use postit_core::{ClientAuth, HttpHandles, OAuthProviderSpec, PlatformId};
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use postit_oauth::engine::{
    ClientCredentials, Pkce, TokenError, authorize_url, exchange_code, missing_scopes,
    redirect_uri, refresh, revoke,
};
use secrecy::{ExposeSecret as _, SecretString};
use serde_json::json;
use url::Url;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CLIENT_ID: &str = "app-1";
const CLIENT_SECRET: &str = "app-secret-value";

fn handles() -> HttpHandles {
    let settings = postit_config::HttpSettings {
        connect_timeout: Duration::from_secs(2),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-test/0".to_owned(),
        extra_ca_files: Vec::new(),
    };
    let standard = postit_http::build_client(&settings).unwrap();
    let public =
        postit_http::build_public_client(&settings, postit_http::AddressPolicy::strict()).unwrap();
    postit_http::agent_handles(standard, public)
}

fn client() -> ClientCredentials {
    ClientCredentials {
        id: CLIENT_ID.to_owned(),
        secret: SecretString::from(CLIENT_SECRET.to_owned()),
    }
}

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 28, 12, 0, 0).unwrap()
}

fn callback() -> Url {
    redirect_uri(
        &Url::parse("https://postit.test").unwrap(),
        &PlatformId::new("mock_oauth").unwrap(),
    )
}

fn secret(value: &str) -> SecretString {
    SecretString::from(value.to_owned())
}

fn query(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

fn sha256_hex(text: &str) -> String {
    emixcrypto::Sha256Hash::new()
        .compute_hash_bytes(text.as_bytes())
        .unwrap()
        .iter()
        .fold(String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

fn form(body: &[u8]) -> Vec<(String, String)> {
    url::form_urlencoded::parse(body).into_owned().collect()
}

async fn provider() -> MockOAuthProvider {
    let provider = MockOAuthProvider::start().await;
    provider.set_client(CLIENT_ID, CLIENT_SECRET);
    provider
}

#[test]
fn authorize_url_carries_pkce_state_scopes_and_extras() {
    let spec = OAuthProviderSpec::new(
        Url::parse("https://auth.example/authorize?fixed=1").unwrap(),
        Url::parse("https://auth.example/token").unwrap(),
    )
    .with_scopes(["read", "write"])
    .with_extra_authorize_param("prompt", "consent");
    let pkce = Pkce::generate();
    let url = authorize_url(&spec, &client(), &callback(), "state-123", Some(&pkce));

    assert_eq!(url.path(), "/authorize");
    assert_eq!(query(&url, "fixed").as_deref(), Some("1"));
    assert_eq!(query(&url, "response_type").as_deref(), Some("code"));
    assert_eq!(query(&url, "client_id").as_deref(), Some(CLIENT_ID));
    assert_eq!(
        query(&url, "redirect_uri").as_deref(),
        Some(callback().as_str())
    );
    assert_eq!(query(&url, "scope").as_deref(), Some("read write"));
    assert_eq!(query(&url, "state").as_deref(), Some("state-123"));
    assert_eq!(
        query(&url, "code_challenge_method").as_deref(),
        Some("S256")
    );
    assert_eq!(query(&url, "prompt").as_deref(), Some("consent"));
    assert!(query(&url, "client_secret").is_none());

    // The challenge is base64url(SHA-256(verifier)) of a 32-byte verifier.
    let verifier = pkce.verifier.expose_secret();
    assert_eq!(URL_SAFE_NO_PAD.decode(verifier).unwrap().len(), 32);
    let digest = emixcrypto::Sha256Hash::new()
        .compute_hash_bytes(verifier.as_bytes())
        .unwrap();
    assert_eq!(pkce.challenge, URL_SAFE_NO_PAD.encode(digest));
    assert_eq!(
        query(&url, "code_challenge").as_deref(),
        Some(pkce.challenge.as_str())
    );
    assert!(!url.as_str().contains(verifier));

    // A comma separator and a custom client ID parameter; no PKCE.
    let spec = spec
        .with_scope_separator(",")
        .with_client_id_param("client_key");
    let url = authorize_url(&spec, &client(), &callback(), "s", None);
    assert_eq!(query(&url, "scope").as_deref(), Some("read,write"));
    assert_eq!(query(&url, "client_key").as_deref(), Some(CLIENT_ID));
    assert!(query(&url, "client_id").is_none());
    assert!(query(&url, "code_challenge").is_none());
    assert!(query(&url, "code_challenge_method").is_none());
}

#[tokio::test]
async fn exchange_with_pkce_succeeds_and_parses_expiry() {
    let provider = provider().await;
    provider.set_granted_scopes(Some(vec!["read".to_owned(), "write".to_owned()]));
    let spec = provider.spec();
    let pkce = Pkce::generate();
    let url = authorize_url(&spec, &client(), &callback(), "state", Some(&pkce));
    let code = provider.approve(&url);

    let exchange = exchange_code(
        &handles(),
        &spec,
        &client(),
        &callback(),
        &secret(&code),
        Some(&pkce.verifier),
        now(),
    )
    .await
    .unwrap();

    assert_eq!(exchange.tokens.access_token.expose_secret(), "access-1");
    assert_eq!(
        exchange
            .tokens
            .refresh_token
            .as_ref()
            .map(|t| t.expose_secret().to_owned())
            .as_deref(),
        Some("refresh-1")
    );
    assert_eq!(
        exchange.tokens.expires_at,
        Some(now() + chrono::Duration::seconds(3600))
    );
    assert_eq!(exchange.tokens.scopes, vec!["read", "write"]);
    assert_eq!(
        exchange.raw.get("token_type").and_then(|v| v.as_str()),
        Some("Bearer")
    );
    assert_eq!(provider.token_requests(), 1);
}

#[tokio::test]
async fn default_access_ttl_applies_when_expires_in_is_missing() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "access_token": "access-x" })),
        )
        .mount(&server)
        .await;
    let base = Url::parse(&format!("{}/", server.uri())).unwrap();
    let spec = OAuthProviderSpec::new(base.join("authorize").unwrap(), base.join("token").unwrap());
    let exchange = exchange_code(
        &handles(),
        &spec,
        &client(),
        &callback(),
        &secret("code"),
        None,
        now(),
    )
    .await
    .unwrap();
    assert_eq!(exchange.tokens.expires_at, None);
    assert!(exchange.tokens.refresh_token.is_none());

    let spec = spec.with_default_access_ttl(Duration::from_secs(600));
    let exchange = exchange_code(
        &handles(),
        &spec,
        &client(),
        &callback(),
        &secret("code"),
        None,
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        exchange.tokens.expires_at,
        Some(now() + chrono::Duration::seconds(600))
    );
}

#[tokio::test]
async fn exchange_with_wrong_verifier_is_invalid_grant() {
    let provider = provider().await;
    let spec = provider.spec();
    let pkce = Pkce::generate();
    let url = authorize_url(&spec, &client(), &callback(), "state", Some(&pkce));
    let code = provider.approve(&url);
    let other = Pkce::generate();

    let err = exchange_code(
        &handles(),
        &spec,
        &client(),
        &callback(),
        &secret(&code),
        Some(&other.verifier),
        now(),
    )
    .await
    .unwrap_err();
    assert_eq!(err, TokenError::InvalidGrant);
    assert!(!err.retryable());
}

#[tokio::test]
async fn post_body_and_basic_client_auth_both_work() {
    let provider = provider().await;
    let basic = provider.spec().with_client_auth(ClientAuth::Basic);
    exchange_code(
        &handles(),
        &basic,
        &client(),
        &callback(),
        &secret("code-a"),
        None,
        now(),
    )
    .await
    .unwrap();
    let request = provider.requests().pop().unwrap();
    assert!(request.headers.get("authorization").is_some());
    assert!(
        !form(&request.body)
            .iter()
            .any(|(k, _)| k == "client_secret")
    );

    let post_body = provider.spec().with_client_auth(ClientAuth::PostBody);
    exchange_code(
        &handles(),
        &post_body,
        &client(),
        &callback(),
        &secret("code-b"),
        None,
        now(),
    )
    .await
    .unwrap();
    let request = provider.requests().pop().unwrap();
    assert!(request.headers.get("authorization").is_none());
    let fields = form(&request.body);
    assert!(fields.contains(&("client_id".to_owned(), CLIENT_ID.to_owned())));
    assert!(fields.contains(&("client_secret".to_owned(), CLIENT_SECRET.to_owned())));

    // A wrong secret is the deployment's fault.
    let wrong = ClientCredentials {
        id: CLIENT_ID.to_owned(),
        secret: secret("not-the-secret"),
    };
    let err = exchange_code(
        &handles(),
        &post_body,
        &wrong,
        &callback(),
        &secret("code-c"),
        None,
        now(),
    )
    .await
    .unwrap_err();
    assert_eq!(err, TokenError::InvalidClient);
    assert!(!err.retryable());
}

#[tokio::test]
async fn public_client_auth_sends_the_id_only() {
    let provider = MockOAuthProvider::start().await;
    let spec = provider.spec().with_client_auth(ClientAuth::None);
    exchange_code(
        &handles(),
        &spec,
        &client(),
        &callback(),
        &secret("code-a"),
        None,
        now(),
    )
    .await
    .unwrap();
    let request = provider.requests().pop().unwrap();
    assert!(request.headers.get("authorization").is_none());
    let fields = form(&request.body);
    assert!(fields.contains(&("client_id".to_owned(), CLIENT_ID.to_owned())));
    assert!(!fields.iter().any(|(k, _)| k == "client_secret"));
}

#[tokio::test]
async fn custom_client_id_param_is_sent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("client_key=app-1"))
        .and(body_string_contains("grant_extra=1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access-x",
            "refresh_token": "refresh-x",
            "expires_in": "7200",
        })))
        .expect(2)
        .mount(&server)
        .await;
    let base = Url::parse(&format!("{}/", server.uri())).unwrap();
    let spec = OAuthProviderSpec::new(base.join("authorize").unwrap(), base.join("token").unwrap())
        .with_client_auth(ClientAuth::PostBody)
        .with_client_id_param("client_key")
        .with_extra_token_param("grant_extra", "1");

    let exchange = exchange_code(
        &handles(),
        &spec,
        &client(),
        &callback(),
        &secret("code"),
        None,
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        exchange.tokens.expires_at,
        Some(now() + chrono::Duration::seconds(7200))
    );
    refresh(&handles(), &spec, &client(), &secret("refresh-x"), now())
        .await
        .unwrap();

    for request in server.received_requests().await.unwrap() {
        let fields = form(&request.body);
        assert!(!fields.iter().any(|(k, _)| k == "client_id"));
        assert!(fields.contains(&("client_secret".to_owned(), CLIENT_SECRET.to_owned())));
    }
}

#[tokio::test]
async fn refresh_without_rotation_returns_no_refresh_token() {
    let provider = provider().await;
    provider.set_refresh_issued(false);
    let spec = provider.spec();
    let refreshed = refresh(&handles(), &spec, &client(), &secret("refresh-1"), now())
        .await
        .unwrap();
    assert_eq!(refreshed.tokens.access_token.expose_secret(), "access-1");
    assert!(refreshed.tokens.refresh_token.is_none());
    assert_eq!(
        refreshed.tokens.expires_at,
        Some(now() + chrono::Duration::seconds(3600))
    );
    let request = provider.requests().pop().unwrap();
    let fields = form(&request.body);
    assert!(fields.contains(&("grant_type".to_owned(), "refresh_token".to_owned())));
    assert!(fields.contains(&("refresh_token".to_owned(), "refresh-1".to_owned())));
}

#[tokio::test]
async fn refresh_invalid_grant_is_classified() {
    let provider = provider().await;
    let spec = provider.spec();
    provider.fail_next(OAuthFailure::InvalidGrant);
    let err = refresh(&handles(), &spec, &client(), &secret("refresh-1"), now())
        .await
        .unwrap_err();
    assert_eq!(err, TokenError::InvalidGrant);
    assert!(!err.retryable());

    provider.fail_next(OAuthFailure::ExpiredRefreshToken);
    let err = refresh(&handles(), &spec, &client(), &secret("refresh-1"), now())
        .await
        .unwrap_err();
    assert_eq!(err, TokenError::InvalidGrant);
}

#[tokio::test]
async fn server_error_is_retryable() {
    // A code exchange is not resent: the 5xx is an unknown outcome, retryable later.
    let provider = provider().await;
    let spec = provider.spec();
    provider.fail_next(OAuthFailure::ServerError);
    let err = exchange_code(
        &handles(),
        &spec,
        &client(),
        &callback(),
        &secret("code"),
        None,
        now(),
    )
    .await
    .unwrap_err();
    assert_eq!(err, TokenError::Transport);
    assert!(err.retryable());

    // A non-rotating refresh retries in place; when every attempt fails, the provider's code
    // survives and is retryable.
    for _ in 0..3 {
        provider.fail_next(OAuthFailure::ServerError);
    }
    let err = refresh(&handles(), &spec, &client(), &secret("refresh-1"), now())
        .await
        .unwrap_err();
    assert_eq!(err, TokenError::Provider("server_error".to_owned()));
    assert!(err.retryable());
}

#[tokio::test]
async fn code_exchange_is_not_resent_after_a_5xx() {
    let provider = provider().await;
    let spec = provider.spec();
    provider.fail_next(OAuthFailure::ServerError);
    let err = exchange_code(
        &handles(),
        &spec,
        &client(),
        &callback(),
        &secret("code"),
        None,
        now(),
    )
    .await
    .unwrap_err();
    assert_eq!(err, TokenError::Transport);
    assert_eq!(provider.token_requests(), 1);
}

#[tokio::test]
async fn refresh_resend_follows_rotation() {
    let provider = provider().await;
    provider.set_rotation(false);
    let spec = provider.spec();
    provider.fail_next(OAuthFailure::ServerError);
    refresh(&handles(), &spec, &client(), &secret("refresh-1"), now())
        .await
        .unwrap();
    assert_eq!(provider.token_requests(), 2);

    let provider = self::provider().await;
    provider.set_rotation(true);
    let spec = provider.spec();
    assert!(spec.refresh.rotates);
    provider.fail_next(OAuthFailure::ServerError);
    let err = refresh(&handles(), &spec, &client(), &secret("refresh-1"), now())
        .await
        .unwrap_err();
    assert_eq!(err, TokenError::Transport);
    assert_eq!(provider.token_requests(), 1);
}

#[test]
fn redirect_uri_has_no_double_slash() {
    let id = PlatformId::new("mock_oauth").unwrap();
    for base in ["https://postit.test", "https://postit.test/"] {
        let url = redirect_uri(&Url::parse(base).unwrap(), &id);
        assert_eq!(
            url.as_str(),
            "https://postit.test/api/v1/platforms/mock_oauth/oauth/callback"
        );
    }
    for base in ["https://postit.test/base", "https://postit.test/base/"] {
        let url = redirect_uri(&Url::parse(base).unwrap(), &id);
        assert_eq!(
            url.as_str(),
            "https://postit.test/base/api/v1/platforms/mock_oauth/oauth/callback"
        );
    }
}

#[tokio::test]
async fn revoke_sends_token_and_hint() {
    let provider = provider().await;
    let spec = provider.spec();
    revoke(
        &handles(),
        &spec,
        &client(),
        &secret("refresh-9"),
        "refresh_token",
    )
    .await
    .unwrap();
    assert_eq!(provider.revoked(), vec![sha256_hex("refresh-9")]);
    let request = provider.requests().pop().unwrap();
    assert!(request.url.path().ends_with("/revoke"));
    let fields = form(&request.body);
    assert!(fields.contains(&("token_type_hint".to_owned(), "refresh_token".to_owned())));
    assert!(request.headers.get("authorization").is_some());

    // No revocation endpoint: nothing is sent.
    let mut no_revoke = spec.clone();
    no_revoke.revoke_url = None;
    let before = provider.requests().len();
    assert!(
        revoke(
            &handles(),
            &no_revoke,
            &client(),
            &secret("t"),
            "access_token"
        )
        .await
        .is_err()
    );
    assert_eq!(provider.requests().len(), before);
}

#[tokio::test]
async fn missing_scopes_only_when_response_lists_scopes() {
    let provider = provider().await;
    let spec = provider.spec();
    assert!(missing_scopes(&spec, &[]).is_empty());
    assert_eq!(missing_scopes(&spec, &["read".to_owned()]), vec!["write"]);
    assert!(missing_scopes(&spec, &["write".to_owned(), "read".to_owned()]).is_empty());

    // Through a real response: no `scope`, then a narrower one.
    let exchange = exchange_code(
        &handles(),
        &spec,
        &client(),
        &callback(),
        &secret("code-a"),
        None,
        now(),
    )
    .await
    .unwrap();
    assert!(exchange.tokens.scopes.is_empty());
    assert!(missing_scopes(&spec, &exchange.tokens.scopes).is_empty());

    provider.set_granted_scopes(Some(vec!["read".to_owned()]));
    let exchange = exchange_code(
        &handles(),
        &spec,
        &client(),
        &callback(),
        &secret("code-b"),
        None,
        now(),
    )
    .await
    .unwrap();
    assert_eq!(exchange.tokens.scopes, vec!["read"]);
    assert_eq!(
        missing_scopes(&spec, &exchange.tokens.scopes),
        vec!["write"]
    );
}

#[tokio::test]
async fn token_error_display_has_no_body() {
    const LEAKED: &str = "tok-SECRET-1234567890";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_request",
            "error_description": format!("the code {LEAKED} is malformed"),
            "access_token": LEAKED,
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": LEAKED,
        })))
        .mount(&server)
        .await;
    let base = Url::parse(&format!("{}/", server.uri())).unwrap();
    let spec = OAuthProviderSpec::new(base.join("authorize").unwrap(), base.join("token").unwrap());

    let (logs, guard) = LogCapture::install();
    let err = exchange_code(
        &handles(),
        &spec,
        &client(),
        &callback(),
        &secret("the-code-value"),
        None,
        now(),
    )
    .await
    .unwrap_err();
    assert_eq!(err, TokenError::Provider("invalid_request".to_owned()));

    // An `error` field that is not an OAuth error code is not echoed.
    let odd = refresh(
        &handles(),
        &spec,
        &client(),
        &secret("refresh-value"),
        now(),
    )
    .await
    .unwrap_err();
    drop(guard);
    assert!(matches!(odd, TokenError::Provider(_)));

    let outputs = [
        format!("{err:?}"),
        err.to_string(),
        format!("{odd:?}"),
        odd.to_string(),
        logs.text(),
    ];
    assert_no_leak(
        &[
            LEAKED,
            CLIENT_SECRET,
            "the-code-value",
            "refresh-value",
            &LEAKED.to_lowercase(),
        ],
        &[
            ("err debug", &outputs[0]),
            ("err display", &outputs[1]),
            ("odd debug", &outputs[2]),
            ("odd display", &outputs[3]),
            ("logs", &outputs[4]),
        ],
    );
}

#[test]
fn client_credentials_come_from_config() {
    let creds = ClientCredentials::from_config(&json!({
        "client_id": CLIENT_ID,
        "client_secret": CLIENT_SECRET,
    }))
    .unwrap();
    assert_eq!(creds.id, CLIENT_ID);
    assert_eq!(creds.secret.expose_secret(), CLIENT_SECRET);
    assert!(!format!("{creds:?}").contains(CLIENT_SECRET));

    for config in [
        json!({}),
        json!({ "client_id": CLIENT_ID }),
        json!({ "client_secret": CLIENT_SECRET }),
        json!({ "client_id": "", "client_secret": CLIENT_SECRET }),
        json!({ "client_id": CLIENT_ID, "client_secret": 5 }),
    ] {
        assert!(matches!(
            ClientCredentials::from_config(&config),
            Err(postit_oauth::AccountError::PlatformUnavailable)
        ));
    }
}
