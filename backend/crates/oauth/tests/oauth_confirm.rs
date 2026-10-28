//! Confirming an OAuth result into a grant, accounts, and account tokens, against the mock
//! OAuth provider and `mock_oauth`.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use common::{SECRET, mock_agent, oauth_agent, oauth_config, oauth_provider, platform, platforms};
use postit_agent_kit::testkit::MockOAuthProvider;
use postit_agent_mock::{ExchangeScript, MockScript};
use postit_core::{AccountId, GrantId, OAuthStateId, UserId};
use postit_data::OwnerScope;
use postit_data::accounts::{AccountTokenRepo, GrantRepo};
use postit_data::audit::AuditContext;
use postit_data::oauth_states::OAuthStateRepo;
use postit_data::testkit::seed_user;
use postit_oauth::testkit::{
    TestOptions, account_service_with, test_accounts_settings, test_vault,
};
use postit_oauth::{
    AccountChange, AccountError, AccountService, CallbackParams, Confirmed, ConnectKind,
};
use secrecy::SecretString;
use serde_json::Value;
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;

const OAUTH: &str = "mock_oauth";
const DONE: &str = "https://postit.local:44315/oauth-done";

struct Fixture {
    service: AccountService,
    provider: MockOAuthProvider,
    script: MockScript,
    owner: UserId,
    scope: OwnerScope,
}

fn options() -> TestOptions {
    let mut configs = BTreeMap::new();
    configs.insert(platform(OAUTH), Some(oauth_config()));
    TestOptions {
        configs,
        ..TestOptions::default()
    }
}

async fn fixture(pool: &PgPool) -> Fixture {
    platforms(pool, &[OAUTH]).await;
    let provider = oauth_provider().await;
    let (agent, script) = oauth_agent(&provider, OAUTH);
    let service = account_service_with(
        pool.clone(),
        vec![agent],
        test_accounts_settings(),
        options(),
    );
    let owner = seed_user(pool, "active").await;
    Fixture {
        service,
        provider,
        script,
        owner,
        scope: OwnerScope::own(owner),
    }
}

fn query(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

/// `scope` starts a connection, the provider approves it, and some browser completes the
/// callback: the `connect_id` the redirect carries.
async fn connected_by(f: &Fixture, scope: &OwnerScope) -> String {
    let start = f
        .service
        .start_oauth(scope, &platform(OAUTH), DONE)
        .await
        .unwrap();
    let code = f.provider.approve(&start.authorize_url);
    let state = query(&start.authorize_url, "state").unwrap();
    let target = f
        .service
        .oauth_callback(
            OAUTH,
            CallbackParams {
                code: Some(SecretString::from(code)),
                state: Some(state),
                error: None,
            },
        )
        .await;
    query(&target, "connect_id").unwrap_or_else(|| panic!("callback failed: {target}"))
}

async fn connected(f: &Fixture) -> String {
    connected_by(f, &f.scope).await
}

async fn confirm(
    f: &Fixture,
    scope: &OwnerScope,
    connect_id: &str,
    ids: Option<&[&str]>,
) -> Result<Confirmed, AccountError> {
    f.service
        .confirm(
            scope,
            connect_id,
            ids.map(|ids| ids.iter().map(|id| (*id).to_owned()).collect()),
            AuditContext::default(),
        )
        .await
}

fn account(n: &str) -> String {
    format!("mock-account-{n}")
}

async fn count(pool: &PgPool, sql: &str, owner: UserId) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .bind(owner.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn grants_of(pool: &PgPool, owner: UserId) -> i64 {
    count(
        pool,
        "SELECT count(*) FROM account_grants WHERE owner_id = $1",
        owner,
    )
    .await
}

async fn accounts_of(pool: &PgPool, owner: UserId) -> i64 {
    count(
        pool,
        "SELECT count(*) FROM connected_accounts WHERE owner_id = $1",
        owner,
    )
    .await
}

#[derive(sqlx::FromRow)]
struct StateRow {
    id: Uuid,
    connect_id_hash: Option<Vec<u8>>,
    grant_tokens: Option<Vec<u8>>,
    remote_user_id: Option<String>,
    expired: bool,
}

async fn state_rows(pool: &PgPool) -> Vec<StateRow> {
    sqlx::query_as(
        "SELECT id, connect_id_hash, grant_tokens, remote_user_id, expires_at <= now() AS expired
         FROM oauth_states ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// The pending result's sealed `grant_tokens`, opened.
fn open_state_tokens(row: &StateRow) -> Value {
    let sealed: postit_vault::Sealed<Value> =
        postit_vault::Sealed::from_bytes(row.grant_tokens.clone().unwrap()).unwrap();
    test_vault()
        .open(
            &sealed,
            &OAuthStateRepo::seal_context(OAuthStateId::from(row.id), "grant_tokens"),
        )
        .unwrap()
}

/// The only state row is a discarded result: unreachable by `connect_id`, expired, and still
/// holding its sealed tokens and remote user for the purge to revoke.
async fn assert_only_discarded(pool: &PgPool) {
    let rows = state_rows(pool).await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert!(row.connect_id_hash.is_none());
    assert!(row.expired);
    assert_eq!(row.remote_user_id.as_deref(), Some("mock-user"));
    assert!(open_state_tokens(row)["access_token"].is_string());
}

#[derive(sqlx::FromRow)]
struct GrantRow {
    id: Uuid,
    kind: String,
    remote_user_id: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    reconnect_by: Option<DateTime<Utc>>,
    scopes: Vec<String>,
}

async fn grants(pool: &PgPool, owner: UserId) -> Vec<GrantRow> {
    sqlx::query_as(
        "SELECT id, kind, remote_user_id, expires_at, reconnect_by, scopes
         FROM account_grants WHERE owner_id = $1 ORDER BY id",
    )
    .bind(owner.as_uuid())
    .fetch_all(pool)
    .await
    .unwrap()
}

/// The access token sealed in a grant's credential.
async fn grant_access_token(pool: &PgPool, owner: UserId, grant: GrantId) -> String {
    let mut conn = pool.acquire().await.unwrap();
    let grant = GrantRepo::get(&mut conn, &OwnerScope::own(owner), grant)
        .await
        .unwrap();
    let opened: Value = test_vault()
        .open(
            grant.credential.as_ref().unwrap(),
            &GrantRepo::seal_context(grant.id),
        )
        .unwrap();
    opened["access_token"].as_str().unwrap().to_owned()
}

/// The account token stored for an account, opened.
async fn account_token(pool: &PgPool, owner: UserId, account: AccountId) -> Option<String> {
    let mut conn = pool.acquire().await.unwrap();
    let (sealed, _) = AccountTokenRepo::get(&mut conn, &OwnerScope::own(owner), account)
        .await
        .unwrap()?;
    let opened: Value = test_vault()
        .open(&sealed, &AccountTokenRepo::seal_context(account))
        .unwrap();
    Some(opened.as_str().unwrap().to_owned())
}

fn bytes_contain(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

fn not_found(result: &Result<Confirmed, AccountError>) {
    assert!(
        matches!(result, Err(AccountError::NotFound)),
        "expected NotFound: {result:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn confirm_creates_oauth_grant_and_chosen_accounts_only(pool: PgPool) {
    let f = fixture(&pool).await;
    f.script.set_oauth_accounts(3);
    f.provider
        .set_granted_scopes(Some(vec!["read".to_owned(), "write".to_owned()]));
    let reconnect_by = DateTime::from_timestamp(Utc::now().timestamp() + 86_400 * 60, 0).unwrap();
    f.script.set_exchange(ExchangeScript {
        reconnect_by: Some(reconnect_by),
        ..ExchangeScript::default()
    });
    let connect_id = connected(&f).await;

    let chosen = [account("oauth-1"), account("oauth-3"), "nowhere".to_owned()];
    let chosen: Vec<&str> = chosen.iter().map(String::as_str).collect();
    let confirmed = confirm(&f, &f.scope, &connect_id, Some(&chosen))
        .await
        .unwrap();

    assert_eq!(confirmed.connect, ConnectKind::OAuth);
    assert_eq!(confirmed.skipped_elsewhere, 0);
    assert_eq!(confirmed.unknown, ["nowhere"]);
    let mut ids: Vec<&str> = confirmed
        .connected
        .iter()
        .map(|(a, change)| {
            assert_eq!(*change, AccountChange::Created);
            a.remote_account_id.as_str()
        })
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, [account("oauth-1"), account("oauth-3")]);

    let grants = grants(&pool, f.owner).await;
    assert_eq!(grants.len(), 1);
    let grant = &grants[0];
    assert_eq!(grant.kind, "oauth");
    assert_eq!(grant.remote_user_id.as_deref(), Some("mock-user"));
    assert!(grant.expires_at.unwrap() > Utc::now());
    assert_eq!(grant.reconnect_by, Some(reconnect_by));
    assert_eq!(grant.scopes, ["read", "write"]);
    for (account, _) in &confirmed.connected {
        assert_eq!(account.grant_id.map(|g| g.as_uuid()), Some(grant.id));
        assert_eq!(account.scopes, ["read", "write"]);
    }
    assert_eq!(accounts_of(&pool, f.owner).await, 2);
    // The tokens moved into the grant, so the result is gone.
    assert!(state_rows(&pool).await.is_empty());
    let events = count(
        &pool,
        "SELECT count(*) FROM audit_events WHERE kind = 'account_connected' AND owner_id = $1",
        f.owner,
    )
    .await;
    assert_eq!(events, 2);
    not_found(&confirm(&f, &f.scope, &connect_id, None).await);
    assert!(f.provider.revoked().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn several_accounts_share_one_grant_token(pool: PgPool) {
    let f = fixture(&pool).await;
    f.script.set_oauth_accounts(2);
    let connect_id = connected(&f).await;
    let exchanged = open_state_tokens(&state_rows(&pool).await[0]);
    let access = exchanged["access_token"].as_str().unwrap().to_owned();

    let confirmed = confirm(&f, &f.scope, &connect_id, None).await.unwrap();
    assert_eq!(confirmed.connected.len(), 2);
    let grant_ids: Vec<GrantId> = confirmed
        .connected
        .iter()
        .map(|(a, _)| a.grant_id.unwrap())
        .collect();
    assert_eq!(grant_ids[0], grant_ids[1]);
    for grant in grant_ids {
        assert_eq!(grant_access_token(&pool, f.owner, grant).await, access);
    }
    // Sealed, never in clear.
    let stored: Vec<u8> = sqlx::query_scalar("SELECT credential FROM account_grants")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!bytes_contain(&stored, &access));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn confirm_by_other_user_is_404_and_discards(pool: PgPool) {
    let f = fixture(&pool).await;
    let connect_id = connected(&f).await;
    let stranger = seed_user(&pool, "active").await;

    not_found(&confirm(&f, &OwnerScope::own(stranger), &connect_id, None).await);
    not_found(&confirm(&f, &f.scope, &connect_id, None).await);
    assert_only_discarded(&pool).await;
    assert_eq!(grants_of(&pool, f.owner).await, 0);
    assert_eq!(grants_of(&pool, stranger).await, 0);
    assert_eq!(accounts_of(&pool, f.owner).await, 0);
    assert!(f.provider.revoked().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expired_result_creates_nothing(pool: PgPool) {
    let f = fixture(&pool).await;
    let connect_id = connected(&f).await;
    sqlx::query("UPDATE oauth_states SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();

    not_found(&confirm(&f, &f.scope, &connect_id, None).await);
    // Discarded, not deleted: the purge still has the tokens to revoke.
    assert_only_discarded(&pool).await;
    assert_eq!(grants_of(&pool, f.owner).await, 0);
    assert_eq!(accounts_of(&pool, f.owner).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn confirm_with_nothing_chosen_discards(pool: PgPool) {
    let f = fixture(&pool).await;
    f.script.set_oauth_accounts(2);
    for chosen in [&[][..], &["nowhere"][..]] {
        let connect_id = connected(&f).await;
        let confirmed = confirm(&f, &f.scope, &connect_id, Some(chosen))
            .await
            .unwrap();
        assert_eq!(confirmed.connect, ConnectKind::OAuth);
        assert!(confirmed.connected.is_empty());
        assert_eq!(confirmed.unknown.len(), chosen.len());
        not_found(&confirm(&f, &f.scope, &connect_id, None).await);
        assert_only_discarded(&pool).await;
        assert_eq!(grants_of(&pool, f.owner).await, 0);
        assert_eq!(accounts_of(&pool, f.owner).await, 0);
        sqlx::query("DELETE FROM oauth_states")
            .execute(&pool)
            .await
            .unwrap();
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn confirm_with_every_account_elsewhere_discards(pool: PgPool) {
    let f = fixture(&pool).await;
    f.script.set_oauth_accounts(2);
    let other = OwnerScope::own(seed_user(&pool, "active").await);
    let theirs = connected_by(&f, &other).await;
    confirm(&f, &other, &theirs, None).await.unwrap();

    let connect_id = connected(&f).await;
    let confirmed = confirm(&f, &f.scope, &connect_id, None).await.unwrap();
    assert!(confirmed.connected.is_empty());
    assert_eq!(confirmed.skipped_elsewhere, 2);
    assert_eq!(grants_of(&pool, f.owner).await, 0);
    assert_eq!(accounts_of(&pool, f.owner).await, 0);
    assert_eq!(grants_of(&pool, other.owner).await, 1);
    assert_only_discarded(&pool).await;
    not_found(&confirm(&f, &f.scope, &connect_id, None).await);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn attacker_initiated_connection_attaches_nothing(pool: PgPool) {
    let f = fixture(&pool).await;
    let attacker = OwnerScope::own(seed_user(&pool, "active").await);
    let victim = f.scope.clone();

    // The attacker starts; the victim's browser completes the callback and gets the
    // `connect_id`, which the attacker never sees.
    let connect_id = connected_by(&f, &attacker).await;

    for guess in [
        "",
        "guessed-connect-id",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    ] {
        not_found(&confirm(&f, &attacker, guess, None).await);
    }
    // The victim is not the starter: refused, and the result is discarded.
    not_found(&confirm(&f, &victim, &connect_id, None).await);
    assert_only_discarded(&pool).await;
    // Nor can the attacker use it afterwards.
    not_found(&confirm(&f, &attacker, &connect_id, None).await);

    for user in [attacker.owner, victim.owner] {
        assert_eq!(grants_of(&pool, user).await, 0);
        assert_eq!(accounts_of(&pool, user).await, 0);
    }
    assert!(f.provider.revoked().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn account_connected_elsewhere_is_skipped_and_counted(pool: PgPool) {
    let f = fixture(&pool).await;
    f.script.set_oauth_accounts(2);
    let other = OwnerScope::own(seed_user(&pool, "active").await);
    let theirs = connected_by(&f, &other).await;
    let held = account("oauth-2");
    confirm(&f, &other, &theirs, Some(&[held.as_str()]))
        .await
        .unwrap();

    let connect_id = connected(&f).await;
    let confirmed = confirm(&f, &f.scope, &connect_id, None).await.unwrap();
    assert_eq!(confirmed.connect, ConnectKind::OAuth);
    assert_eq!(confirmed.skipped_elsewhere, 1);
    assert_eq!(confirmed.connected.len(), 1);
    assert_eq!(
        confirmed.connected[0].0.remote_account_id,
        account("oauth-1")
    );
    assert_eq!(grants_of(&pool, f.owner).await, 1);
    assert!(state_rows(&pool).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reconnect_moves_account_and_deletes_old_grant_without_revoke(pool: PgPool) {
    let f = fixture(&pool).await;
    let first = connected(&f).await;
    let before = confirm(&f, &f.scope, &first, None).await.unwrap();
    let old_grant = before.connected[0].0.grant_id.unwrap();

    let second = connected(&f).await;
    let after = confirm(&f, &f.scope, &second, None).await.unwrap();
    assert_eq!(after.connected.len(), 1);
    let (moved, change) = &after.connected[0];
    assert_eq!(*change, AccountChange::Moved);
    assert_eq!(moved.id, before.connected[0].0.id);
    assert_ne!(moved.grant_id, Some(old_grant));

    let grants = grants(&pool, f.owner).await;
    assert_eq!(grants.len(), 1);
    assert_eq!(Some(GrantId::from(grants[0].id)), moved.grant_id);
    assert!(state_rows(&pool).await.is_empty());
    assert!(f.provider.revoked().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn account_tokens_stored_for_chosen_accounts(pool: PgPool) {
    let f = fixture(&pool).await;
    f.script.set_oauth_accounts(2);
    f.script.set_exchange(ExchangeScript {
        account_tokens: vec![account("oauth-1")],
        ..ExchangeScript::default()
    });
    let connect_id = connected(&f).await;
    let confirmed = confirm(&f, &f.scope, &connect_id, None).await.unwrap();

    let by_remote: BTreeMap<&str, AccountId> = confirmed
        .connected
        .iter()
        .map(|(a, _)| (a.remote_account_id.as_str(), a.id))
        .collect();
    let first = by_remote[account("oauth-1").as_str()];
    let second = by_remote[account("oauth-2").as_str()];
    let expected = format!("acct-token-{}", account("oauth-1"));
    assert_eq!(
        account_token(&pool, f.owner, first).await.as_deref(),
        Some(expected.as_str())
    );
    assert_eq!(account_token(&pool, f.owner, second).await, None);
    let stored: Vec<u8> = sqlx::query_scalar("SELECT access_token FROM account_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!bytes_contain(&stored, &expected));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reconnect_drops_stale_account_token(pool: PgPool) {
    let f = fixture(&pool).await;
    f.script.set_oauth_accounts(1);
    f.script.set_exchange(ExchangeScript {
        account_tokens: vec![account("oauth-1")],
        ..ExchangeScript::default()
    });
    let first = connected(&f).await;
    let before = confirm(&f, &f.scope, &first, None).await.unwrap();
    let id = before.connected[0].0.id;
    assert!(account_token(&pool, f.owner, id).await.is_some());

    f.script.set_exchange(ExchangeScript::default());
    let second = connected(&f).await;
    let after = confirm(&f, &f.scope, &second, None).await.unwrap();
    assert_eq!(after.connected[0].1, AccountChange::Moved);
    assert_eq!(after.connected[0].0.id, id);
    assert_eq!(account_token(&pool, f.owner, id).await, None);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn credential_connect_never_calls_after_exchange(pool: PgPool) {
    platforms(&pool, &["mock", OAUTH]).await;
    let provider = oauth_provider().await;
    let (oauth, oauth_script) = oauth_agent(&provider, OAUTH);
    let (credentials, script) = mock_agent("mock");
    let service = account_service_with(
        pool.clone(),
        vec![credentials, oauth],
        test_accounts_settings(),
        options(),
    );
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);

    let check = service
        .start_credentials(&scope, &platform("mock"), common::values(SECRET))
        .await
        .unwrap();
    let connect_id = check.pending.unwrap().0;
    let confirmed = service
        .confirm(&scope, connect_id.as_str(), None, AuditContext::default())
        .await
        .unwrap();
    assert_eq!(confirmed.connect, ConnectKind::Credentials);
    assert!(!confirmed.connected.is_empty());

    assert_eq!(script.exchange_calls(), 0);
    assert_eq!(oauth_script.exchange_calls(), 0);
    assert_eq!(provider.token_requests(), 0);
    let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM account_grants")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(kinds, ["credentials"]);
    let tokens: i64 = sqlx::query_scalar("SELECT count(*) FROM account_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tokens, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn confirm_on_a_switched_off_platform_discards(pool: PgPool) {
    let f = fixture(&pool).await;
    let connect_id = connected(&f).await;
    postit_data::platforms::PlatformRepo::set_enabled(&pool, &platform(OAUTH), false)
        .await
        .unwrap();

    let refused = confirm(&f, &f.scope, &connect_id, None).await;
    assert!(
        refused.is_err() && !matches!(refused, Err(AccountError::NotFound)),
        "{refused:?}"
    );
    assert_only_discarded(&pool).await;
    assert_eq!(grants_of(&pool, f.owner).await, 0);
    assert_eq!(accounts_of(&pool, f.owner).await, 0);
}
