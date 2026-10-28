//! Tests for the testkit fakes, contexts, and the mock OAuth provider.
#![cfg(feature = "testkit")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Duration;

use bytes::Bytes;
use futures::StreamExt;
use postit_agent_kit::testkit::{
    MemoryCredentialStore, MemoryMediaHost, MemoryMediaStore, MockOAuthProvider, OAuthFailure,
    TestContext, responders,
};
use postit_core::{
    AccountId, AccountRef, AccountToken, CredentialStore, Credentials, MediaAsset, MediaAssetId,
    MediaAssetState, MediaError, MediaHost, MediaKind, MediaStore, OAuthTokens, PlatformId, UserId,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn asset(id: MediaAssetId, bytes: u64) -> MediaAsset {
    MediaAsset {
        id,
        owner_id: UserId::from_uuid(Uuid::nil()),
        kind: MediaKind::Video,
        mime: "video/mp4".to_owned(),
        bytes,
        width: None,
        height: None,
        duration_ms: None,
        sha256: None,
        state: MediaAssetState::Ready,
        derived_from: None,
    }
}

#[tokio::test]
async fn media_store_streams_inserted_bytes() {
    let store = MemoryMediaStore::new();
    let id = MediaAssetId::from_uuid(Uuid::from_u128(1));
    let data: Vec<u8> = (0..200 * 1024_usize)
        .map(|i| u8::try_from(i % 251).unwrap())
        .collect();
    store.insert(id, Bytes::from(data.clone()));

    let mut stream = store.open(&asset(id, data.len() as u64)).await.unwrap();
    let mut pieces = 0;
    let mut out = Vec::new();
    while let Some(piece) = stream.next().await {
        out.extend_from_slice(&piece.unwrap());
        pieces += 1;
    }
    assert!(pieces >= 4, "got {pieces} pieces");
    assert_eq!(out, data);

    let missing = MediaAssetId::from_uuid(Uuid::from_u128(2));
    let err = store.open(&asset(missing, 0)).await.err().unwrap();
    assert_eq!(err, MediaError::NotFound);
}

#[test]
fn media_host_records_issued_urls() {
    let host = MemoryMediaHost::new();
    let id = MediaAssetId::from_uuid(Uuid::from_u128(7));
    let url = host
        .public_url(&asset(id, 1), Duration::from_secs(90))
        .unwrap();
    assert_eq!(url.as_str(), format!("https://media.test/{id}?ttl=90"));
    assert_eq!(host.issued(), vec![(id, Duration::from_secs(90))]);
}

#[tokio::test]
async fn credential_store_records_refresh() {
    let empty = MemoryCredentialStore::empty();
    assert!(empty.credentials().await.is_err());

    let tokens = OAuthTokens {
        access_token: SecretString::from("a1"),
        refresh_token: None,
        expires_at: None,
        scopes: vec!["read".to_owned()],
    };
    let store = MemoryCredentialStore::new(Credentials::OAuth(tokens.clone()));
    assert!(store.refreshed().is_empty());
    let fresh = OAuthTokens {
        access_token: SecretString::from("a2"),
        ..tokens
    };
    store.store_refreshed(fresh).await.unwrap();
    assert_eq!(store.refreshed().len(), 1);
    let Credentials::OAuth(now) = store.credentials().await.unwrap() else {
        panic!("expected OAuth credentials");
    };
    assert_eq!(now.access_token.expose_secret(), "a2");
}

async fn post(url: url::Url, form: &[(&str, &str)]) -> (u16, serde_json::Value) {
    let ctx = TestContext::new();
    let response = postit_agent_kit::standard(&ctx.http)
        .unwrap()
        .post(url)
        .form(form)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let body = response.json().await.unwrap_or(serde_json::Value::Null);
    (status, body)
}

#[tokio::test]
async fn oauth_code_exchange_and_refresh() {
    for (rotates, expected) in [(false, "refresh-1"), (true, "refresh-2")] {
        let provider = MockOAuthProvider::start().await;
        provider.set_rotation(rotates);
        let token = provider.spec().token_url;

        let (status, body) = post(
            token.clone(),
            &[("grant_type", "authorization_code"), ("code", "c")],
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["access_token"], "access-1");
        assert_eq!(body["refresh_token"], "refresh-1");
        assert_eq!(body["expires_in"], 3600);

        let (status, body) = post(
            token,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", "refresh-1"),
            ],
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["access_token"], "access-2");
        assert_eq!(body["refresh_token"], expected);
        assert_eq!(provider.requests().len(), 2);
    }
}

#[tokio::test]
async fn oauth_scripted_invalid_grant() {
    let provider = MockOAuthProvider::start().await;
    provider.fail_next(OAuthFailure::InvalidGrant);
    let token = provider.spec().token_url;
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", "secret-r"),
    ];

    let (status, body) = post(token.clone(), &form).await;
    assert_eq!(status, 400);
    assert_eq!(body, serde_json::json!({"error": "invalid_grant"}));

    let (status, _) = post(token, &form).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn oauth_scripted_expired_refresh_token_and_server_error() {
    let provider = MockOAuthProvider::start().await;
    provider.fail_next(OAuthFailure::ExpiredRefreshToken);
    provider.fail_next(OAuthFailure::ServerError);
    let token = provider.spec().token_url;
    let form = [("grant_type", "refresh_token"), ("refresh_token", "r")];

    let (status, body) = post(token.clone(), &form).await;
    assert_eq!(status, 400);
    assert_eq!(body["error"], "invalid_grant");
    assert_eq!(body["error_description"], "The refresh token has expired.");

    let (status, body) = post(token.clone(), &form).await;
    assert_eq!(status, 500);
    assert_eq!(body, serde_json::json!({"error": "server_error"}));

    let (status, _) = post(token, &form).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn oauth_revoke_ignores_queued_failures() {
    let provider = MockOAuthProvider::start().await;
    provider.fail_next(OAuthFailure::ServerError);
    let revoke = provider.spec().revoke_url.unwrap();
    let (status, _) = post(revoke, &[("token", "t")]).await;
    assert_eq!(status, 200);
    assert_eq!(provider.revoked().len(), 1);
}

#[tokio::test]
async fn oauth_spec_is_a_snapshot_of_rotation() {
    let provider = MockOAuthProvider::start().await;
    let before = provider.spec();
    provider.set_rotation(true);
    assert!(!before.refresh.rotates);
    assert!(provider.spec().refresh.rotates);
}

#[tokio::test]
async fn oauth_revoke_records_hash_only() {
    let provider = MockOAuthProvider::start().await;
    let revoke = provider.spec().revoke_url.unwrap();
    let (status, _) = post(revoke, &[("token", "super-secret")]).await;
    assert_eq!(status, 200);

    // SHA-256 of "super-secret".
    assert_eq!(
        provider.revoked(),
        vec!["aec8084845b41a6952d46cbaa1c9b798659487ffd133796d95d05ba45d9096c2".to_owned()]
    );
}

#[tokio::test]
async fn oauth_spec_urls() {
    let provider = MockOAuthProvider::start().await;
    let base = provider.base_url();
    let spec = provider.spec();
    assert_eq!(spec.authorize_url.path(), "/authorize");
    assert_eq!(spec.token_url.as_str(), format!("{base}token"));
    assert_eq!(spec.scopes, vec!["read", "write"]);
}

#[tokio::test]
async fn context_public_client_reaches_loopback() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ping"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let ctx = TestContext::new();
    let client = postit_agent_kit::public(&ctx.http).unwrap();
    let request = reqwest::Request::new(
        reqwest::Method::GET,
        format!("{}/ping", server.uri()).parse().unwrap(),
    );
    let response = postit_http::execute_public(client, request).await.unwrap();
    assert_eq!(response.status(), 200);
    assert!(postit_agent_kit::standard(&ctx.http).is_ok());

    let agent = ctx.agent_context(serde_json::json!({"k": 1}));
    assert_eq!(agent.config["k"], 1);
    assert_eq!(
        ctx.check_context(serde_json::json!({})).config,
        serde_json::json!({})
    );
    assert_eq!(
        ctx.event_context(serde_json::Value::Null).config,
        serde_json::Value::Null
    );
}

#[tokio::test]
async fn responders_shape_responses() {
    let server = MockServer::start().await;
    Mock::given(path("/429"))
        .respond_with(responders::rate_limited(Some(7)))
        .mount(&server)
        .await;
    Mock::given(path("/500"))
        .respond_with(responders::server_error())
        .mount(&server)
        .await;
    Mock::given(path("/bad"))
        .respond_with(responders::malformed_json())
        .mount(&server)
        .await;
    let ctx = TestContext::new();
    let client = postit_agent_kit::standard(&ctx.http).unwrap();
    let get = |p: &str| client.get(format!("{}{p}", server.uri())).send();
    let r = get("/429").await.unwrap();
    assert_eq!(r.status(), 429);
    assert_eq!(r.headers()["retry-after"], "7");
    assert_eq!(get("/500").await.unwrap().status(), 500);
    assert!(
        get("/bad")
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .is_err()
    );
}

#[tokio::test]
async fn dropped_response_outlasts_the_client_timeout() {
    let server = MockServer::start().await;
    Mock::given(path("/slow"))
        .respond_with(responders::dropped())
        .mount(&server)
        .await;
    let ctx = TestContext::new();
    let client = postit_agent_kit::standard(&ctx.http).unwrap();
    let started = std::time::Instant::now();
    let err = client
        .get(format!("{}/slow", server.uri()))
        .send()
        .await
        .unwrap_err();
    assert!(err.is_timeout(), "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

fn s256(verifier: &str) -> String {
    use base64::Engine as _;
    use sha2::Digest as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier))
}

fn authorize_url(provider: &MockOAuthProvider, challenge: &str, redirect: &str) -> url::Url {
    let mut url = provider.spec().authorize_url;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("state", "st-1")
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("redirect_uri", redirect);
    url
}

const REDIRECT: &str = "https://postit.local/callback";

async fn exchange(
    provider: &MockOAuthProvider,
    code: &str,
    verifier: &str,
    redirect: &str,
) -> (u16, serde_json::Value) {
    post(
        provider.spec().token_url,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect),
        ],
    )
    .await
}

#[tokio::test]
async fn approved_code_requires_matching_verifier() {
    let provider = MockOAuthProvider::start().await;
    let code = provider.approve(&authorize_url(&provider, &s256("verifier-1"), REDIRECT));
    let (status, body) = exchange(&provider, &code, "wrong", REDIRECT).await;
    assert_eq!((status, &body["error"]), (400, &json!("invalid_grant")));
    let code = provider.approve(&authorize_url(&provider, &s256("verifier-1"), REDIRECT));
    let (status, body) = exchange(&provider, &code, "verifier-1", REDIRECT).await;
    assert_eq!(status, 200);
    assert!(body["access_token"].is_string());
}

#[tokio::test]
async fn approved_code_is_single_use() {
    let provider = MockOAuthProvider::start().await;
    let code = provider.approve(&authorize_url(&provider, &s256("v"), REDIRECT));
    assert_eq!(exchange(&provider, &code, "v", REDIRECT).await.0, 200);
    let (status, body) = exchange(&provider, &code, "v", REDIRECT).await;
    assert_eq!((status, &body["error"]), (400, &json!("invalid_grant")));
}

#[tokio::test]
async fn approved_code_requires_same_redirect_uri() {
    let provider = MockOAuthProvider::start().await;
    let code = provider.approve(&authorize_url(&provider, &s256("v"), REDIRECT));
    let (status, body) = exchange(&provider, &code, "v", "https://evil.test/cb").await;
    assert_eq!((status, &body["error"]), (400, &json!("invalid_grant")));
}

#[tokio::test]
async fn wrong_client_is_rejected() {
    let provider = MockOAuthProvider::start().await;
    provider.set_client("client-1", "secret-1");
    let token = provider.spec().token_url;
    let form = [("grant_type", "authorization_code"), ("code", "c")];
    let (status, body) = post(token.clone(), &form).await;
    assert_eq!((status, &body["error"]), (401, &json!("invalid_client")));

    let mut bad = form.to_vec();
    bad.extend([("client_id", "client-1"), ("client_secret", "nope")]);
    assert_eq!(post(token.clone(), &bad).await.0, 401);

    let mut good = form.to_vec();
    good.extend([("client_id", "client-1"), ("client_secret", "secret-1")]);
    assert_eq!(post(token.clone(), &good).await.0, 200);

    let ctx = TestContext::new();
    let response = postit_agent_kit::standard(&ctx.http)
        .unwrap()
        .post(token)
        .basic_auth("client-1", Some("secret-1"))
        .form(&form)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(provider.token_requests(), 4);
}

#[tokio::test]
async fn granted_scopes_are_reported() {
    let provider = MockOAuthProvider::start().await;
    let token = provider.spec().token_url;
    let form = [("grant_type", "authorization_code"), ("code", "c")];
    let (_, body) = post(token.clone(), &form).await;
    assert!(body.get("scope").is_none());
    provider.set_granted_scopes(Some(vec!["read".to_owned(), "write".to_owned()]));
    let (_, body) = post(token, &form).await;
    assert_eq!(body["scope"], "read write");
}

#[tokio::test]
async fn refresh_token_can_be_withheld() {
    let provider = MockOAuthProvider::start().await;
    provider.set_refresh_issued(false);
    let (_, body) = post(
        provider.spec().token_url,
        &[("grant_type", "authorization_code"), ("code", "c")],
    )
    .await;
    assert!(body.get("refresh_token").is_none());
    assert!(body["access_token"].is_string());
}

#[tokio::test]
async fn delay_postpones_the_token_answer() {
    let provider = MockOAuthProvider::start().await;
    provider.set_delay(Some(Duration::from_millis(300)));
    let started = std::time::Instant::now();
    let (status, _) = post(
        provider.spec().token_url,
        &[("grant_type", "authorization_code"), ("code", "c")],
    )
    .await;
    assert_eq!(status, 200);
    assert!(started.elapsed() >= Duration::from_millis(300));
}

#[tokio::test]
async fn credential_store_refreshes_and_serves_account_tokens() {
    let tokens = |a: &str| OAuthTokens {
        access_token: SecretString::from(a.to_owned()),
        refresh_token: None,
        expires_at: None,
        scopes: Vec::new(),
    };
    let store = MemoryCredentialStore::new(Credentials::OAuth(tokens("a1")))
        .with_refresh_script(vec![tokens("a2")])
        .with_account_token(
            "page-1",
            AccountToken {
                access_token: SecretString::from("pt".to_owned()),
                expires_at: None,
            },
        );
    let fresh = store.refresh(&tokens("a1")).await.unwrap();
    assert_eq!(fresh.access_token.expose_secret(), "a2");
    assert_eq!(store.refresh_calls(), 1);
    assert!(store.refresh(&tokens("a2")).await.is_err());
    assert_eq!(store.refresh_calls(), 2);

    let account = |remote: &str| AccountRef {
        platform: PlatformId::new("echo").unwrap(),
        account: AccountId::from_uuid(Uuid::nil()),
        remote_id: remote.to_owned(),
    };
    let got = store
        .account_token(&account("page-1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.access_token.expose_secret(), "pt");
    assert!(
        store
            .account_token(&account("other"))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn revoke_requires_the_set_client_and_is_not_a_token_request() {
    let provider = MockOAuthProvider::start().await;
    provider.set_client("client-1", "secret-1");
    let revoke = provider.spec().revoke_url.unwrap();
    let (status, body) = post(revoke.clone(), &[("token", "t")]).await;
    assert_eq!((status, &body["error"]), (401, &json!("invalid_client")));
    let good = [
        ("token", "t"),
        ("client_id", "client-1"),
        ("client_secret", "secret-1"),
    ];
    assert_eq!(post(revoke, &good).await.0, 200);
    assert_eq!(provider.revoked().len(), 1);
    assert_eq!(provider.token_requests(), 0);
    assert_eq!(provider.requests().len(), 2);
}

#[tokio::test]
async fn basic_credentials_are_form_urldecoded() {
    // RFC 6749 section 2.3.1: the ID and secret are form-urlencoded before Basic encoding.
    let provider = MockOAuthProvider::start().await;
    provider.set_client("client:1", "s3cret/+ &");
    let ctx = TestContext::new();
    let id: String = url::form_urlencoded::byte_serialize(b"client:1").collect();
    let secret: String = url::form_urlencoded::byte_serialize(b"s3cret/+ &").collect();
    let response = postit_agent_kit::standard(&ctx.http)
        .unwrap()
        .post(provider.spec().token_url)
        .basic_auth(id, Some(secret))
        .form(&[("grant_type", "authorization_code"), ("code", "c")])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
}
