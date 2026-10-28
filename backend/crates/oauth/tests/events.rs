//! Platform events: a signed `deauthorize` revokes every grant of the remote user, whoever owns
//! it, and anything the route should not reveal answers "not found".
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use common::{
    CLIENT_SECRET, oauth_agent, oauth_config, oauth_grant, oauth_provider, platform, platforms,
};
use hmac::{Hmac, Mac};
use postit_agent_kit::testkit::MockOAuthProvider;
use postit_agent_mock::oauth::{DEAUTHORIZE, SIGNATURE_HEADER};
use postit_core::{AccountId, GrantId, InboundEvent};
use postit_data::testkit::seed_user;
use postit_oauth::testkit::{TestOptions, account_service_with, test_accounts_settings};
use postit_oauth::{AccountError, AccountService};
use serde_json::Value;
use sha2::Sha256;
use sqlx::PgPool;

const OAUTH: &str = "mock_oauth";

fn service_with(
    pool: &PgPool,
    provider: &MockOAuthProvider,
    config: Option<Value>,
) -> AccountService {
    let (agent, _) = oauth_agent(provider, OAUTH);
    let mut configs = BTreeMap::new();
    configs.insert(platform(OAUTH), config);
    account_service_with(
        pool.clone(),
        vec![agent],
        test_accounts_settings(),
        TestOptions {
            configs,
            ..TestOptions::default()
        },
    )
}

/// A `kind` event whose body names `user` and whose signature is keyed with `secret`.
fn event(kind: &str, user: &str, secret: &str) -> InboundEvent {
    let body = serde_json::to_vec(&serde_json::json!({ "user_id": user })).unwrap();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(&body);
    let signature = hex::encode(mac.finalize().into_bytes());
    InboundEvent {
        kind: kind.to_owned(),
        path_params: BTreeMap::new(),
        headers: vec![(SIGNATURE_HEADER.to_owned(), signature.into_bytes())],
        raw_body: body.into(),
    }
}

struct Seeded {
    revoked: Vec<(GrantId, AccountId)>,
    kept: (GrantId, AccountId),
}

/// Two owners' grants of `mock-user` and a third owner's grant of `other-user`, each with one
/// account and its account token.
async fn seed(pool: &PgPool) -> Seeded {
    let mut revoked = Vec::new();
    for (refresh, account) in [("r-a", "acct-a"), ("r-b", "acct-b")] {
        let owner = seed_user(pool, "active").await;
        let (grant, accounts) =
            oauth_grant(pool, owner, OAUTH, Some("mock-user"), refresh, &[account]).await;
        revoked.push((grant, accounts[0]));
    }
    let owner = seed_user(pool, "active").await;
    let (grant, accounts) =
        oauth_grant(pool, owner, OAUTH, Some("other-user"), "r-c", &["acct-c"]).await;
    Seeded {
        revoked,
        kept: (grant, accounts[0]),
    }
}

/// The grant's status, whether it holds a credential, its account's status, and whether the
/// account still has an account token.
async fn state(
    pool: &PgPool,
    (grant, account): (GrantId, AccountId),
) -> (String, bool, String, bool) {
    let (grant_status, has_credential): (String, bool) =
        sqlx::query_as("SELECT status, credential IS NOT NULL FROM account_grants WHERE id = $1")
            .bind(grant.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap();
    let account_status: String =
        sqlx::query_scalar("SELECT status FROM connected_accounts WHERE id = $1")
            .bind(account.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap();
    let has_token: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM account_tokens WHERE account_id = $1)")
            .bind(account.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap();
    (grant_status, has_credential, account_status, has_token)
}

fn untouched() -> (String, bool, String, bool) {
    ("active".to_owned(), true, "active".to_owned(), true)
}

#[sqlx::test(migrations = "../data/migrations")]
async fn deauthorize_event_revokes_matching_grants(pool: PgPool) {
    platforms(&pool, &[OAUTH]).await;
    let provider = oauth_provider().await;
    let service = service_with(&pool, &provider, Some(oauth_config()));
    let seeded = seed(&pool).await;

    let response = service
        .handle_platform_event(OAUTH, event(DEAUTHORIZE, "mock-user", CLIENT_SECRET))
        .await
        .unwrap();

    assert_eq!(response.status, 200);
    for &pair in &seeded.revoked {
        assert_eq!(
            state(&pool, pair).await,
            ("revoked".to_owned(), false, "revoked".to_owned(), false)
        );
    }
    assert_eq!(state(&pool, seeded.kept).await, untouched());
    let events: Vec<Value> = sqlx::query_scalar(
        "SELECT details FROM audit_events WHERE kind = 'account_revoked_by_platform'
         ORDER BY details->>'account_id'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let mut expected: Vec<String> = seeded
        .revoked
        .iter()
        .map(|(_, account)| account.to_string())
        .collect();
    expected.sort();
    let recorded: Vec<String> = events
        .iter()
        .map(|details| details["account_id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(recorded, expected);
    assert!(events.iter().all(|details| details["platform"] == OAUTH));
    // The platform already ended the authorization: nothing is sent back to it.
    assert!(provider.revoked().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn deauthorize_with_bad_signature_changes_nothing(pool: PgPool) {
    platforms(&pool, &[OAUTH]).await;
    let provider = oauth_provider().await;
    let service = service_with(&pool, &provider, Some(oauth_config()));
    let seeded = seed(&pool).await;

    let result = service
        .handle_platform_event(OAUTH, event(DEAUTHORIZE, "mock-user", "not-the-secret"))
        .await;

    assert!(
        matches!(result, Err(AccountError::EventRejected)),
        "{result:?}"
    );
    for &pair in &seeded.revoked {
        assert_eq!(state(&pool, pair).await, untouched());
    }
    assert_eq!(state(&pool, seeded.kept).await, untouched());
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE kind = 'account_revoked_by_platform'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(events, 0);
}

/// A body the platform signed but the agent cannot read is the sender's fault, not an outage:
/// `EventMalformed`, never `PlatformUnavailable`, which the route would answer with a 503.
#[sqlx::test(migrations = "../data/migrations")]
async fn signed_but_unreadable_event_is_malformed(pool: PgPool) {
    platforms(&pool, &[OAUTH]).await;
    let provider = oauth_provider().await;
    let service = service_with(&pool, &provider, Some(oauth_config()));
    let body = b"not json".to_vec();
    let mut mac = Hmac::<Sha256>::new_from_slice(CLIENT_SECRET.as_bytes()).unwrap();
    mac.update(&body);
    let signature = hex::encode(mac.finalize().into_bytes());
    let event = InboundEvent {
        kind: DEAUTHORIZE.to_owned(),
        path_params: BTreeMap::new(),
        headers: vec![(SIGNATURE_HEADER.to_owned(), signature.into_bytes())],
        raw_body: body.into(),
    };

    let result = service.handle_platform_event(OAUTH, event).await;

    assert!(
        matches!(result, Err(AccountError::EventMalformed)),
        "{result:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn event_kind_unsupported_is_not_found(pool: PgPool) {
    platforms(&pool, &[OAUTH]).await;
    let provider = oauth_provider().await;
    let service = service_with(&pool, &provider, Some(oauth_config()));

    let result = service
        .handle_platform_event(OAUTH, event("data_deletion", "mock-user", CLIENT_SECRET))
        .await;

    assert!(matches!(result, Err(AccountError::NotFound)), "{result:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn event_for_unconfigured_platform_is_not_found(pool: PgPool) {
    platforms(&pool, &[OAUTH]).await;
    let provider = oauth_provider().await;
    let unconfigured = service_with(&pool, &provider, None);
    let configured = service_with(&pool, &provider, Some(oauth_config()));
    let seeded = seed(&pool).await;
    let deauthorize = || event(DEAUTHORIZE, "mock-user", CLIENT_SECRET);

    // Unconfigured, unknown, and malformed platform IDs all look the same.
    for (service, id) in [
        (&unconfigured, OAUTH),
        (&configured, "no_such_platform"),
        (&configured, "Not A Platform!"),
    ] {
        let result = service.handle_platform_event(id, deauthorize()).await;
        assert!(
            matches!(result, Err(AccountError::NotFound)),
            "{id}: {result:?}"
        );
    }
    // So does a platform an admin switched off.
    postit_data::platforms::PlatformRepo::set_enabled(&pool, &platform(OAUTH), false)
        .await
        .unwrap();
    let result = configured.handle_platform_event(OAUTH, deauthorize()).await;
    assert!(matches!(result, Err(AccountError::NotFound)), "{result:?}");

    for &pair in &seeded.revoked {
        assert_eq!(state(&pool, pair).await, untouched());
    }
}
