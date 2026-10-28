//! The `mock_oauth` agent's scripted exchange, discovery count, and deauthorize events.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use bytes::Bytes;
use chrono::{TimeZone, Utc};
use hmac::{Hmac, Mac};
use postit_agent_kit::testkit::TestContext;
use postit_agent_mock::{DiscoveryScript, ExchangeScript, MockAgent};
use postit_core::{
    Agent, AgentError, Credentials, ExchangeContext, InboundEvent, OAuthTokens, RawTokenResponse,
    TokenExchange,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value, json};
use sha2::Sha256;
use url::Url;

fn agent() -> MockAgent {
    MockAgent::oauth(&Url::parse("https://provider.test/").unwrap())
}

fn exchange_ctx(ctx: &TestContext) -> ExchangeContext {
    ExchangeContext {
        config: json!({"client_id": "c", "client_secret": "s"}),
        http: ctx.http.clone(),
        clock: ctx.clock.clone(),
    }
}

fn exchange() -> TokenExchange {
    TokenExchange {
        tokens: OAuthTokens {
            access_token: SecretString::from("tok-1".to_owned()),
            refresh_token: None,
            expires_at: None,
            scopes: vec!["read".to_owned()],
        },
        raw: RawTokenResponse::new(Map::new()),
    }
}

fn sign(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

fn event(kind: &str, body: &str, signature: Option<&str>) -> InboundEvent {
    InboundEvent {
        kind: kind.to_owned(),
        path_params: BTreeMap::new(),
        headers: signature
            .map(|s| vec![("X-Mock-Signature".to_owned(), s.as_bytes().to_vec())])
            .unwrap_or_default(),
        raw_body: Bytes::from(body.to_owned()),
    }
}

#[tokio::test]
async fn exchange_defaults_to_mock_user() {
    let agent = agent();
    let ctx = TestContext::new();
    let grant = agent
        .after_exchange(&exchange_ctx(&ctx), exchange())
        .await
        .unwrap();
    assert_eq!(grant.remote_user_id.as_deref(), Some("mock-user"));
    assert!(grant.reconnect_by.is_none());
    assert!(grant.account_tokens.is_empty());
    assert_eq!(grant.tokens.access_token.expose_secret(), "tok-1");
    assert_eq!(agent.script().exchange_calls(), 1);
}

#[tokio::test]
async fn exchange_script_sets_tokens_and_reconnect_by() {
    let agent = agent();
    let when = Utc.with_ymd_and_hms(2026, 12, 1, 0, 0, 0).unwrap();
    agent.script().set_exchange(ExchangeScript {
        remote_user_id: None,
        reconnect_by: Some(when),
        account_tokens: vec!["a1".to_owned(), "a2".to_owned()],
        fail: false,
    });
    let ctx = TestContext::new();
    let grant = agent
        .after_exchange(&exchange_ctx(&ctx), exchange())
        .await
        .unwrap();
    assert!(grant.remote_user_id.is_none());
    assert_eq!(grant.reconnect_by, Some(when));
    let tokens: Vec<_> = grant
        .account_tokens
        .iter()
        .map(|(id, t)| (id.as_str(), t.access_token.expose_secret().to_owned()))
        .collect();
    assert_eq!(
        tokens,
        vec![
            ("a1", "acct-token-a1".to_owned()),
            ("a2", "acct-token-a2".to_owned())
        ]
    );
}

#[tokio::test]
async fn exchange_failure_is_authentication() {
    let agent = agent();
    agent.script().set_exchange(ExchangeScript {
        fail: true,
        ..ExchangeScript::default()
    });
    let ctx = TestContext::new();
    let err = agent
        .after_exchange(&exchange_ctx(&ctx), exchange())
        .await
        .unwrap_err();
    assert!(matches!(err, AgentError::Authentication { .. }), "{err:?}");
    assert_eq!(agent.script().exchange_calls(), 1);
}

#[tokio::test]
async fn oauth_discovery_returns_scripted_count() {
    let agent = agent();
    let tokens = exchange().tokens;
    let ctx = TestContext::new().with_credentials(Credentials::OAuth(tokens));
    let client = common::client(&agent, &ctx);
    let one = client.discover_accounts().await.unwrap();
    assert_eq!(one.accounts.len(), 1);
    assert_eq!(one.accounts[0].remote_id, "mock-account-oauth");

    agent.script().set_oauth_accounts(3);
    let three = client.discover_accounts().await.unwrap();
    let ids: Vec<_> = three
        .accounts
        .iter()
        .map(|a| a.remote_id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "mock-account-oauth-1",
            "mock-account-oauth-2",
            "mock-account-oauth-3"
        ]
    );
}

#[tokio::test]
async fn deauthorize_with_valid_signature_names_the_user() {
    let agent = agent();
    let ctx = TestContext::new();
    let body = r#"{"user_id":"u-7"}"#;
    let sig = sign("s", body.as_bytes());
    let outcome = agent
        .handle_event(
            &ctx.event_context(json!({"client_id": "c", "client_secret": "s"})),
            event("deauthorize", body, Some(&sig)),
        )
        .await
        .unwrap();
    assert_eq!(outcome.response.status, 200);
    assert!(outcome.response.body.is_empty());
    assert_eq!(outcome.deauthorized, vec!["u-7".to_owned()]);
}

#[tokio::test]
async fn deauthorize_with_bad_signature_is_refused() {
    let agent = agent();
    let ctx = TestContext::new();
    let config: Value = json!({"client_id": "c", "client_secret": "s"});
    let body = r#"{"user_id":"u-7"}"#;
    let valid = sign("s", body.as_bytes());
    for signature in [
        Some(sign("other", body.as_bytes())),
        Some("not-hex".to_owned()),
        // The contract is lowercase hex exactly: no uppercase, no surrounding whitespace.
        Some(valid.to_uppercase()),
        Some(format!(" {valid} ")),
        None,
    ] {
        let err = agent
            .handle_event(
                &ctx.event_context(config.clone()),
                event("deauthorize", body, signature.as_deref()),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AgentError::Authentication { .. }), "{err:?}");
    }
}

#[tokio::test]
async fn unknown_event_kind_is_unsupported() {
    let agent = agent();
    let ctx = TestContext::new();
    let err = agent
        .handle_event(
            &ctx.event_context(json!({"client_secret": "s"})),
            event("other", "{}", None),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AgentError::Unsupported { .. }), "{err:?}");
}

#[tokio::test]
async fn oauth_discovery_script_refreshes_then_fails() {
    let agent = agent();
    let ctx = TestContext::new().with_credentials(Credentials::OAuth(exchange().tokens));
    let client = common::client(&agent, &ctx);
    agent.script().set_oauth_discovery(DiscoveryScript {
        refresh_to: Some("tok-refreshed".to_owned()),
        delay: Some(std::time::Duration::from_millis(1)),
        fail: true,
    });
    let err = client.discover_accounts().await.unwrap_err();
    assert!(matches!(err, AgentError::Transport { .. }), "{err:?}");
    let refreshed = ctx.credentials.refreshed();
    assert_eq!(refreshed.len(), 1);
    assert_eq!(refreshed[0].access_token.expose_secret(), "tok-refreshed");
}
