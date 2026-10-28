//! Refreshing OAuth grants: the cron, the on-demand path behind `PgCredentialStore`, and the
//! per-grant lock, against the mock OAuth provider and `mock_oauth`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use common::{oauth_agent_with, oauth_config, oauth_provider, platform, platforms};
use postit_agent_kit::testkit::{MockOAuthProvider, OAuthFailure};
use postit_core::{
    AccountId, AccountRef, AgentError, CredentialStore, Credentials, GrantId, OAuthProviderSpec,
    OAuthTokens, UserId,
};
use postit_data::OwnerScope;
use postit_data::accounts::{
    AccountRepo, AccountTokenRepo, GrantKind, GrantRepo, NewAccount, NewGrant,
};
use postit_data::testkit::seed_user;
use postit_oauth::jobs::run_refresh_credentials;
use postit_oauth::testkit::{
    TestOptions, account_service_with, test_accounts_settings, test_vault,
};
use postit_oauth::{AccountService, PgCredentialStore};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const OAUTH: &str = "mock_oauth";

struct Fixture {
    pool: PgPool,
    service: AccountService,
    provider: MockOAuthProvider,
    owner: UserId,
}

fn options() -> TestOptions {
    let mut configs = BTreeMap::new();
    configs.insert(platform(OAUTH), Some(oauth_config()));
    TestOptions {
        configs,
        ..TestOptions::default()
    }
}

fn service_on(
    pool: &PgPool,
    provider: &MockOAuthProvider,
    edit: impl FnOnce(&mut OAuthProviderSpec),
) -> AccountService {
    let (agent, _) = oauth_agent_with(provider, OAUTH, edit);
    account_service_with(
        pool.clone(),
        vec![agent],
        test_accounts_settings(),
        options(),
    )
}

async fn fixture_with(pool: &PgPool, edit: impl FnOnce(&mut OAuthProviderSpec)) -> Fixture {
    platforms(pool, &[OAUTH]).await;
    let provider = oauth_provider().await;
    let service = service_on(pool, &provider, edit);
    let owner = seed_user(pool, "active").await;
    Fixture {
        pool: pool.clone(),
        service,
        provider,
        owner,
    }
}

async fn fixture(pool: &PgPool) -> Fixture {
    fixture_with(pool, |_| {}).await
}

/// A spec whose refresh tokens rotate, so a refresh is sent once and a 5xx is never resent.
async fn rotating_fixture(pool: &PgPool) -> Fixture {
    fixture_with(pool, |spec| spec.refresh.rotates = true).await
}

/// What a test grant holds.
struct Seed<'a> {
    access: &'a str,
    refresh: Option<&'a str>,
    expires_in: Option<Duration>,
    accounts: &'a [&'a str],
}

impl Default for Seed<'_> {
    fn default() -> Self {
        Self {
            access: "old-access",
            refresh: Some("old-refresh"),
            expires_in: Some(Duration::hours(1)),
            accounts: &[],
        }
    }
}

/// An active OAuth grant of `owner` on `mock_oauth`, with its accounts.
async fn grant(f: &Fixture, owner: UserId, seed: Seed<'_>) -> GrantId {
    let id = GrantId::from(Uuid::now_v7());
    let credential = test_vault()
        .seal(
            &json!({"access_token": seed.access, "refresh_token": seed.refresh}),
            &GrantRepo::seal_context(id),
        )
        .unwrap();
    let scope = OwnerScope::own(owner);
    let mut conn = f.pool.acquire().await.unwrap();
    GrantRepo::insert(
        &mut conn,
        &scope,
        NewGrant {
            id,
            platform: platform(OAUTH),
            kind: GrantKind::Oauth,
            remote_user_id: Some("mock-user".to_owned()),
            credential,
            connection_config: json!({}),
            expires_at: seed.expires_in.map(|ttl| Utc::now() + ttl),
            reconnect_by: None,
            scopes: vec!["read".to_owned(), "write".to_owned()],
        },
    )
    .await
    .unwrap();
    for remote in seed.accounts {
        AccountRepo::insert_or_reactivate(
            &mut conn,
            &scope,
            NewAccount {
                id: AccountId::from(Uuid::now_v7()),
                grant_id: id,
                platform: platform(OAUTH),
                remote_account_id: (*remote).to_owned(),
                display_name: (*remote).to_owned(),
                avatar_url: None,
                scopes: vec![],
                test_account: false,
            },
        )
        .await
        .unwrap();
    }
    id
}

/// The grant's stored tokens, read without refreshing.
struct Stored {
    access: String,
    refresh: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    status: String,
}

async fn stored(f: &Fixture, grant: GrantId) -> Stored {
    let (credential, expires_at, status): (Option<Vec<u8>>, Option<DateTime<Utc>>, String) =
        sqlx::query_as("SELECT credential, expires_at, status FROM account_grants WHERE id = $1")
            .bind(grant.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let sealed = postit_vault::Sealed::<Value>::from_bytes(credential.unwrap()).unwrap();
    let opened: Value = test_vault()
        .open(&sealed, &GrantRepo::seal_context(grant))
        .unwrap();
    Stored {
        access: opened["access_token"].as_str().unwrap().to_owned(),
        refresh: opened["refresh_token"].as_str().map(str::to_owned),
        expires_at,
        status,
    }
}

async fn account_statuses(f: &Fixture, grant: GrantId) -> Vec<String> {
    sqlx::query_scalar("SELECT status FROM connected_accounts WHERE grant_id = $1 ORDER BY id")
        .bind(grant.as_uuid())
        .fetch_all(&f.pool)
        .await
        .unwrap()
}

/// `account_needs_reconnect` events recorded with no actor.
async fn reconnect_events(f: &Fixture) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM audit_events
         WHERE kind = 'account_needs_reconnect' AND actor_user_id IS NULL",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap()
}

fn store(f: &Fixture, grant: GrantId) -> PgCredentialStore {
    PgCredentialStore::new(f.service.clone(), f.owner, grant)
}

async fn oauth_tokens(store: &PgCredentialStore) -> OAuthTokens {
    match store.credentials().await.unwrap() {
        Credentials::OAuth(tokens) => tokens,
        Credentials::Fields(_) => panic!("expected OAuth tokens"),
    }
}

fn is_authentication<T: std::fmt::Debug>(result: &Result<T, AgentError>) -> bool {
    matches!(result, Err(AgentError::Authentication { .. }))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cron_refreshes_grants_expiring_within_15_minutes_only(pool: PgPool) {
    let f = fixture(&pool).await;
    let due = grant(
        &f,
        f.owner,
        Seed {
            expires_in: Some(Duration::minutes(10)),
            ..Seed::default()
        },
    )
    .await;
    let later = grant(
        &f,
        f.owner,
        Seed {
            expires_in: Some(Duration::minutes(30)),
            ..Seed::default()
        },
    )
    .await;
    let never = grant(
        &f,
        f.owner,
        Seed {
            expires_in: None,
            ..Seed::default()
        },
    )
    .await;

    run_refresh_credentials(&f.service, Utc::now(), 100)
        .await
        .unwrap();

    assert_eq!(f.provider.token_requests(), 1);
    let refreshed = stored(&f, due).await;
    assert_eq!(refreshed.access, "access-1");
    assert_eq!(refreshed.refresh.as_deref(), Some("old-refresh"));
    assert!(refreshed.expires_at.unwrap() > Utc::now() + Duration::minutes(50));
    assert_eq!(refreshed.status, "active");
    for untouched in [later, never] {
        assert_eq!(stored(&f, untouched).await.access, "old-access");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn on_demand_refresh_after_401(pool: PgPool) {
    let f = fixture(&pool).await;
    let id = grant(&f, f.owner, Seed::default()).await;
    let store = store(&f, id);
    let stale = oauth_tokens(&store).await;
    assert_eq!(stale.access_token.expose_secret(), "old-access");
    assert_eq!(f.provider.token_requests(), 0);

    let fresh = store.refresh(&stale).await.unwrap();
    assert_eq!(fresh.access_token.expose_secret(), "access-1");
    assert_eq!(f.provider.token_requests(), 1);
    let current = oauth_tokens(&store).await;
    assert_eq!(current.access_token.expose_secret(), "access-1");
    assert_eq!(
        current
            .refresh_token
            .as_ref()
            .map(|t| t.expose_secret().to_owned()),
        Some("old-refresh".to_owned())
    );

    // A second caller holding the same stale tokens gets the new ones without a request.
    let again = store.refresh(&stale).await.unwrap();
    assert_eq!(again.access_token.expose_secret(), "access-1");
    assert_eq!(f.provider.token_requests(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expired_access_token_is_refreshed_before_it_is_served(pool: PgPool) {
    let f = fixture(&pool).await;
    let id = grant(
        &f,
        f.owner,
        Seed {
            expires_in: Some(Duration::minutes(-5)),
            ..Seed::default()
        },
    )
    .await;

    let tokens = oauth_tokens(&store(&f, id)).await;
    assert_eq!(tokens.access_token.expose_secret(), "access-1");
    assert_eq!(f.provider.token_requests(), 1);
    assert_eq!(stored(&f, id).await.access, "access-1");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn rotation_is_stored(pool: PgPool) {
    let f = rotating_fixture(&pool).await;
    f.provider.set_rotation(true);
    let id = grant(&f, f.owner, Seed::default()).await;
    let stale = oauth_tokens(&store(&f, id)).await;

    let fresh = f.service.refresh_grant(id, Some(&stale)).await.unwrap();
    assert_eq!(
        fresh.refresh_token.as_ref().unwrap().expose_secret(),
        "refresh-1"
    );
    let row = stored(&f, id).await;
    assert_eq!(row.access, "access-1");
    assert_eq!(row.refresh.as_deref(), Some("refresh-1"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn no_rotation_keeps_the_old_refresh_token(pool: PgPool) {
    let f = fixture(&pool).await;
    // The response carries no `refresh_token` at all.
    f.provider.set_refresh_issued(false);
    let id = grant(&f, f.owner, Seed::default()).await;
    let stale = oauth_tokens(&store(&f, id)).await;

    let fresh = f.service.refresh_grant(id, Some(&stale)).await.unwrap();
    assert_eq!(
        fresh.refresh_token.as_ref().unwrap().expose_secret(),
        "old-refresh"
    );
    let row = stored(&f, id).await;
    assert_eq!(row.access, "access-1");
    assert_eq!(row.refresh.as_deref(), Some("old-refresh"));
}

async fn assert_needs_reconnect(f: &Fixture, id: GrantId, accounts: usize) {
    assert_eq!(stored(f, id).await.status, "needs_reconnect");
    let statuses = account_statuses(f, id).await;
    assert_eq!(statuses.len(), accounts);
    assert!(
        statuses.iter().all(|s| s == "needs_reconnect"),
        "{statuses:?}"
    );
    assert_eq!(reconnect_events(f).await, i64::try_from(accounts).unwrap());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn invalid_grant_sets_needs_reconnect_with_audit(pool: PgPool) {
    let f = fixture(&pool).await;
    let id = grant(
        &f,
        f.owner,
        Seed {
            accounts: &["acct-1", "acct-2"],
            ..Seed::default()
        },
    )
    .await;
    let store = store(&f, id);
    let stale = oauth_tokens(&store).await;
    f.provider.fail_next(OAuthFailure::InvalidGrant);

    assert!(is_authentication(&store.refresh(&stale).await));
    assert_needs_reconnect(&f, id, 2).await;
    assert!(is_authentication(&store.credentials().await));
    assert_eq!(f.provider.token_requests(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expired_refresh_token_sets_needs_reconnect(pool: PgPool) {
    let f = fixture(&pool).await;
    let id = grant(
        &f,
        f.owner,
        Seed {
            expires_in: Some(Duration::minutes(5)),
            accounts: &["acct-1"],
            ..Seed::default()
        },
    )
    .await;
    f.provider.fail_next(OAuthFailure::ExpiredRefreshToken);

    run_refresh_credentials(&f.service, Utc::now(), 100)
        .await
        .unwrap();
    assert_needs_reconnect(&f, id, 1).await;
    // No longer active, so the next run leaves it alone.
    run_refresh_credentials(&f.service, Utc::now(), 100)
        .await
        .unwrap();
    assert_eq!(f.provider.token_requests(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn server_error_leaves_grant_active(pool: PgPool) {
    let f = rotating_fixture(&pool).await;
    let id = grant(
        &f,
        f.owner,
        Seed {
            expires_in: Some(Duration::minutes(5)),
            accounts: &["acct-1"],
            ..Seed::default()
        },
    )
    .await;
    f.provider.fail_next(OAuthFailure::ServerError);

    let failed = f.service.refresh_grant(id, None).await;
    assert!(
        matches!(failed, Err(AgentError::Transport { .. })),
        "{failed:?}"
    );
    let row = stored(&f, id).await;
    assert_eq!(row.status, "active");
    assert_eq!(row.access, "old-access");
    assert_eq!(account_statuses(&f, id).await, ["active"]);
    assert_eq!(reconnect_events(&f).await, 0);
    assert_eq!(f.provider.token_requests(), 1);

    // The next try succeeds.
    let fresh = f.service.refresh_grant(id, None).await.unwrap();
    assert_eq!(fresh.access_token.expose_secret(), "access-1");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expired_without_refresh_token_needs_reconnect(pool: PgPool) {
    let f = fixture(&pool).await;
    let seed = || Seed {
        refresh: None,
        expires_in: Some(Duration::minutes(-30)),
        ..Seed::default()
    };

    // On demand: `credentials()` answers `Authentication` and sends nothing.
    let on_demand = grant(
        &f,
        f.owner,
        Seed {
            accounts: &["acct-1"],
            ..seed()
        },
    )
    .await;
    assert!(is_authentication(&store(&f, on_demand).credentials().await));
    assert_needs_reconnect(&f, on_demand, 1).await;

    // The cron: the same answer, through the same function.
    let by_cron = grant(
        &f,
        f.owner,
        Seed {
            accounts: &["acct-2"],
            ..seed()
        },
    )
    .await;
    run_refresh_credentials(&f.service, Utc::now(), 100)
        .await
        .unwrap();
    assert_eq!(stored(&f, by_cron).await.status, "needs_reconnect");
    assert_eq!(account_statuses(&f, by_cron).await, ["needs_reconnect"]);
    assert_eq!(reconnect_events(&f).await, 2);
    assert!(is_authentication(
        &f.service.refresh_grant(by_cron, None).await
    ));

    // Neither path ever reached the token endpoint, and a later run does not either.
    run_refresh_credentials(&f.service, Utc::now(), 100)
        .await
        .unwrap();
    assert_eq!(f.provider.token_requests(), 0);
    assert_eq!(reconnect_events(&f).await, 2);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unexpired_without_refresh_token_makes_no_request(pool: PgPool) {
    let f = fixture(&pool).await;
    let id = grant(
        &f,
        f.owner,
        Seed {
            refresh: None,
            expires_in: Some(Duration::minutes(10)),
            ..Seed::default()
        },
    )
    .await;

    run_refresh_credentials(&f.service, Utc::now(), 100)
        .await
        .unwrap();
    let store = store(&f, id);
    let tokens = oauth_tokens(&store).await;
    assert_eq!(tokens.access_token.expose_secret(), "old-access");
    let again = store.refresh(&tokens).await.unwrap();
    assert_eq!(again.access_token.expose_secret(), "old-access");
    assert_eq!(f.provider.token_requests(), 0);
    assert_eq!(stored(&f, id).await.status, "active");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cron_does_not_loop_on_grants_that_stay_due(pool: PgPool) {
    let f = rotating_fixture(&pool).await;
    let mut ids = Vec::new();
    for _ in 0..3 {
        ids.push(
            grant(
                &f,
                f.owner,
                Seed {
                    expires_in: Some(Duration::minutes(5)),
                    ..Seed::default()
                },
            )
            .await,
        );
    }
    // A due grant with no refresh token, not expired yet: visited, never sent.
    let no_refresh = grant(
        &f,
        f.owner,
        Seed {
            refresh: None,
            expires_in: Some(Duration::minutes(5)),
            ..Seed::default()
        },
    )
    .await;
    for _ in 0..3 {
        f.provider.fail_next(OAuthFailure::ServerError);
    }

    let run = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        run_refresh_credentials(&f.service, Utc::now(), 2),
    )
    .await
    .expect("the run finishes");
    run.unwrap();

    assert_eq!(f.provider.token_requests(), 3);
    for id in ids.into_iter().chain([no_refresh]) {
        let row = stored(&f, id).await;
        assert_eq!(row.status, "active");
        assert_eq!(row.access, "old-access");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn concurrent_refresh_across_two_pools_makes_one_request(pool: PgPool) {
    let f = fixture(&pool).await;
    // The token endpoint answers slowly, so the second caller waits on the lock.
    f.provider
        .set_delay(Some(std::time::Duration::from_millis(300)));
    let id = grant(&f, f.owner, Seed::default()).await;
    let stale = oauth_tokens(&store(&f, id)).await;

    let other_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with(pool.connect_options().as_ref().clone())
        .await
        .unwrap();
    let other = service_on(&other_pool, &f.provider, |_| {});

    let (a, b) = tokio::join!(
        f.service.refresh_grant(id, Some(&stale)),
        other.refresh_grant(id, Some(&stale)),
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(f.provider.token_requests(), 1);
    assert_eq!(a.access_token.expose_secret(), "access-1");
    assert_eq!(b.access_token.expose_secret(), "access-1");
    other_pool.close().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reconnect_by_passed_sets_needs_reconnect(pool: PgPool) {
    let f = fixture(&pool).await;
    let id = grant(
        &f,
        f.owner,
        Seed {
            expires_in: Some(Duration::days(30)),
            accounts: &["acct-1", "acct-2"],
            ..Seed::default()
        },
    )
    .await;
    let kept = grant(
        &f,
        f.owner,
        Seed {
            expires_in: Some(Duration::days(30)),
            accounts: &["acct-3"],
            ..Seed::default()
        },
    )
    .await;
    sqlx::query(
        "UPDATE account_grants SET reconnect_by = now() - interval '1 minute' WHERE id = $1",
    )
    .bind(id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE account_grants SET reconnect_by = now() + interval '7 days' WHERE id = $1")
        .bind(kept.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    run_refresh_credentials(&f.service, Utc::now(), 100)
        .await
        .unwrap();
    assert_needs_reconnect(&f, id, 2).await;
    let warned: String =
        sqlx::query_scalar("SELECT reconnect_warned FROM account_grants WHERE id = $1")
            .bind(id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(warned, "expired");
    assert_eq!(stored(&f, kept).await.status, "active");

    // Once warned, a later run records nothing more.
    run_refresh_credentials(&f.service, Utc::now(), 100)
        .await
        .unwrap();
    assert_eq!(reconnect_events(&f).await, 2);
    assert_eq!(f.provider.token_requests(), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn account_token_is_served_for_its_account_only(pool: PgPool) {
    let f = fixture(&pool).await;
    let id = grant(
        &f,
        f.owner,
        Seed {
            accounts: &["acct-1", "acct-2"],
            ..Seed::default()
        },
    )
    .await;
    let other_grant = grant(
        &f,
        f.owner,
        Seed {
            accounts: &["acct-3"],
            ..Seed::default()
        },
    )
    .await;
    let scope = OwnerScope::own(f.owner);
    let mut conn = pool.acquire().await.unwrap();
    let by_remote: BTreeMap<String, (AccountId, GrantId)> =
        sqlx::query_as::<_, (Uuid, String, Uuid)>(
            "SELECT id, remote_account_id, grant_id FROM connected_accounts WHERE owner_id = $1",
        )
        .bind(f.owner.as_uuid())
        .fetch_all(&mut *conn)
        .await
        .unwrap()
        .into_iter()
        .map(|(id, remote, grant)| (remote, (AccountId::from(id), GrantId::from(grant))))
        .collect();
    for remote in ["acct-1", "acct-3"] {
        let account = by_remote[remote].0;
        let sealed = test_vault()
            .seal(
                &json!(format!("page-token-{remote}")),
                &AccountTokenRepo::seal_context(account),
            )
            .unwrap();
        AccountTokenRepo::upsert(&mut conn, &scope, account, &sealed, None)
            .await
            .unwrap();
    }
    drop(conn);
    let reference = |remote: &str| AccountRef {
        platform: platform(OAUTH),
        account: by_remote[remote].0,
        remote_id: remote.to_owned(),
    };
    assert_eq!(by_remote["acct-3"].1, other_grant);

    let store = store(&f, id);
    let token = store
        .account_token(&reference("acct-1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token.access_token.expose_secret(), "page-token-acct-1");
    // An account of the grant without a token, and an account of another grant.
    assert!(
        store
            .account_token(&reference("acct-2"))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .account_token(&reference("acct-3"))
            .await
            .unwrap()
            .is_none()
    );
    // A reference whose remote ID does not match its account.
    let mismatched = AccountRef {
        remote_id: "acct-2".to_owned(),
        ..reference("acct-1")
    };
    assert!(store.account_token(&mismatched).await.unwrap().is_none());
    // Another owner's store for the same grant serves nothing.
    let stranger = seed_user(&pool, "active").await;
    let theirs = PgCredentialStore::new(f.service.clone(), stranger, id);
    assert!(
        theirs
            .account_token(&reference("acct-1"))
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn store_refreshed_writes_the_agents_tokens(pool: PgPool) {
    let f = fixture(&pool).await;
    let id = grant(&f, f.owner, Seed::default()).await;
    let store = store(&f, id);
    let expires = DateTime::from_timestamp(Utc::now().timestamp() + 7200, 0).unwrap();
    store
        .store_refreshed(OAuthTokens {
            access_token: SecretString::from("agent-access".to_owned()),
            refresh_token: None,
            expires_at: Some(expires),
            scopes: vec![],
        })
        .await
        .unwrap();
    let row = stored(&f, id).await;
    assert_eq!(row.access, "agent-access");
    assert_eq!(row.refresh.as_deref(), Some("old-refresh"));
    assert_eq!(row.expires_at, Some(expires));
}
